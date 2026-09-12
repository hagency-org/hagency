use super::wire::object;
use super::{
    Decoder, Error, MAX_PENDING, MAX_REQUEST_MS, MAX_SERVER_IDS, Message, RequestId, RpcError,
    encode, text,
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    New,
    Initializing,
    AwaitingInitialized,
    Ready,
    Closed,
}

/// These are upstream correlation values supplied by the host adapter, not
/// Hagency session IDs, authenticated dispatch identity, or approval authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnScope {
    thread_id: String,
    turn_id: String,
}
impl TurnScope {
    pub fn new(thread_id: String, turn_id: String) -> Result<Self, Error> {
        if !text(&thread_id, 256) || !text(&turn_id, 256) {
            return Err(Error::Envelope);
        }
        Ok(Self { thread_id, turn_id })
    }
    pub fn thread_id(&self) -> &str {
        &self.thread_id
    }
    pub fn turn_id(&self) -> &str {
        &self.turn_id
    }
}

/// Payloads remain untrusted. The host must separately validate negotiated
/// schema, thread/turn/item identity, current capability and durable policy.
pub enum Event {
    Initialized {
        result: Value,
    },
    Response {
        id: RequestId,
        method: String,
        result: Result<Value, RpcError>,
    },
    Notification {
        method: String,
        params: Option<Value>,
    },
    ServerRequest {
        id: RequestId,
        method: String,
        params: Option<Value>,
    },
    InterruptAcknowledged {
        scope: TurnScope,
    },
    InterruptRejected {
        scope: TurnScope,
        error: RpcError,
    },
}

enum Kind {
    Initialize,
    Call(String),
    Interrupt(TurnScope),
}
struct Pending {
    kind: Kind,
    deadline: u64,
}
struct ServerPending {
    method: String,
    params: Option<Value>,
    responded: bool,
    deadline: u64,
    thread_id: Option<String>,
}

/// One transport connection, intended for one disposable runner. There are no
/// background tasks or IO here: callers must pump bytes and call tick on time.
pub struct Connection {
    decoder: Decoder,
    phase: Phase,
    next_id: i64,
    clock: u64,
    pending: BTreeMap<RequestId, Pending>,
    server_pending: BTreeMap<RequestId, ServerPending>,
    server_seen: BTreeSet<RequestId>,
}

impl Default for Connection {
    fn default() -> Self {
        Self {
            decoder: Decoder::default(),
            phase: Phase::New,
            next_id: 0,
            clock: 0,
            pending: BTreeMap::new(),
            server_pending: BTreeMap::new(),
            server_seen: BTreeSet::new(),
        }
    }
}

impl Connection {
    pub(super) fn approval_admitted_ms(&self, id: &RequestId) -> Option<u64> {
        self.server_pending
            .get(id)?
            .deadline
            .checked_sub(MAX_REQUEST_MS)
    }
    pub(super) fn has_prepared_approval(&self, id: &RequestId) -> bool {
        self.server_pending
            .get(id)
            .is_some_and(|pending| pending.responded)
    }
    pub(super) fn partial_frame_bytes(&self) -> usize {
        self.decoder.buffered_bytes()
    }
    pub(super) fn deadline_ms(&self) -> Option<u64> {
        self.pending
            .values()
            .map(|p| p.deadline)
            .chain(self.server_pending.values().map(|p| p.deadline))
            .chain(self.decoder.deadline_ms())
            .min()
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }
    pub fn pending_server_count(&self) -> usize {
        self.server_pending.len()
    }

    fn fail<T>(&mut self, error: Error) -> Result<T, Error> {
        self.phase = Phase::Closed;
        self.pending.clear();
        self.server_pending.clear();
        let _ = self.decoder.eof();
        Err(error)
    }

    /// Call from a bounded timer, even during silence or continuous notifications.
    pub fn tick(&mut self, now_ms: u64) -> Result<(), Error> {
        if self.phase == Phase::Closed {
            return Err(Error::Closed);
        }
        if now_ms < self.clock {
            return self.fail(Error::Clock);
        }
        self.clock = now_ms;
        if let Err(error) = self.decoder.check_deadline(now_ms) {
            return self.fail(error);
        }
        if self.pending.values().any(|p| p.deadline <= now_ms)
            || self
                .server_pending
                .values()
                .any(|request| request.deadline <= now_ms)
        {
            return self.fail(Error::Timeout);
        }
        Ok(())
    }

    fn issue(
        &mut self,
        kind: Kind,
        method: &str,
        params: Value,
        now_ms: u64,
        timeout_ms: u64,
    ) -> Result<(RequestId, Vec<u8>), Error> {
        self.tick(now_ms)?;
        if timeout_ms == 0 || timeout_ms > MAX_REQUEST_MS {
            return Err(Error::Envelope);
        }
        let deadline = now_ms.checked_add(timeout_ms).ok_or(Error::Clock)?;
        if self.pending.len() >= MAX_PENDING {
            return Err(Error::Capacity);
        }
        let next = self.next_id.checked_add(1).ok_or(Error::Capacity)?;
        let id = RequestId::Number(self.next_id);
        let bytes = encode(Message::Request {
            id: id.clone(),
            method: method.into(),
            params: Some(params),
            trace: None,
        })?;
        self.pending.insert(id.clone(), Pending { kind, deadline });
        self.next_id = next;
        Ok((id, bytes))
    }

    pub fn initialize(
        &mut self,
        version: &str,
        now_ms: u64,
        timeout_ms: u64,
    ) -> Result<(RequestId, Vec<u8>), Error> {
        if self.phase == Phase::Closed {
            return Err(Error::Closed);
        }
        if self.phase != Phase::New {
            return Err(Error::State);
        }
        if !text(version, 128) {
            return Err(Error::Envelope);
        }
        let request = self.issue(
            Kind::Initialize,
            "initialize",
            json!({
                "clientInfo": { "name": "hagency", "title": "Hagency runner", "version": version }
            }),
            now_ms,
            timeout_ms,
        )?;
        self.phase = Phase::Initializing;
        Ok(request)
    }

    /// The host must write this fully before it writes another returned request.
    /// A transport write failure must call transport_failed, never retry bytes.
    pub fn initialized(&mut self, now_ms: u64) -> Result<Vec<u8>, Error> {
        self.tick(now_ms)?;
        if self.phase != Phase::AwaitingInitialized {
            return Err(Error::State);
        }
        let bytes = encode(Message::Notification {
            method: "initialized".into(),
            params: Some(object()),
        })?;
        self.phase = Phase::Ready;
        Ok(bytes)
    }

    /// Host-side wire primitive, not a runtime-facing tool or authorization API.
    /// Full request schema/policy validation belongs to the future typed adapter.
    pub fn request(
        &mut self,
        method: &str,
        params: Value,
        now_ms: u64,
        timeout_ms: u64,
    ) -> Result<(RequestId, Vec<u8>), Error> {
        self.tick(now_ms)?;
        if self.phase != Phase::Ready
            || matches!(method, "initialize" | "initialized" | "turn/interrupt")
        {
            return Err(Error::State);
        }
        self.issue(
            Kind::Call(method.into()),
            method,
            params,
            now_ms,
            timeout_ms,
        )
    }

    pub fn interrupt(
        &mut self,
        scope: TurnScope,
        now_ms: u64,
        timeout_ms: u64,
    ) -> Result<(RequestId, Vec<u8>), Error> {
        self.tick(now_ms)?;
        if self.phase != Phase::Ready {
            return Err(Error::State);
        }
        if self
            .pending
            .values()
            .any(|p| matches!(&p.kind, Kind::Interrupt(existing) if existing == &scope))
        {
            return Err(Error::State);
        }
        let params = json!({ "threadId": scope.thread_id, "turnId": scope.turn_id });
        self.issue(
            Kind::Interrupt(scope),
            "turn/interrupt",
            params,
            now_ms,
            timeout_ms,
        )
    }

    /// Refuse requests when no typed host coordinator is attached.
    pub fn reject_server_request(&mut self, id: &RequestId, now_ms: u64) -> Result<Vec<u8>, Error> {
        self.tick(now_ms)?;
        if self
            .server_pending
            .get(id)
            .is_none_or(|pending| pending.responded)
        {
            return Err(Error::Identity);
        }
        let bytes = encode(Message::Error {
            id: id.clone(),
            error: RpcError {
                code: -32601,
                message: "Native runner request handler is unavailable".into(),
                data: None,
            },
        })?;
        self.server_pending.remove(id);
        Ok(bytes)
    }

    /// Exact typed host response; ownership/decision consumption is the
    /// permissions coordinator's responsibility. Retain the pending scope until
    /// upstream resolution, which proves neither selected policy nor execution.
    pub fn respond_approval(
        &mut self,
        response: super::approval::ApprovalResponse,
        now_ms: u64,
    ) -> Result<Vec<u8>, Error> {
        self.tick(now_ms)?;
        let request = &response.request;
        let pending = self
            .server_pending
            .get_mut(request.id())
            .ok_or(Error::Identity)?;
        if pending.responded
            || pending.method != request.method()
            || pending.params.as_ref() != Some(request.params())
        {
            return Err(Error::Identity);
        }
        let bytes = encode(Message::Response {
            id: request.id().clone(),
            result: response.result,
        })?;
        pending.responded = true;
        Ok(bytes)
    }

    pub fn receive(&mut self, bytes: &[u8], now_ms: u64) -> Result<(usize, Option<Event>), Error> {
        self.tick(now_ms)?;
        let (consumed, message) = match self.decoder.feed(bytes, now_ms) {
            Ok(value) => value,
            Err(error) => return self.fail(error),
        };
        let Some(message) = message else {
            return Ok((consumed, None));
        };
        match self.accept(message, now_ms) {
            Ok(event) => Ok((consumed, Some(event))),
            Err(error) => self.fail(error),
        }
    }

    fn accept(&mut self, message: Message, now_ms: u64) -> Result<Event, Error> {
        match message {
            Message::Response { id, result } => self.response(id, Ok(result)),
            Message::Error { id, error } => self.response(id, Err(error)),
            Message::Notification { method, params } => {
                if !matches!(
                    self.phase,
                    Phase::Initializing | Phase::AwaitingInitialized | Phase::Ready
                ) {
                    return Err(Error::State);
                }
                if method == "serverRequest/resolved" {
                    self.server_resolved(params.as_ref().ok_or(Error::Envelope)?)?;
                }
                // No model text, item event, turn event or arbitrary notification
                // can resolve a host request or mutate durable task truth.
                Ok(Event::Notification { method, params })
            }
            Message::Request {
                id,
                method,
                params,
                trace: _,
            } => {
                if self.phase != Phase::Ready {
                    return Err(Error::State);
                }
                if self.server_seen.contains(&id) {
                    return Err(Error::Identity);
                }
                if self.server_seen.len() >= MAX_SERVER_IDS
                    || self.server_pending.len() >= MAX_PENDING
                {
                    return Err(Error::Capacity);
                }
                let deadline = now_ms.checked_add(MAX_REQUEST_MS).ok_or(Error::Clock)?;
                let thread_id = params
                    .as_ref()
                    .and_then(|p| p.get("threadId"))
                    .map(|value| {
                        value
                            .as_str()
                            .filter(|v| text(v, 256))
                            .map(str::to_owned)
                            .ok_or(Error::Envelope)
                    })
                    .transpose()?;
                // Only typed approval responses need a content-bound copy.
                // Bound it before cloning; unrelated unsupported callbacks keep
                // no payload copy in the connection's pending table.
                let approval = matches!(
                    method.as_str(),
                    "item/commandExecution/requestApproval"
                        | "item/fileChange/requestApproval"
                        | "item/permissions/requestApproval"
                );
                let response_params = if approval
                    && serde_json::to_vec(&params)
                        .map_err(|_| Error::Envelope)?
                        .len()
                        <= super::approval::MAX_APPROVAL_BYTES
                {
                    params.clone()
                } else {
                    // Generic protocol ingress keeps its existing frame/queue
                    // bounds. Oversized requests have no response capability;
                    // the typed session rejects them without another copy.
                    None
                };
                self.server_seen.insert(id.clone());
                self.server_pending.insert(
                    id.clone(),
                    ServerPending {
                        deadline,
                        thread_id,
                        method: method.clone(),
                        params: response_params,
                        responded: false,
                    },
                );
                Ok(Event::ServerRequest { id, method, params })
            }
        }
    }

    fn server_resolved(&mut self, params: &Value) -> Result<(), Error> {
        let id: RequestId =
            serde_json::from_value(params.get("requestId").ok_or(Error::Envelope)?.clone())
                .map_err(|_| Error::Envelope)?;
        let thread_id = params
            .get("threadId")
            .and_then(Value::as_str)
            .ok_or(Error::Envelope)?;
        if !id.valid() || !text(thread_id, 256) {
            return Err(Error::Envelope);
        }
        if let Some(request) = self.server_pending.get(&id)
            && request.thread_id.as_deref() != Some(thread_id)
        {
            return Err(Error::Identity);
        }
        if !self.server_seen.contains(&id) && self.server_seen.len() >= MAX_SERVER_IDS {
            return Err(Error::Capacity);
        }
        // A completion observed before its request is a tombstone too.
        self.server_seen.insert(id.clone());
        self.server_pending.remove(&id);
        Ok(())
    }

    fn response(&mut self, id: RequestId, result: Result<Value, RpcError>) -> Result<Event, Error> {
        let pending = self.pending.remove(&id).ok_or(Error::Identity)?;
        match pending.kind {
            Kind::Initialize => {
                if self.phase != Phase::Initializing {
                    return Err(Error::State);
                }
                let result = result.map_err(|_| Error::State)?;
                let obj = result.as_object().ok_or(Error::Envelope)?;
                for key in ["userAgent", "platformFamily", "platformOs", "codexHome"] {
                    if !obj
                        .get(key)
                        .and_then(Value::as_str)
                        .is_some_and(|v| text(v, 4096))
                    {
                        return Err(Error::Envelope);
                    }
                }
                // These are observations, not effective sandbox/OS qualification.
                self.phase = Phase::AwaitingInitialized;
                Ok(Event::Initialized { result })
            }
            Kind::Call(method) => Ok(Event::Response { id, method, result }),
            Kind::Interrupt(scope) => match result {
                Ok(value) if value.as_object().is_some_and(|o| o.is_empty()) => {
                    Ok(Event::InterruptAcknowledged { scope })
                }
                Ok(_) => Err(Error::Envelope),
                Err(error) => Ok(Event::InterruptRejected { scope, error }),
            },
        }
    }

    pub fn transport_failed(&mut self) -> Result<(), Error> {
        if self.phase == Phase::Closed {
            return Err(Error::Closed);
        }
        self.fail(Error::Transport)
    }

    pub fn eof(&mut self) -> Result<(), Error> {
        if self.phase == Phase::Closed {
            return Err(Error::Closed);
        }
        let result = self.decoder.eof();
        let pending = !self.pending.is_empty() || !self.server_pending.is_empty();
        self.phase = Phase::Closed;
        self.pending.clear();
        self.server_pending.clear();
        if pending || result.is_err() {
            Err(Error::UnexpectedEof)
        } else {
            Ok(())
        }
    }
}
