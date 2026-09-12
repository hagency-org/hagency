//! Host-local progress attachment. No runtime data constructs a run/route or
//! claims execution authority, canonical completion or Matrix delivery.
use hagency_progress::{
    Accumulator, AttemptId, AttemptOutcome, Emission, Filter, PendingState, RunId, ToolEvent,
    ToolState,
};
use hagency_runtime::{
    codex::session::{
        self, ItemPhase, Observation, ObservationKind, ObservationSource, SessionDriver, ToolKind,
        ToolResult, TurnResult, Update,
    },
    owned::OwnedSession,
};
use tokio::io::{AsyncRead, AsyncWrite};

pub const MAX_RECEIPTS: usize = 1024;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Retirement {
    Host,
    Source,
    Cancelled,
    Runtime,
    InvalidObservation,
    Capacity,
    FailedTurn,
    InterruptedTurn,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("progress attachment source is unavailable or changed")]
    Source,
    #[error("progress attachment has retired")]
    Retired,
    #[error("progress attachment observation order or replay is invalid")]
    Observation,
    #[error("progress attachment receipt limit reached")]
    Capacity,
    #[error("progress policy rejected an observation: {0}")]
    Policy(hagency_progress::Error),
    #[error("native session observation failed: {0}")]
    Runtime(session::Error),
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Diagnostics {
    pub gated_tool_calls: u32,
}

/// One caller-owned immutable run and one exact upstream source. No reset,
/// Deserialize, Debug, serialization or public raw-observation constructor.
pub struct Attachment {
    run: RunId,
    source: ObservationSource,
    policy: Accumulator,
    receipts: Vec<Observation>,
    policy_sequence: u64,
    now: u64,
    retirement: Option<Retirement>,
    diagnostics: Diagnostics,
}
impl Attachment {
    pub fn for_driver<R, W, E>(
        run: RunId,
        filter: Filter,
        driver: &SessionDriver<R, W, E>,
        now: u64,
    ) -> Result<Self, Error> {
        Self::new(
            run,
            filter,
            driver.observation_source().map_err(|_| Error::Source)?,
            now,
        )
    }
    pub fn for_owned(
        run: RunId,
        filter: Filter,
        owner: &OwnedSession,
        now: u64,
    ) -> Result<Self, Error> {
        Self::new(
            run,
            filter,
            owner.observation_source().map_err(|_| Error::Source)?,
            now,
        )
    }
    fn new(run: RunId, filter: Filter, source: ObservationSource, now: u64) -> Result<Self, Error> {
        if source.is_retired() {
            return Err(Error::Source);
        }
        let mut policy = Accumulator::new(run.clone(), filter);
        policy.start(&run, 1, now).map_err(Error::Policy)?;
        Ok(Self {
            run,
            source,
            policy,
            receipts: vec![],
            policy_sequence: 1,
            now,
            retirement: None,
            diagnostics: Diagnostics::default(),
        })
    }
    fn retire_inner(&mut self, reason: Retirement) {
        if self.retirement.is_none() {
            // now only advances after successful policy operations. Retirement
            // cannot fail at that exact scoped clock and never clears custody.
            let _ = self.policy.retire(&self.run, self.now);
            self.retirement = Some(reason);
        }
    }
    fn refresh_source(&mut self) {
        if self.source.is_retired() {
            self.retire_inner(Retirement::Source);
        }
    }
    fn check(&mut self, run: &RunId) -> Result<(), Error> {
        self.refresh_source();
        if &self.run != run {
            self.retire_inner(Retirement::Source);
            return Err(Error::Source);
        }
        if self.retirement.is_some() {
            return Err(Error::Retired);
        }
        Ok(())
    }
    pub fn retirement(&mut self) -> Option<Retirement> {
        self.refresh_source();
        self.retirement
    }
    pub fn diagnostics(&self) -> Diagnostics {
        self.diagnostics
    }
    pub fn retire(&mut self, run: &RunId) -> Result<(), Error> {
        if &self.run != run {
            self.retire_inner(Retirement::Source);
            return Err(Error::Source);
        }
        self.retire_inner(Retirement::Host);
        Ok(())
    }
    pub fn pending_state(&mut self) -> Option<PendingState> {
        self.refresh_source();
        self.policy.pending_state()
    }
    pub fn pending_id(&self) -> Option<&AttemptId> {
        self.policy.pending_id()
    }
    pub fn summary(&mut self, run: &RunId) -> Result<Option<String>, Error> {
        self.check(run)?;
        self.policy.summary().map_err(Error::Policy)
    }
    pub fn claim(&mut self, run: &RunId, now: u64) -> Result<Option<Emission>, Error> {
        self.check(run)?;
        let value = self.policy.claim(run, now).map_err(Error::Policy)?;
        self.now = now;
        Ok(value)
    }
    /// Historical local acceptance inspection remains possible after retirement.
    /// This is not an answer-delivery or Matrix receipt interface.
    pub fn settle(
        &mut self,
        run: &RunId,
        id: &AttemptId,
        now: u64,
        outcome: AttemptOutcome,
    ) -> Result<hagency_progress::Observation, Error> {
        self.refresh_source();
        let result = self
            .policy
            .settle(run, id, now, outcome)
            .map_err(Error::Policy)?;
        self.now = self.now.max(now);
        Ok(result)
    }
    pub fn observe(
        &mut self,
        run: &RunId,
        event: &Observation,
        now: u64,
    ) -> Result<hagency_progress::Observation, Error> {
        self.check(run)?;
        let result = self.observe_inner(event, now);
        if let Err(error) = result {
            self.retire_inner(if error == Error::Capacity {
                Retirement::Capacity
            } else {
                Retirement::InvalidObservation
            });
        }
        result
    }
    fn observe_inner(
        &mut self,
        event: &Observation,
        now: u64,
    ) -> Result<hagency_progress::Observation, Error> {
        if event.source() != &self.source {
            return Err(Error::Source);
        }
        let seq = event.sequence();
        if seq == 0 {
            return Err(Error::Observation);
        }
        if seq <= self.receipts.len() as u64 {
            return if self.receipts[seq as usize - 1] == *event {
                Ok(hagency_progress::Observation::Duplicate)
            } else {
                Err(Error::Observation)
            };
        }
        if seq != self.receipts.len() as u64 + 1 || now < self.now {
            return Err(Error::Observation);
        }
        if self.receipts.len() == MAX_RECEIPTS {
            return Err(Error::Capacity);
        }
        // Quiet/gated events advance the same clock used by claims and
        // settlement, without fabricating a tool event or policy receipt.
        self.policy
            .advance_clock(&self.run, now)
            .map_err(Error::Policy)?;
        self.now = now;
        match event.kind() {
            ObservationKind::Invalidated => return Err(Error::Observation),
            ObservationKind::Tool(tool) if tool.kind() == ToolKind::Unsupported => {
                if tool.phase() == ItemPhase::Active {
                    self.diagnostics.gated_tool_calls += 1;
                }
            }
            ObservationKind::Tool(tool) => {
                let state = match tool.result() {
                    ToolResult::Pending => ToolState::Pending,
                    ToolResult::Completed => ToolState::Completed,
                    ToolResult::Failed => ToolState::Failed,
                    ToolResult::Unconfirmed => ToolState::Unknown,
                };
                let input = if tool.phase() == ItemPhase::Active {
                    ToolEvent::started(
                        tool.id(),
                        if tool.kind() == ToolKind::Command {
                            "Bash"
                        } else {
                            "Edit"
                        },
                        state,
                    )
                } else {
                    ToolEvent::updated(tool.id(), state)
                };
                self.policy
                    .observe_tool(&self.run, self.policy_sequence + 1, now, input)
                    .map_err(Error::Policy)?;
                self.policy_sequence += 1;
            }
            ObservationKind::TurnEnded(TurnResult::Completed) => {
                self.policy
                    .finish(&self.run, self.policy_sequence + 1, now, None)
                    .map_err(Error::Policy)?;
                self.policy_sequence += 1;
            }
            ObservationKind::TurnEnded(result) => self.retire_inner(match result {
                TurnResult::Failed => Retirement::FailedTurn,
                TurnResult::Interrupted => Retirement::InterruptedTurn,
                _ => Retirement::Runtime,
            }),
            ObservationKind::Ignored | ObservationKind::Usage(_) => {}
        }
        self.receipts.push(event.clone());
        Ok(hagency_progress::Observation::Recorded)
    }

    /// Cancellation drops only the borrowed read future. Its existing runtime
    /// guard handles cancellation; the caller still owns the driver and process.
    pub async fn next_driver<R: AsyncRead + Unpin, W: AsyncWrite + Unpin, E: AsyncRead + Unpin>(
        &mut self,
        run: &RunId,
        driver: &mut SessionDriver<R, W, E>,
        now: u64,
    ) -> Result<Update, Error> {
        self.check(run)?;
        if !driver.matches_observation_source(&self.source) {
            self.retire_inner(Retirement::Source);
            return Err(Error::Source);
        }
        let mut guard = ReadGuard {
            attachment: self,
            done: false,
        };
        let result = driver.next_observed_update().await;
        guard.finish(run, result, now)
    }
    pub async fn next_owned(
        &mut self,
        run: &RunId,
        owner: &mut OwnedSession,
        now: u64,
    ) -> Result<Update, Error> {
        self.check(run)?;
        if !owner.matches_observation_source(&self.source) {
            self.retire_inner(Retirement::Source);
            return Err(Error::Source);
        }
        let mut guard = ReadGuard {
            attachment: self,
            done: false,
        };
        let result = owner.next_observed_update().await;
        guard.finish(run, result, now)
    }
}
struct ReadGuard<'a> {
    attachment: &'a mut Attachment,
    done: bool,
}
impl ReadGuard<'_> {
    fn finish(
        &mut self,
        run: &RunId,
        result: Result<(Update, Observation), session::Error>,
        now: u64,
    ) -> Result<Update, Error> {
        self.done = true;
        match result {
            Ok((update, event)) => {
                self.attachment.observe(run, &event, now)?;
                Ok(update)
            }
            Err(error) => {
                self.attachment.retire_inner(Retirement::Runtime);
                Err(Error::Runtime(error))
            }
        }
    }
}
impl Drop for ReadGuard<'_> {
    fn drop(&mut self) {
        if !self.done {
            self.attachment.retire_inner(Retirement::Cancelled);
        }
    }
}
