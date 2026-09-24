use super::{Cleanup, StartError};
use crate::claude::session::{
    ApprovalControlPolicy, ControlUpdate, PermissionDecision, PreparedApproval, PreparedUpdate,
};
use crate::claude::session::{Observation, ObservationSource};
use crate::claude::{
    Message,
    session::{Error, Limits, Phase, SessionDriver, StderrSnapshot, Termination, WriteProgress},
};
use hagency_platform::{Launch, SupervisedProcess};
#[cfg(windows)]
use hagency_platform::{WindowsPipe as Receiver, WindowsPipe as Sender};
use std::{io, path::Path, time::Duration};
#[cfg(unix)]
use tokio::net::unix::pipe::{Receiver, Sender};

/// Host-internal process custody, not a launch-policy or production admission
/// API. Launch is explicit and env-cleared by the platform. Effective runner
/// settings, sandbox, credentials and MCP remain the execution Host's duty.
pub struct OwnedClaudeSession {
    session: SessionDriver<Receiver, Sender, Receiver>,
    owner: SupervisedProcess,
    cleanup: Cleanup,
}
impl OwnedClaudeSession {
    /// Use on an IO-enabled execution worker: guardian startup/stop is bounded
    /// synchronous work. Never detach this owner into an HTTP request task.
    pub fn spawn(guardian: &Path, launch: &Launch, limits: Limits) -> Result<Self, StartError> {
        if limits.validate().is_err() || tokio::runtime::Handle::try_current().is_err() {
            return Err(StartError::Settings);
        }
        let (mut owner, pipes) =
            SupervisedProcess::spawn_piped(guardian, launch).map_err(|error| {
                if error.kind() == io::ErrorKind::Unsupported {
                    StartError::Unsupported
                } else {
                    StartError::Uncertain {
                        kind: error.kind(),
                        cleanup: Cleanup::Unknown { kind: error.kind() },
                    }
                }
            })?;
        let session = (|| {
            #[cfg(unix)]
            let (stdin, stdout, stderr) = {
                let (stdin, stdout, stderr) = pipes.into_parts();
                (
                    Sender::from_owned_fd(stdin)?,
                    Receiver::from_owned_fd(stdout)?,
                    Receiver::from_owned_fd(stderr)?,
                )
            };
            #[cfg(windows)]
            let (stdin, stdout, stderr) = pipes.into_async_parts()?;
            SessionDriver::new(stdout, stdin, stderr, limits).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidInput, "invalid Claude session limits")
            })
        })();
        match session {
            Ok(session) => Ok(Self {
                session,
                owner,
                cleanup: Cleanup::Pending,
            }),
            Err(error) => Err(StartError::Uncertain {
                kind: error.kind(),
                cleanup: observe_stop(&mut owner),
            }),
        }
    }
    pub fn id(&self) -> u32 {
        self.owner.id()
    }
    pub fn phase(&self) -> Phase {
        self.session.phase()
    }
    pub fn session_id(&self) -> Option<&str> {
        self.session.session_id()
    }
    pub fn observation_source(&self) -> Result<ObservationSource, Error> {
        self.session.observation_source()
    }
    pub fn matches_observation_source(&self, source: &ObservationSource) -> bool {
        self.session.matches_observation_source(source)
    }
    pub fn last_observation(&self) -> Option<&Observation> {
        self.session.last_observation()
    }
    pub fn cleanup(&self) -> Cleanup {
        self.cleanup
    }
    pub fn termination(&self) -> Option<&Termination> {
        self.session.termination()
    }
    pub fn write_progress(&self) -> Option<WriteProgress> {
        self.session.write_progress()
    }
    pub fn stderr_snapshot(&self) -> StderrSnapshot {
        self.session.stderr_snapshot()
    }
    pub fn enable_approval_control(&mut self, policy: ApprovalControlPolicy) -> Result<(), Error> {
        let operation = Operation::new(self)?;
        let result = operation.runner.session.enable_approval_control(policy);
        operation.finish(result)
    }
    pub fn approval_deadline(&self, id: &str) -> Result<tokio::time::Instant, Error> {
        self.session.approval_deadline(id)
    }
    pub fn prepare_approval(
        &mut self,
        id: &str,
        decision: PermissionDecision,
    ) -> Result<PreparedApproval, Error> {
        let operation = Operation::new(self)?;
        let result = operation.runner.session.prepare_approval(id, decision);
        operation.finish(result)
    }
    /// ResultObserved is not automatic stop: background activity may survive a
    /// Claude result. Only the retained owner can supply physical stop evidence.
    pub fn stop(&mut self) -> Cleanup {
        self.session.close();
        if !matches!(self.cleanup,Cleanup::Observed(report) if report.scope.whole_tree_stopped) {
            self.cleanup = observe_stop(&mut self.owner);
        }
        self.cleanup
    }
    pub async fn initialize(&mut self) -> Result<(), Error> {
        let operation = Operation::new(self)?;
        let result = operation.runner.session.initialize().await;
        operation.finish(result)
    }
    pub async fn prompt(&mut self, text: &str) -> Result<WriteProgress, Error> {
        let operation = Operation::new(self)?;
        let result = operation.runner.session.prompt(text).await;
        operation.finish(result)
    }
    pub async fn bind_task_mcp(&mut self, helper: crate::claude::TaskMcp) -> Result<(), Error> {
        let operation = Operation::new(self)?;
        let result = operation.runner.session.bind_task_mcp(helper).await;
        operation.finish(result)
    }
    pub async fn next_message(&mut self) -> Result<Message, Error> {
        let operation = Operation::new(self)?;
        let result = operation.runner.session.next_message().await;
        operation.finish(result)
    }
    pub async fn next_or_control<F: std::future::Future + ?Sized>(
        &mut self,
        control: std::pin::Pin<&mut F>,
    ) -> Result<ControlUpdate<F::Output>, Error> {
        let operation = Operation::new(self)?;
        let result = operation.runner.session.next_or_control(control).await;
        operation.finish(result)
    }
    pub async fn send_prepared_approval(
        &mut self,
        prepared: &mut PreparedApproval,
    ) -> Result<PreparedUpdate, Error> {
        let operation = Operation::new(self)?;
        let result = operation
            .runner
            .session
            .send_prepared_approval(prepared)
            .await;
        operation.finish(result)
    }
}
impl Drop for OwnedClaudeSession {
    fn drop(&mut self) {
        self.stop();
    }
}
fn observe_stop(owner: &mut SupervisedProcess) -> Cleanup {
    match owner.stop(Duration::from_secs(3)) {
        Ok(report) => Cleanup::Observed(report),
        Err(error) => Cleanup::Unknown { kind: error.kind() },
    }
}
struct Operation<'a> {
    runner: &'a mut OwnedClaudeSession,
    finished: bool,
}
impl<'a> Operation<'a> {
    fn new(runner: &'a mut OwnedClaudeSession) -> Result<Self, Error> {
        if runner.cleanup != Cleanup::Pending {
            return Err(Error::Closed);
        }
        Ok(Self {
            runner,
            finished: false,
        })
    }
    fn finish<T>(mut self, result: Result<T, Error>) -> Result<T, Error> {
        if result.is_err() {
            self.runner.stop();
        }
        self.finished = true;
        result
    }
}
impl Drop for Operation<'_> {
    fn drop(&mut self) {
        if !self.finished {
            self.runner.stop();
        }
    }
}
