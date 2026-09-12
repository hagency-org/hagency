use super::{Cleanup, StartError};
use crate::codex::{
    MAX_REQUEST_MS,
    session::{self, InterruptDisposition, Outcome, Phase, SessionDriver, Settings, Update},
    transport,
};
#[cfg(windows)]
use hagency_platform::WindowsPipe as Receiver;
#[cfg(windows)]
use hagency_platform::WindowsPipe as Sender;
use hagency_platform::{Launch, SupervisedProcess};
use std::{io, path::Path, time::Duration};
#[cfg(unix)]
use tokio::net::unix::pipe::{Receiver, Sender};

type Session = SessionDriver<Receiver, Sender, Receiver>;
const STOP_TIMEOUT: Duration = Duration::from_secs(3);

/// Spawning and stopping use the platform's bounded synchronous guardian API.
/// Call from a host execution worker with a Tokio IO runtime, not an HTTP handler.
/// Dropping an operation also performs bounded stop; it creates no detached task.
pub struct OwnedSession {
    session: Session,
    owner: SupervisedProcess,
    cleanup: Cleanup,
}
impl OwnedSession {
    pub fn spawn(
        guardian: &Path,
        launch: &Launch,
        settings: Settings,
        limits: transport::Limits,
        response_timeout_ms: u64,
    ) -> Result<Self, StartError> {
        if Path::new(settings.cwd()) != launch.directory
            || limits.validate().is_err()
            || response_timeout_ms == 0
            || response_timeout_ms > MAX_REQUEST_MS
            || tokio::runtime::Handle::try_current().is_err()
        {
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
            // These consume checked FIFO descriptors and make only the host
            // endpoint nonblocking. Work keeps ordinary blocking stdio. No
            // spawn_blocking wrappers or uncancellable pipe-reader tasks exist.
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
            Session::new(stdout, stdin, stderr, settings, limits, response_timeout_ms).map_err(
                |_| io::Error::new(io::ErrorKind::InvalidInput, "invalid runner session limits"),
            )
        })();
        match session {
            Ok(session) => Ok(Self {
                session,
                owner,
                cleanup: Cleanup::Pending,
            }),
            Err(error) => {
                let cleanup = observe_stop(&mut owner);
                Err(StartError::Uncertain {
                    kind: error.kind(),
                    cleanup,
                })
            }
        }
    }
    pub fn id(&self) -> u32 {
        self.owner.id()
    }
    pub fn phase(&self) -> Phase {
        self.session.phase()
    }
    pub fn observation_source(&self) -> Result<session::ObservationSource, session::Error> {
        self.session.observation_source()
    }
    pub fn matches_observation_source(&self, source: &session::ObservationSource) -> bool {
        self.session.matches_observation_source(source)
    }
    pub fn protocol_outcome(&self) -> Option<&Outcome> {
        self.session.outcome()
    }
    pub fn cleanup(&self) -> Cleanup {
        self.cleanup
    }
    pub fn transport_termination(&self) -> Option<&transport::Termination> {
        self.session.transport_termination()
    }
    pub fn stderr_snapshot(&self) -> transport::StderrSnapshot {
        self.session.stderr_snapshot()
    }

    /// Stop through the retained owner. A false whole_tree_stopped or an IO
    /// error stays explicit; neither result frees a domain lease or task.
    pub fn stop(&mut self) -> Cleanup {
        self.session.close();
        if !matches!(self.cleanup, Cleanup::Observed(report) if report.scope.whole_tree_stopped) {
            self.cleanup = observe_stop(&mut self.owner);
        }
        self.cleanup
    }

    /// Runtime opt-in only; durable owner grants and finite launch admission
    /// remain the execution host's responsibility.
    pub fn enable_approval_control(
        &mut self,
        policy: session::ApprovalControlPolicy,
    ) -> Result<(), session::Error> {
        let operation = Operation::new(self)?;
        let result = operation.runner.session.enable_approval_control(policy);
        operation.finish(result)
    }
    pub fn approval_deadline(
        &self,
        id: &crate::codex::RequestId,
    ) -> Result<tokio::time::Instant, session::Error> {
        self.session.approval_deadline(id)
    }
    pub fn prepare_approval(
        &mut self,
        response: crate::codex::approval::ApprovalResponse,
    ) -> Result<session::PreparedApproval, session::Error> {
        let operation = Operation::new(self)?;
        let result = operation.runner.session.prepare_approval(response);
        operation.finish(result)
    }
    /// Borrow the existing pinned control future without transferring this
    /// session or its original process owner. A started future drop still stops.
    pub async fn next_observed_or_control<F: std::future::Future + ?Sized>(
        &mut self,
        control: std::pin::Pin<&mut F>,
    ) -> Result<session::ControlUpdate<F::Output>, session::Error> {
        let operation = Operation::new(self)?;
        let result = operation
            .runner
            .session
            .next_observed_or_control(control)
            .await;
        operation.finish(result)
    }
    pub async fn send_prepared_approval(
        &mut self,
        prepared: &mut session::PreparedApproval,
    ) -> Result<session::PreparedUpdate, session::Error> {
        let operation = Operation::new(self)?;
        let result = operation
            .runner
            .session
            .send_prepared_approval(prepared)
            .await;
        operation.finish(result)
    }
    pub async fn initialize(&mut self) -> Result<(), session::Error> {
        let operation = Operation::new(self)?;
        let result = operation.runner.session.initialize().await;
        operation.finish(result)
    }
    pub async fn start_thread(&mut self) -> Result<String, session::Error> {
        let operation = Operation::new(self)?;
        let result = operation.runner.session.start_thread().await;
        operation.finish(result)
    }
    pub async fn start_turn(&mut self, input: String) -> Result<String, session::Error> {
        let operation = Operation::new(self)?;
        let result = operation.runner.session.start_turn(input).await;
        operation.finish(result)
    }
    pub async fn interrupt(&mut self) -> Result<InterruptDisposition, session::Error> {
        let operation = Operation::new(self)?;
        let result = operation.runner.session.interrupt().await;
        operation.finish(result)
    }
    pub async fn next_update(&mut self) -> Result<Update, session::Error> {
        let operation = Operation::new(self)?;
        let result = operation.runner.session.next_update().await;
        operation.finish(result)
    }
    pub async fn next_observed_update(
        &mut self,
    ) -> Result<(Update, session::Observation), session::Error> {
        let operation = Operation::new(self)?;
        let result = operation.runner.session.next_observed_update().await;
        operation.finish(result)
    }
}
impl Drop for OwnedSession {
    fn drop(&mut self) {
        self.stop();
    }
}
fn observe_stop(owner: &mut SupervisedProcess) -> Cleanup {
    match owner.stop(STOP_TIMEOUT) {
        Ok(report) => Cleanup::Observed(report),
        Err(error) => Cleanup::Unknown { kind: error.kind() },
    }
}
struct Operation<'a> {
    runner: &'a mut OwnedSession,
    finished: bool,
}
impl<'a> Operation<'a> {
    fn new(runner: &'a mut OwnedSession) -> Result<Self, session::Error> {
        if runner.cleanup != Cleanup::Pending {
            return Err(session::Error::State);
        }
        Ok(Self {
            runner,
            finished: false,
        })
    }
    fn finish<T>(mut self, result: Result<T, session::Error>) -> Result<T, session::Error> {
        if result.is_err() || self.runner.phase() == Phase::Ended {
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
