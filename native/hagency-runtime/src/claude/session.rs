//! One disposable upstream conversation. No task, approval or cleanup authority.
mod control;
mod io;
mod observation;
mod task_mcp;
mod usage;
use super::{ControlOutcome, EventKind, Message};
pub use control::{
    ApprovalControlPolicy, ControlUpdate, PermissionDecision, PreparedApproval, PreparedUpdate,
};
pub use io::{Limits, StderrSnapshot, Termination, WriteProgress};
pub use observation::{MAX_OBSERVATIONS, Observation, ObservationKind, ObservationSource};
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncWrite};
pub use usage::{UsageCounts, UsageCoverage, UsageDiagnostics, UsageEvidence};

const INITIALIZE_ID: &str = "hagency-claude-initialize-1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("invalid Claude session limits")]
    Configuration,
    #[error("invalid Claude session state")]
    State,
    #[error("Claude session is closed")]
    Closed,
    #[error("Claude session operation was cancelled")]
    Cancelled,
    #[error("Claude session deadline exceeded")]
    Timeout,
    #[error("Claude session IO failed ({0})")]
    Io(&'static str),
    #[error("Claude session peer closed")]
    PeerEof,
    #[error("Claude session capacity exceeded")]
    Capacity,
    #[error("Claude session protocol failed: {0}")]
    Protocol(super::Error),
    #[error("Claude initialization was refused")]
    Refused,
    #[error("Claude session identity or control response mismatch")]
    Identity,
    #[error("Claude permission request is cancelled or already answered")]
    PermissionUnavailable,
    #[error("Claude scoped task helper was not connected with its exact tool inventory")]
    TaskWriterStartup,
    #[error("Claude session was closed by its host")]
    HostClosed,
}
impl From<super::Error> for Error {
    fn from(error: super::Error) -> Self {
        Self::Protocol(error)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    New,
    Initializing,
    Ready,
    Running,
    ResultObserved,
    Closed,
}

/// Private payloads deliberately have no Debug/Serialize projection. The caller
/// owns these streams; only OwnedClaudeSession separately owns a process.
pub struct SessionDriver<R, W, E> {
    wire: io::Wire<R, W, E>,
    phase: Phase,
    session_id: Option<String>,
    control: control::State,
    source: Arc<()>,
    observations: observation::State,
    task_mcp: Option<super::TaskMcp>,
    task_mcp_attempted: bool,
}
impl<R, W, E> SessionDriver<R, W, E> {
    pub fn new(stdout: R, stdin: W, stderr: E, limits: Limits) -> Result<Self, Error> {
        Ok(Self {
            wire: io::Wire::new(stdout, stdin, stderr, limits)?,
            phase: Phase::New,
            session_id: None,
            control: control::State::default(),
            source: Arc::new(()),
            observations: Default::default(),
            task_mcp: None,
            task_mcp_attempted: false,
        })
    }
    pub fn phase(&self) -> Phase {
        self.phase
    }
    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }
    pub fn termination(&self) -> Option<&Termination> {
        self.wire.termination()
    }
    pub fn write_progress(&self) -> Option<WriteProgress> {
        self.wire.write_progress()
    }
    pub fn stderr_snapshot(&self) -> StderrSnapshot {
        self.wire.stderr_snapshot()
    }
    pub fn close(&mut self) {
        self.fail(Error::HostClosed);
    }
    fn fail(&mut self, error: Error) {
        self.phase = Phase::Closed;
        self.observations.retire();
        self.control.clear();
        self.wire.close(error);
    }
    fn observe(&mut self, message: &Message, received: tokio::time::Instant) -> Result<(), Error> {
        match message {
            Message::Event {
                session_id,
                kind,
                payload,
            } => {
                let init = *kind == EventKind::System && payload["subtype"] == "init";
                match &self.session_id {
                    None if init => self.session_id = Some(session_id.clone()),
                    Some(bound) if bound == session_id && !init => {}
                    _ => return Err(Error::Identity),
                }
                if *kind == EventKind::Result {
                    self.phase = Phase::ResultObserved;
                }
            }
            Message::Permission { .. } => {
                if self.session_id.is_none() {
                    return Err(Error::Identity);
                }
                self.control.admit(message, received, &self.wire)?;
            }
            Message::ControlCancel { request_id } => self.control.cancel(request_id)?,
            Message::ControlResponse { .. } => return Err(Error::Identity),
        }
        self.control.observed();
        self.observations
            .accept(message, self.session_id.as_deref().ok_or(Error::Identity)?)?;
        Ok(())
    }
}
struct Operation<'a, R, W, E> {
    driver: &'a mut SessionDriver<R, W, E>,
    finished: bool,
}
impl<'a, R, W, E> Operation<'a, R, W, E> {
    fn new(driver: &'a mut SessionDriver<R, W, E>, expected: Phase) -> Result<Self, Error> {
        if driver.phase == Phase::Closed {
            return Err(Error::Closed);
        }
        if driver.phase != expected {
            driver.fail(Error::State);
            return Err(Error::State);
        }
        Ok(Self {
            driver,
            finished: false,
        })
    }
    fn finish<T>(mut self, result: Result<T, Error>) -> Result<T, Error> {
        if let Err(error) = result {
            self.driver.fail(error);
        }
        self.finished = true;
        result
    }
}
impl<R, W, E> Drop for Operation<'_, R, W, E> {
    fn drop(&mut self) {
        if !self.finished {
            self.driver.fail(Error::Cancelled);
        }
    }
}
impl<R: AsyncRead + Unpin, W: AsyncWrite + Unpin, E: AsyncRead + Unpin> SessionDriver<R, W, E> {
    pub async fn initialize(&mut self) -> Result<(), Error> {
        let operation = Operation::new(self, Phase::New)?;
        operation.driver.phase = Phase::Initializing;
        let result = operation.driver.initialize_inner().await;
        operation.finish(result)
    }
    async fn initialize_inner(&mut self) -> Result<(), Error> {
        // One response deadline covers the write and read, not one per event.
        let until = self.wire.event_deadline();
        self.wire
            .send(super::initialize(INITIALIZE_ID)?, until)
            .await?;
        match self.wire.next(until).await?.message {
            Message::ControlResponse {
                request_id,
                outcome,
            } if request_id == INITIALIZE_ID => match outcome {
                ControlOutcome::Success(_) => {
                    self.phase = Phase::Ready;
                    Ok(())
                }
                ControlOutcome::Refused => Err(Error::Refused),
            },
            _ => Err(Error::Identity),
        }
    }
    /// Flushed stdin only: neither execution acknowledgement nor a task receipt.
    pub async fn prompt(&mut self, text: &str) -> Result<WriteProgress, Error> {
        let operation = Operation::new(self, Phase::Ready)?;
        let result = operation.driver.prompt_inner(text).await;
        operation.finish(result)
    }
    async fn prompt_inner(&mut self, text: &str) -> Result<WriteProgress, Error> {
        if self.wire.has_buffered_input() {
            return Err(Error::Identity);
        }
        if text.is_empty() || text.len() > super::MAX_TEXT_BYTES {
            return Err(Error::Protocol(super::Error::Input));
        }
        let guided = self
            .task_mcp
            .as_ref()
            .map(|helper| format!("{}\n\nAssigned task input:\n{}", helper.guidance(), text));
        let bytes = super::prompt(guided.as_deref().unwrap_or(text), None)?;
        self.phase = Phase::Running;
        self.wire.send(bytes, self.wire.event_deadline()).await
    }
    /// Observations alone authorize no reply. The configured execution Host must
    /// still refuse Claude until its original private approval adapter exists.
    pub async fn next_message(&mut self) -> Result<Message, Error> {
        let operation = Operation::new(self, Phase::Running)?;
        let result = operation.driver.next_inner().await;
        operation.finish(result)
    }
    async fn next_inner(&mut self) -> Result<Message, Error> {
        let deadline = self.control.deadline(&self.wire)?;
        let received = self.wire.next(deadline).await?;
        self.observe(&received.message, received.at)?;
        Ok(received.message)
    }
}
