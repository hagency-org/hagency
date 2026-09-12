//! Runtime mechanics only. The host separately owns all durable approval grants.
use super::*;
use crate::codex::approval::ApprovalResponse;
use std::{collections::BTreeMap, future::Future, pin::Pin};

const MAX_CALLBACKS: usize = 16;

/// Explicit finite maintenance for owner interaction, not an execution renewal.
/// Both fields are milliseconds. Their sum must fit the ORIGINAL transport
/// lifetime; the response reserve must cover its existing write timeout.
#[derive(Clone, Copy, Debug)]
pub struct ApprovalControlPolicy {
    pub owner_wait_ms: u64,
    pub response_reserve_ms: u64,
}

/// A successful control output is not a runner observation or owner authority.
pub enum ControlUpdate<T> {
    Update(Update, Box<super::super::Observation>),
    Control(T),
}

/// Every observed update must reach the host before it continues this unsent
/// frame. WriteAccepted proves only complete AsyncWrite acceptance and flush.
pub enum PreparedUpdate {
    Update(Update, Box<super::super::Observation>),
    WriteAccepted(transport::TransportWrite),
}

/// Original one-shot typed frame, deliberately neither Clone nor Deserialize.
/// Retain this in the execution owner BEFORE awaiting durable response begin.
/// Association/deadline data does not authorize a write or prove application.
pub struct PreparedApproval {
    source: Arc<AtomicBool>,
    id: RequestId,
    frame: transport::PreparedFrame,
    response_deadline: Instant,
}
impl PreparedApproval {
    /// Fixed original bound for response admission plus write completion. It
    /// never changes when control wakes or a new callback requires rechecking.
    pub fn response_deadline(&self) -> Instant {
        self.response_deadline
    }

    pub fn request_id(&self) -> &RequestId {
        &self.id
    }
}

enum CallbackStage {
    Waiting,
    Prepared,
    Sent,
}
struct Callback {
    owner_deadline: Instant,
    response_deadline: Instant,
    stage: CallbackStage,
}
#[derive(Default)]
pub(super) struct ControlState {
    policy: Option<ApprovalControlPolicy>,
    callbacks: BTreeMap<RequestId, Callback>,
    read_deadline: Option<Instant>,
}
impl ControlState {
    pub(super) fn enabled(&self) -> bool {
        self.policy.is_some()
    }
    pub(super) fn admit<R, W, E>(
        &mut self,
        wire: &transport::Driver<R, W, E>,
        id: &RequestId,
    ) -> Result<(), Error> {
        let Some(policy) = self.policy else {
            return Ok(());
        };
        if self.callbacks.len() >= MAX_CALLBACKS || self.callbacks.contains_key(id) {
            return Err(Error::Capacity);
        }
        let (owner_deadline, response_deadline) = wire
            .approval_deadlines(id, policy.owner_wait_ms, policy.response_reserve_ms)
            .map_err(Error::Transport)?;
        self.callbacks.insert(
            id.clone(),
            Callback {
                owner_deadline,
                response_deadline,
                stage: CallbackStage::Waiting,
            },
        );
        Ok(())
    }
    pub(super) fn resolve(&mut self, id: &RequestId) {
        self.callbacks.remove(id);
    }
    fn deadline<R, W, E>(&mut self, wire: &transport::Driver<R, W, E>) -> Result<Instant, Error> {
        if !self.enabled() {
            return Err(Error::State);
        }
        let deadline = self
            .callbacks
            .values()
            .filter_map(|c| match c.stage {
                CallbackStage::Waiting => Some(c.owner_deadline),
                CallbackStage::Prepared => Some(c.response_deadline),
                CallbackStage::Sent => None,
            })
            .min()
            .unwrap_or_else(|| {
                *self
                    .read_deadline
                    .get_or_insert_with(|| wire.event_deadline())
            });
        if Instant::now() >= deadline {
            return Err(Error::Transport(transport::Error::Timeout));
        }
        Ok(deadline)
    }
}

impl<R, W, E> SessionDriver<R, W, E> {
    /// Host opt-in alone authorizes no response. The future execution host must
    /// obtain the exact durable grant before sending and fit owner TTL plus its
    /// admission margin in the original launch. Its present 30s policy is not
    /// changed by this API. Default sessions continue to refuse callbacks.
    pub fn enable_approval_control(&mut self, policy: ApprovalControlPolicy) -> Result<(), Error> {
        if self.phase() != Phase::Running || self.approvals_enabled {
            return Err(Error::State);
        }
        if !self
            .wire
            .control_policy_fits(policy.owner_wait_ms, policy.response_reserve_ms)
        {
            return Err(Error::Settings);
        }
        self.control.policy = Some(policy);
        self.approvals_enabled = true;
        Ok(())
    }

    /// Original monotonic owner deadline, for bounding owner decision wait.
    /// PreparedApproval separately exposes the fixed response admission bound. It is association data, not a wall-clock or durable verdict.
    pub fn approval_deadline(&self, id: &RequestId) -> Result<Instant, Error> {
        self.control
            .callbacks
            .get(id)
            .map(|c| c.owner_deadline)
            .ok_or(Error::Scope)
    }

    /// Synchronously reserve and encode the exact original callback once. This
    /// is a wire primitive, not domain authorization. The caller must compare
    /// the durable grant's complete association and retain this value before
    /// awaiting response-begin. Lost/unknown admission never authorizes sending.
    pub fn prepare_approval(
        &mut self,
        response: ApprovalResponse,
    ) -> Result<PreparedApproval, Error> {
        if self.phase() != Phase::Running || !self.control.enabled() {
            return Err(Error::State);
        }
        let operation = Operation {
            session: self,
            finished: false,
        };
        let result = operation.session.prepare_inner(response);
        operation.finish(result)
    }
    fn prepare_inner(&mut self, response: ApprovalResponse) -> Result<PreparedApproval, Error> {
        if self.thread_id() != Some(response.request.thread_id())
            || self.turn_id() != Some(response.request.turn_id())
        {
            return Err(Error::Scope);
        }
        self.control.deadline(&self.wire)?;
        let id = response.request.id().clone();
        let callback = self.control.callbacks.get(&id).ok_or(Error::Scope)?;
        if !matches!(callback.stage, CallbackStage::Waiting) {
            return Err(Error::State);
        }
        if Instant::now() >= callback.owner_deadline {
            return Err(Error::Transport(transport::Error::Timeout));
        }
        let response_deadline = callback.response_deadline;
        let frame = self
            .wire
            .prepare_approval(response, callback.response_deadline)
            .map_err(Error::Transport)?;
        self.control
            .callbacks
            .get_mut(&id)
            .ok_or(Error::Scope)?
            .stage = CallbackStage::Prepared;
        Ok(PreparedApproval {
            response_deadline,
            source: self.observation_live.clone(),
            id,
            frame,
        })
    }
    pub(super) fn advance_observation(&mut self) -> Result<(), Error> {
        self.observation_sequence = self
            .observation_sequence
            .checked_add(1)
            .ok_or(Error::Capacity)?;
        Ok(())
    }
    fn observation(&self) -> Result<super::super::Observation, Error> {
        Ok(super::super::Observation {
            source: self.bound_source()?,
            sequence: self.observation_sequence,
            kind: self.observation_kind.clone(),
        })
    }
}

impl<R: AsyncRead + Unpin, W: AsyncWrite + Unpin, E: AsyncRead + Unpin> SessionDriver<R, W, E> {
    /// Poll an independently retained, pinned host future alongside original IO.
    /// On Update, keep that SAME future pinned and handle the update in order;
    /// on Control, consume its output once and do not poll the completed future.
    /// Keep in-flight grant batches separate from the callback map so processing
    /// a new callback does not require cancelling a future borrowing that batch.
    /// Dropping this started operation still closes the session.
    pub async fn next_observed_or_control<F: Future + ?Sized>(
        &mut self,
        control: Pin<&mut F>,
    ) -> Result<ControlUpdate<F::Output>, Error> {
        if self.phase() != Phase::Running || !self.control.enabled() {
            return Err(Error::State);
        }
        let operation = Operation {
            session: self,
            finished: false,
        };
        let result = operation.session.control_inner(control).await;
        operation.finish(result)
    }
    async fn control_inner<F: Future + ?Sized>(
        &mut self,
        control: Pin<&mut F>,
    ) -> Result<ControlUpdate<F::Output>, Error> {
        let deadline = self.control.deadline(&self.wire)?;
        self.wire.ensure_live().map_err(Error::Transport)?;
        let next = match self.pop()? {
            Some(event) => transport::Controlled::Event(event),
            None => self
                .wire
                .next_or_control(control, deadline)
                .await
                .map_err(Error::Transport)?,
        };
        match next {
            transport::Controlled::Event(event) => {
                let update = self.accept_update(event).await?;
                self.advance_observation()?;
                // A real typed read completes this event operation. Control
                // returns and prepared-send buffer drains never reset it.
                self.control.read_deadline = None;
                Ok(ControlUpdate::Update(update, Box::new(self.observation()?)))
            }
            transport::Controlled::Control(output) => Ok(ControlUpdate::Control(output)),
        }
    }

    /// Send only after positive durable response-begin/recheck for this exact
    /// original grant. On Update, process all new barriers/cancellation and
    /// recheck the original admitted grant before calling again. Runtime has no
    /// domain grant type and cannot perform or substitute that authorization.
    /// An unreceived future result remains unknown, never permission to retry.
    pub async fn send_prepared_approval(
        &mut self,
        prepared: &mut PreparedApproval,
    ) -> Result<PreparedUpdate, Error> {
        if self.phase() != Phase::Running || !self.control.enabled() {
            return Err(Error::State);
        }
        let operation = Operation {
            session: self,
            finished: false,
        };
        let result = operation.session.send_prepared_inner(prepared).await;
        operation.finish(result)
    }
    async fn send_prepared_inner(
        &mut self,
        prepared: &mut PreparedApproval,
    ) -> Result<PreparedUpdate, Error> {
        if !Arc::ptr_eq(&self.observation_live, &prepared.source) {
            return Err(Error::Scope);
        }
        self.control.deadline(&self.wire)?;
        self.wire
            .begin_prepared_send(&mut prepared.frame)
            .map_err(Error::Transport)?;
        if let Some(event) = self.pop()? {
            let update = self.accept_update(event).await?;
            self.advance_observation()?;
            return Ok(PreparedUpdate::Update(
                update,
                Box::new(self.observation()?),
            ));
        }
        match self
            .wire
            .send_prepared_or_event(&mut prepared.frame)
            .await
            .map_err(Error::Transport)?
        {
            transport::Controlled::Event(event) => {
                let update = self.accept_update(event).await?;
                self.advance_observation()?;
                Ok(PreparedUpdate::Update(
                    update,
                    Box::new(self.observation()?),
                ))
            }
            transport::Controlled::Control(receipt) => {
                self.control
                    .callbacks
                    .get_mut(&prepared.id)
                    .ok_or(Error::Scope)?
                    .stage = CallbackStage::Sent;
                Ok(PreparedUpdate::WriteAccepted(receipt))
            }
        }
    }
}
