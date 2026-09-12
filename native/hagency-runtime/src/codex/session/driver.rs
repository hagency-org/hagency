#[path = "control.rs"]
mod control;
pub use control::{ApprovalControlPolicy, ControlUpdate, PreparedApproval, PreparedUpdate};

use super::state::{State, scope};
use super::{
    Error, InterruptDisposition, MAX_DEFERRED, MAX_DEFERRED_BYTES, MAX_TEXT_BYTES, Outcome, Phase,
    ResumeThreadId, Settings, Update,
};
use crate::codex::{Event, MAX_REQUEST_MS, RequestId, TurnScope, transport};
use serde_json::Value;
use std::collections::VecDeque;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::time::{Instant, timeout_at};

/// One disposable upstream turn. The host supplies already-owned streams and
/// separately holds all process, Hagency task, approval and lease authority.
pub struct SessionDriver<R, W, E> {
    wire: transport::Driver<R, W, E>,
    settings: Settings,
    state: State,
    response_timeout_ms: u64,
    deferred: VecDeque<Event>,
    deferred_bytes: usize,
    approvals_enabled: bool,
    control: control::ControlState,
    observation_live: Arc<AtomicBool>,
    observation_sequence: u64,
    observation_kind: super::ObservationKind,
    observation_evidence: super::observation::EvidenceTracker,
}

impl<R, W, E> SessionDriver<R, W, E> {
    pub fn new(
        stdout: R,
        stdin: W,
        stderr: E,
        settings: Settings,
        limits: transport::Limits,
        response_timeout_ms: u64,
    ) -> Result<Self, Error> {
        if response_timeout_ms == 0 || response_timeout_ms > MAX_REQUEST_MS {
            return Err(Error::Settings);
        }
        Ok(Self {
            wire: transport::Driver::new(stdout, stdin, stderr, limits)
                .map_err(Error::Transport)?,
            settings,
            state: State::default(),
            response_timeout_ms,
            deferred: VecDeque::new(),
            deferred_bytes: 0,
            approvals_enabled: false,
            control: control::ControlState::default(),
            observation_live: Arc::new(AtomicBool::new(true)),
            observation_sequence: 0,
            observation_kind: super::ObservationKind::Ignored,
            observation_evidence: super::observation::EvidenceTracker::default(),
        })
    }
    pub fn settings(&self) -> &Settings {
        &self.settings
    }
    /// Attach once the exact turn is Running, before consuming any updates.
    pub fn observation_source(&self) -> Result<super::ObservationSource, Error> {
        if self.phase() != Phase::Running || self.observation_sequence != 0 {
            return Err(Error::State);
        }
        self.bound_source()
    }
    fn bound_source(&self) -> Result<super::ObservationSource, Error> {
        Ok(super::ObservationSource {
            live: self.observation_live.clone(),
            thread: self.thread_id().ok_or(Error::State)?.into(),
            turn: self.turn_id().ok_or(Error::State)?.into(),
        })
    }
    pub fn matches_observation_source(&self, source: &super::ObservationSource) -> bool {
        self.phase() == Phase::Running
            && !source.is_retired()
            && self.bound_source().is_ok_and(|value| &value == source)
    }
    /// Explicit host opt-in after the upstream thread and turn were validated.
    pub fn enable_approvals(&mut self) -> Result<(), Error> {
        if self.phase() != Phase::Running || self.approvals_enabled {
            return Err(Error::State);
        }
        self.approvals_enabled = true;
        Ok(())
    }
    pub fn phase(&self) -> Phase {
        self.state.phase
    }
    pub fn thread_id(&self) -> Option<&str> {
        self.state.thread.as_deref()
    }
    pub fn turn_id(&self) -> Option<&str> {
        self.state.turn.as_deref()
    }
    pub fn outcome(&self) -> Option<&Outcome> {
        self.state.outcome.as_ref()
    }
    pub fn item_count(&self) -> usize {
        self.state.item_count()
    }
    pub fn text_bytes(&self) -> usize {
        self.state.text_bytes()
    }
    pub fn event_count(&self) -> usize {
        self.state.event_count()
    }
    pub fn transport_termination(&self) -> Option<&transport::Termination> {
        self.wire.termination()
    }
    pub fn stderr_snapshot(&self) -> transport::StderrSnapshot {
        self.wire.stderr_snapshot()
    }

    pub fn close(&mut self) {
        if self.state.phase != Phase::Ended {
            self.fail(Error::Cancelled);
        }
        if !matches!(self.outcome(), Some(Outcome::Completed { .. })) {
            self.observation_live.store(false, Ordering::Release);
        }
        self.wire.close();
    }
    fn fail(&mut self, error: Error) {
        self.observation_live.store(false, Ordering::Release);
        self.state.failed(error);
        self.wire.close();
        self.deferred.clear();
        self.deferred_bytes = 0;
    }
    fn buffer(&mut self, event: Event) -> Result<(), Error> {
        let Event::Notification {
            method,
            params: Some(params),
        } = &event
        else {
            return Err(Error::Malformed);
        };
        // A new thread is unbound until its RPC response. Only thread lifecycle
        // and global notices may race that response; no turn is admitted yet.
        if self.state.phase == Phase::OpeningThread
            && !matches!(
                method.as_str(),
                "thread/started" | "thread/status/changed" | "warning" | "configWarning"
            )
        {
            return Err(Error::Scope);
        }
        scope(
            method,
            params,
            self.state.thread.as_deref(),
            self.state.turn.as_deref(),
        )?;
        let charge = transport::event_bytes(&event).map_err(Error::Transport)?;
        let total = self
            .deferred_bytes
            .checked_add(charge)
            .filter(|&n| n <= MAX_DEFERRED_BYTES)
            .ok_or(Error::Capacity)?;
        if self.deferred.len() >= MAX_DEFERRED {
            return Err(Error::Capacity);
        }
        self.deferred.push_back(event);
        self.deferred_bytes = total;
        Ok(())
    }
    fn validate_deferred(&self) -> Result<(), Error> {
        for event in &self.deferred {
            let Event::Notification {
                method,
                params: Some(params),
            } = event
            else {
                return Err(Error::Malformed);
            };
            scope(
                method,
                params,
                self.state.thread.as_deref(),
                self.state.turn.as_deref(),
            )?;
        }
        Ok(())
    }
    fn pop(&mut self) -> Result<Option<Event>, Error> {
        let Some(event) = self.deferred.pop_front() else {
            return Ok(None);
        };
        self.deferred_bytes = self
            .deferred_bytes
            .checked_sub(transport::event_bytes(&event).map_err(Error::Transport)?)
            .ok_or(Error::Capacity)?;
        Ok(Some(event))
    }
}
impl<R, W, E> Drop for SessionDriver<R, W, E> {
    fn drop(&mut self) {
        if !matches!(self.outcome(), Some(Outcome::Completed { .. })) {
            self.observation_live.store(false, Ordering::Release);
        }
    }
}

// Dropping any started operation poisons the typed state even when cancellation
// happens between lower-level IO calls. Closing streams does not stop a child.
struct Operation<'a, R, W, E> {
    session: &'a mut SessionDriver<R, W, E>,
    finished: bool,
}
impl<R, W, E> Drop for Operation<'_, R, W, E> {
    fn drop(&mut self) {
        if !self.finished {
            self.session.fail(Error::Cancelled);
        }
    }
}
impl<R, W, E> Operation<'_, R, W, E> {
    fn finish<T>(mut self, result: Result<T, Error>) -> Result<T, Error> {
        if let Err(error) = result.as_ref() {
            self.session.fail(*error);
        }
        self.finished = true;
        result
    }
}

impl<R: AsyncRead + Unpin, W: AsyncWrite + Unpin, E: AsyncRead + Unpin> SessionDriver<R, W, E> {
    pub async fn initialize(&mut self) -> Result<(), Error> {
        if self.phase() != Phase::New {
            return Err(Error::State);
        }
        let operation = Operation {
            session: self,
            finished: false,
        };
        let result = operation.session.initialize_inner().await;
        operation.finish(result)
    }
    async fn initialize_inner(&mut self) -> Result<(), Error> {
        self.state.phase = Phase::Initializing;
        self.wire
            .send(transport::Command::Initialize {
                client_version: env!("CARGO_PKG_VERSION").into(),
                response_timeout_ms: self.response_timeout_ms,
            })
            .await
            .map_err(Error::Transport)?;
        loop {
            match self.receive().await? {
                Event::Initialized { .. } => break,
                Event::Notification {
                    method,
                    params: Some(params),
                } if matches!(method.as_str(), "warning" | "configWarning") => {
                    self.state.notification(&method, &params)?;
                }
                _ => return Err(Error::Scope),
            }
        }
        self.wire
            .send(transport::Command::Initialized)
            .await
            .map_err(Error::Transport)?;
        self.state.phase = Phase::Ready;
        Ok(())
    }

    pub async fn start_thread(&mut self) -> Result<String, Error> {
        self.open_thread(None).await
    }
    /// Explicit host-recorded upstream identity only; this never authorizes
    /// automatic resume/replay of a Hagency dispatch or an unknown prior turn.
    pub async fn resume_thread(&mut self, id: ResumeThreadId) -> Result<String, Error> {
        self.open_thread(Some(id)).await
    }
    async fn open_thread(&mut self, resume: Option<ResumeThreadId>) -> Result<String, Error> {
        if self.phase() != Phase::Ready {
            return Err(Error::State);
        }
        let operation = Operation {
            session: self,
            finished: false,
        };
        let result = operation
            .session
            .open_thread_inner(resume.as_ref().map(|id| id.0.as_str()))
            .await;
        operation.finish(result)
    }
    async fn open_thread_inner(&mut self, resume: Option<&str>) -> Result<String, Error> {
        self.state.phase = Phase::OpeningThread;
        // For resume, reject a substituted notification even before its response.
        self.state.thread = resume.map(str::to_owned);
        let method = if resume.is_some() {
            "thread/resume"
        } else {
            "thread/start"
        };
        let result = self
            .call(method, self.settings.thread_request(resume))
            .await?;
        let id = self.state.observe_thread(&result, &self.settings, resume)?;
        self.validate_deferred()?;
        while let Some(event) = self.pop()? {
            if let Event::Notification {
                method,
                params: Some(params),
            } = event
            {
                self.state.notification(&method, &params)?;
            }
        }
        Ok(id)
    }

    pub async fn start_turn(&mut self, input: String) -> Result<String, Error> {
        if self.phase() != Phase::ThreadReady {
            return Err(Error::State);
        }
        if input.is_empty() || input.len() > MAX_TEXT_BYTES {
            return Err(Error::Settings);
        }
        let operation = Operation {
            session: self,
            finished: false,
        };
        let result = operation.session.start_turn_inner(input).await;
        operation.finish(result)
    }
    async fn start_turn_inner(&mut self, input: String) -> Result<String, Error> {
        self.state.phase = Phase::StartingTurn;
        let thread = self.state.thread.as_deref().ok_or(Error::State)?;
        let result = self
            .call("turn/start", self.settings.turn_request(thread, input))
            .await?;
        let id = self.state.observe_turn(&result)?;
        self.validate_deferred()?;
        Ok(id)
    }

    pub async fn interrupt(&mut self) -> Result<InterruptDisposition, Error> {
        if self.phase() != Phase::Running || self.state.interrupt_sent {
            return Err(Error::State);
        }
        let operation = Operation {
            session: self,
            finished: false,
        };
        let result = operation.session.interrupt_inner().await;
        operation.finish(result)
    }
    async fn interrupt_inner(&mut self) -> Result<InterruptDisposition, Error> {
        self.state.interrupt_sent = true;
        let scope = TurnScope::new(
            self.state.thread.clone().ok_or(Error::State)?,
            self.state.turn.clone().ok_or(Error::State)?,
        )
        .map_err(|_| Error::Scope)?;
        self.wire
            .send(transport::Command::Interrupt {
                scope: scope.clone(),
                response_timeout_ms: self.response_timeout_ms,
            })
            .await
            .map_err(Error::Transport)?;
        loop {
            match self.receive().await? {
                Event::InterruptAcknowledged { scope: observed } if observed == scope => {
                    return Ok(InterruptDisposition::Acknowledged);
                }
                Event::InterruptRejected {
                    scope: observed,
                    error,
                } if observed == scope => {
                    return Ok(InterruptDisposition::Rejected { code: error.code });
                }
                event @ Event::Notification { .. } => self.buffer(event)?,
                _ => return Err(Error::Scope),
            }
        }
    }

    pub async fn respond_approval(
        &mut self,
        response: crate::codex::approval::ApprovalResponse,
    ) -> Result<(), Error> {
        if self.phase() != Phase::Running || !self.approvals_enabled || self.control.enabled() {
            return Err(Error::State);
        }
        if self.thread_id() != Some(response.request.thread_id())
            || self.turn_id() != Some(response.request.turn_id())
        {
            return Err(Error::Scope);
        }
        let operation = Operation {
            session: self,
            finished: false,
        };
        let result = operation
            .session
            .wire
            .send(transport::Command::RespondApproval { response })
            .await
            .map(|_| ())
            .map_err(Error::Transport);
        operation.finish(result)
    }

    pub async fn next_update(&mut self) -> Result<Update, Error> {
        if self.phase() != Phase::Running {
            return Err(Error::State);
        }
        let operation = Operation {
            session: self,
            finished: false,
        };
        let mut result = operation.session.update_inner().await;
        if result.is_ok()
            && let Err(error) = operation.session.advance_observation()
        {
            result = Err(error);
        }
        operation.finish(result)
    }
    pub async fn next_observed_update(&mut self) -> Result<(Update, super::Observation), Error> {
        let update = self.next_update().await?;
        let observation = super::Observation {
            source: self.bound_source()?,
            sequence: self.observation_sequence,
            kind: self.observation_kind.clone(),
        };
        Ok((update, observation))
    }
    async fn update_inner(&mut self) -> Result<Update, Error> {
        self.wire.ensure_live().map_err(Error::Transport)?;
        let event = match self.pop()? {
            Some(event) => event,
            None => self.receive().await?,
        };
        self.accept_update(event).await
    }
    async fn accept_update(&mut self, event: Event) -> Result<Update, Error> {
        let event = match event {
            Event::ServerRequest {
                id,
                method,
                params: Some(params),
            } if self.approvals_enabled => {
                let request = crate::codex::approval::ApprovalRequest::parse(id, method, params)?;
                if self.thread_id() != Some(request.thread_id())
                    || self.turn_id() != Some(request.turn_id())
                {
                    return Err(Error::Scope);
                }
                // Patch and permission requests can precede item/started in
                // Codex 0.153.4. Bind their exact callback item without inventing
                // an active timeline item or host session identity.
                self.control.admit(&self.wire, request.id())?;
                self.observation_kind = super::ObservationKind::Ignored;
                return Ok(Update::Approval(request));
            }
            event => event,
        };
        let Event::Notification {
            method,
            params: Some(params),
        } = event
        else {
            return Err(Error::Scope);
        };
        let mut update = self.state.notification(&method, &params)?;
        if self.approvals_enabled && method == "serverRequest/resolved" {
            let id =
                serde_json::from_value(params.get("requestId").ok_or(Error::Malformed)?.clone())
                    .map_err(|_| Error::Malformed)?;
            self.control.resolve(&id);
            update = Update::ApprovalResolved { id };
        }
        if self.phase() == Phase::Ended {
            self.drain_terminal().await?;
            self.wire.close();
        }
        self.observation_kind = if method == "thread/tokenUsage/updated" {
            super::ObservationKind::Usage(super::usage::project(&params))
        } else {
            self.observation_evidence
                .project(&update, &params, self.state.outcome.as_ref())
        };
        Ok(update)
    }

    async fn drain_terminal(&mut self) -> Result<(), Error> {
        let deadline = Instant::now()
            .checked_add(Duration::from_millis(self.response_timeout_ms))
            .ok_or(Error::Transport(transport::Error::Timeout))?;
        loop {
            // Finish an already-started suffix frame, but never start a read
            // when the received snapshot is empty. Fragmentation alone must not
            // make a normal completed + idle stream fail. The whole drain has
            // one absolute deadline, in addition to transport deadlines.
            tokio::task::yield_now().await;
            self.wire.ensure_live().map_err(Error::Transport)?;
            if Instant::now() >= deadline {
                return Err(Error::Transport(transport::Error::Timeout));
            }
            let mut event = match self.pop()? {
                Some(event) => Some(event),
                None => self.wire.buffered_event().map_err(Error::Transport)?,
            };
            if event.is_none() && self.wire.has_partial_frame() {
                event = Some(
                    timeout_at(deadline, self.receive())
                        .await
                        .map_err(|_| Error::Transport(transport::Error::Timeout))??,
                );
            }
            match event {
                Some(Event::Notification {
                    method,
                    params: Some(params),
                }) => self.state.terminal_suffix(&method, &params)?,
                Some(Event::ServerRequest { id, .. }) => {
                    self.unsupported(id).await?;
                }
                Some(_) => return Err(Error::Scope),
                None => break,
            }
        }
        if self.wire.pending_server_requests() > 0 {
            return Err(Error::UnsupportedRequest);
        }
        Ok(())
    }

    async fn call(&mut self, method: &str, params: Value) -> Result<Value, Error> {
        let written = self
            .wire
            .send(transport::Command::Request {
                method: method.into(),
                params,
                response_timeout_ms: self.response_timeout_ms,
            })
            .await
            .map_err(Error::Transport)?;
        let expected = written.request_id.ok_or(Error::State)?;
        loop {
            match self.receive().await? {
                Event::Response {
                    id,
                    method: observed,
                    result,
                } if id == expected && observed == method => {
                    return result.map_err(|error| Error::Rejected(error.code));
                }
                event @ Event::Notification { .. } => self.buffer(event)?,
                _ => return Err(Error::Scope),
            }
        }
    }
    async fn receive(&mut self) -> Result<Event, Error> {
        let event = self.wire.next_event().await.map_err(Error::Transport)?;
        if !self.approvals_enabled
            && let Event::ServerRequest { id, .. } = event
        {
            return self.unsupported(id).await;
        }
        Ok(event)
    }
    async fn unsupported(&mut self, id: RequestId) -> Result<Event, Error> {
        self.wire
            .send(transport::Command::RejectServerRequest { id })
            .await
            .map_err(Error::Transport)?;
        Err(Error::UnsupportedRequest)
    }
}
