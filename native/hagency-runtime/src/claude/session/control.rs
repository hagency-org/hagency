//! Wire mechanics only. Original durable owner grants remain the Host's duty.
use super::{Error, Message, Operation, Phase, SessionDriver, WriteProgress, io};
use serde_json::{Value, json};
use std::{collections::BTreeMap, future::Future, pin::Pin, sync::Arc};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    time::Instant,
};

const MAX_CALLBACKS: usize = 16;
const MAX_INPUT_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, Debug)]
pub struct ApprovalControlPolicy {
    pub owner_wait_ms: u64,
    pub response_reserve_ms: u64,
}
/// Host-selected wire behavior, not an owner verdict. No JSON constructor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PermissionDecision {
    Allow,
    Deny,
}
pub enum ControlUpdate<T> {
    Message(Message),
    Control(T),
}
pub enum PreparedUpdate {
    Message(Message),
    WriteAccepted(WriteProgress),
}
/// Exactly one original frame. Neither Clone, Deserialize nor arbitrary bytes.
pub struct PreparedApproval {
    source: Arc<()>,
    id: String,
    frame: io::PreparedFrame,
    response_deadline: Instant,
    sequence: u64,
}
impl PreparedApproval {
    pub fn request_id(&self) -> &str {
        &self.id
    }
    pub fn response_deadline(&self) -> Instant {
        self.response_deadline
    }
}
enum Stage {
    Waiting,
    Prepared(u64),
    Sent,
    Cancelled,
}
struct Callback {
    stage: Stage,
    input: Option<Value>,
    owner: Option<Instant>,
    response: Option<Instant>,
}
#[derive(Default)]
pub(super) struct State {
    policy: Option<ApprovalControlPolicy>,
    entries: BTreeMap<String, Callback>,
    read_deadline: Option<Instant>,
    sequence: u64,
}
impl State {
    pub fn clear(&mut self) {
        self.entries.clear();
    }
    pub fn observed(&mut self) {
        self.read_deadline = None;
    }
    pub fn admit<R, W, E>(
        &mut self,
        message: &Message,
        received: Instant,
        wire: &io::Wire<R, W, E>,
    ) -> Result<(), Error> {
        let Message::Permission {
            request_id, input, ..
        } = message
        else {
            return Err(Error::State);
        };
        if self.entries.contains_key(request_id) {
            return Err(Error::Identity);
        }
        if self.entries.len() >= MAX_CALLBACKS {
            return Err(Error::Capacity);
        }
        let (input, owner, response) = if let Some(policy) = self.policy {
            // Input is already a one-MiB/depth-bounded decoded JSON object.
            // This tighter limit bounds all16 retained originals and responses.
            if serde_json::to_vec(input)
                .map_err(|_| Error::Capacity)?
                .len()
                > MAX_INPUT_BYTES
            {
                return Err(Error::Capacity);
            }
            let (owner, response) = wire.permission_deadlines(
                received,
                policy.owner_wait_ms,
                policy.response_reserve_ms,
            )?;
            (Some(input.clone()), Some(owner), Some(response))
        } else {
            (None, None, None)
        };
        self.entries.insert(
            request_id.clone(),
            Callback {
                stage: Stage::Waiting,
                input,
                owner,
                response,
            },
        );
        Ok(())
    }
    pub fn cancel(&mut self, id: &str) -> Result<(), Error> {
        let callback = self.entries.get_mut(id).ok_or(Error::Identity)?;
        if matches!(callback.stage, Stage::Cancelled) {
            return Err(Error::Identity);
        }
        callback.stage = Stage::Cancelled;
        callback.input = None;
        Ok(())
    }
    pub fn deadline<R, W, E>(&mut self, wire: &io::Wire<R, W, E>) -> Result<Instant, Error> {
        if self.policy.is_none() {
            return Ok(wire.event_deadline());
        }
        let until = self
            .entries
            .values()
            .filter_map(|callback| match callback.stage {
                Stage::Waiting => callback.owner,
                Stage::Prepared(_) => callback.response,
                Stage::Sent | Stage::Cancelled => None,
            })
            .min()
            .unwrap_or_else(|| {
                *self
                    .read_deadline
                    .get_or_insert_with(|| wire.event_deadline())
            });
        if Instant::now() >= until {
            return Err(Error::Timeout);
        }
        Ok(until)
    }
}
impl<R, W, E> SessionDriver<R, W, E> {
    /// Explicit host opt-in after original stream identity binding and before
    /// any callback is consumed. Does not authorize an allow or durable response.
    pub fn enable_approval_control(&mut self, policy: ApprovalControlPolicy) -> Result<(), Error> {
        let operation = Operation::new(self, Phase::Running)?;
        let result = operation.driver.enable_inner(policy);
        operation.finish(result)
    }
    fn enable_inner(&mut self, policy: ApprovalControlPolicy) -> Result<(), Error> {
        if self.session_id.is_none()
            || self.control.policy.is_some()
            || !self.control.entries.is_empty()
        {
            return Err(Error::State);
        }
        self.wire.permission_deadlines(
            Instant::now(),
            policy.owner_wait_ms,
            policy.response_reserve_ms,
        )?;
        self.control.policy = Some(policy);
        Ok(())
    }
    pub fn approval_deadline(&self, id: &str) -> Result<Instant, Error> {
        self.control
            .entries
            .get(id)
            .and_then(|callback| callback.owner)
            .ok_or(Error::Identity)
    }
    /// Freeze once from the privately retained original input. The Host must
    /// hold this value before awaiting durable response-begin and must compare
    /// the exact original grant before sending. Encoding grants no authority.
    pub fn prepare_approval(
        &mut self,
        id: &str,
        decision: PermissionDecision,
    ) -> Result<PreparedApproval, Error> {
        let operation = Operation::new(self, Phase::Running)?;
        let result = operation.driver.prepare_inner(id, decision);
        operation.finish(result)
    }
    fn prepare_inner(
        &mut self,
        id: &str,
        decision: PermissionDecision,
    ) -> Result<PreparedApproval, Error> {
        if self.control.policy.is_none() {
            return Err(Error::State);
        }
        let until = self.control.deadline(&self.wire)?;
        self.wire.check(until)?;
        let callback = self.control.entries.get_mut(id).ok_or(Error::Identity)?;
        if !matches!(callback.stage, Stage::Waiting) {
            return Err(Error::PermissionUnavailable);
        }
        if Instant::now() >= callback.owner.ok_or(Error::State)? {
            return Err(Error::Timeout);
        }
        let input = callback.input.take().ok_or(Error::State)?;
        let response = match decision {
            PermissionDecision::Allow => json!({"behavior":"allow","updatedInput":input}),
            PermissionDecision::Deny => {
                json!({"behavior":"deny","message":"Permission denied by Hagency.","interrupt":true})
            }
        };
        let bytes = crate::claude::encode(json!({"type":"control_response","response":{
            "subtype":"success","request_id":id,"response":response}}))?;
        let response_deadline = callback.response.ok_or(Error::State)?;
        self.control.sequence = self
            .control
            .sequence
            .checked_add(1)
            .ok_or(Error::Capacity)?;
        let sequence = self.control.sequence;
        callback.stage = Stage::Prepared(sequence);
        Ok(PreparedApproval {
            source: self.source.clone(),
            id: id.into(),
            frame: io::PreparedFrame::new(sequence, bytes, response_deadline),
            response_deadline,
            sequence,
        })
    }
}
impl<R: AsyncRead + Unpin, W: AsyncWrite + Unpin, E: AsyncRead + Unpin> SessionDriver<R, W, E> {
    /// Keep the SAME host future pinned across Message returns. Control returns
    /// its output once; never poll a completed future. No public read is raced
    /// or cancelled internally; selection happens at cancellation-safe leaf IO.
    pub async fn next_or_control<F: Future + ?Sized>(
        &mut self,
        control: Pin<&mut F>,
    ) -> Result<ControlUpdate<F::Output>, Error> {
        let operation = Operation::new(self, Phase::Running)?;
        let result = operation.driver.control_inner(control).await;
        operation.finish(result)
    }
    async fn control_inner<F: Future + ?Sized>(
        &mut self,
        control: Pin<&mut F>,
    ) -> Result<ControlUpdate<F::Output>, Error> {
        if self.control.policy.is_none() {
            return Err(Error::State);
        }
        let until = self.control.deadline(&self.wire)?;
        match self.wire.next_or_control(control, until).await? {
            io::Controlled::Message(received) => {
                self.observe(&received.message, received.at)?;
                Ok(ControlUpdate::Message(received.message))
            }
            io::Controlled::Control(output) => Ok(ControlUpdate::Control(output)),
        }
    }
    /// Send only following positive original durable grant/begin/recheck. Each
    /// Message return suspends this exact frame: process it, recheck authority,
    /// then continue this same value. No ordinary read resumes its writer.
    pub async fn send_prepared_approval(
        &mut self,
        prepared: &mut PreparedApproval,
    ) -> Result<PreparedUpdate, Error> {
        let operation = Operation::new(self, Phase::Running)?;
        let result = operation.driver.send_inner(prepared).await;
        operation.finish(result)
    }
    async fn send_inner(
        &mut self,
        prepared: &mut PreparedApproval,
    ) -> Result<PreparedUpdate, Error> {
        if !Arc::ptr_eq(&self.source, &prepared.source) {
            return Err(Error::Identity);
        }
        if self.control.policy.is_none() {
            return Err(Error::State);
        }
        let until = self.control.deadline(&self.wire)?;
        self.wire.check(until)?;
        let callback = self
            .control
            .entries
            .get(&prepared.id)
            .ok_or(Error::Identity)?;
        if !matches!(callback.stage,Stage::Prepared(sequence) if sequence==prepared.sequence) {
            return Err(Error::PermissionUnavailable);
        }
        match self.wire.send_prepared(&mut prepared.frame, until).await? {
            io::Controlled::Message(received) => {
                self.observe(&received.message, received.at)?;
                Ok(PreparedUpdate::Message(received.message))
            }
            io::Controlled::Control(receipt) => {
                self.control
                    .entries
                    .get_mut(&prepared.id)
                    .ok_or(Error::Identity)?
                    .stage = Stage::Sent;
                Ok(PreparedUpdate::WriteAccepted(receipt))
            }
        }
    }
}
