use crate::{
    Counts, Error, Filter, Kind, MAX_ATTEMPTS, MAX_CALLS, MAX_EVENTS, Verb, acp_tool,
    build_summary, fingerprint, identity,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// Created by the host for one immutable dispatch/connection/turn incarnation.
/// No Deserialize, persistence import, reset or route selection is exposed.
#[derive(Clone, PartialEq, Eq)]
pub struct RunId(String);
impl RunId {
    pub fn new(value: String) -> Result<Self, Error> {
        if identity(&value) {
            Ok(Self(value))
        } else {
            Err(Error::Shape)
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryProof {
    FinalReplyJournalInspection,
    MatrixTimelineInspection,
}
/// Host inspection of the complete run's answer deliveries, not tool output or
/// progress acceptance. Unknown is represented by None on finish.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnswerDelivery {
    count: u32,
    proof: DeliveryProof,
}
impl AnswerDelivery {
    pub fn new(count: u32, proof: DeliveryProof) -> Self {
        Self { count, proof }
    }
    pub fn count(self) -> u32 {
        self.count
    }
    pub fn proof(self) -> DeliveryProof {
        self.proof
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Observation {
    Recorded,
    Duplicate,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttemptOutcome {
    ObservedAccepted,
    ObservedNotAccepted,
    Unknown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingState {
    Attempted,
    Uncertain,
}
#[derive(Clone, PartialEq, Eq)]
pub struct AttemptId {
    run: RunId,
    ordinal: u32,
}
/// Text only; the token is a host custody value and is not serializable content.
pub struct Emission {
    id: AttemptId,
    kind: Kind,
    text: String,
}
impl Emission {
    pub fn id(&self) -> &AttemptId {
        &self.id
    }
    pub fn kind(&self) -> Kind {
        self.kind
    }
    pub fn text(&self) -> &str {
        &self.text
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Active,
    Finished,
    Retired,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum CallState {
    Started,
    Completed,
    Failed,
}
impl CallState {
    // ACP initial tool_call carries its current status too. Missing/unknown or
    // malformed status never supplies completion evidence. Case-insensitive
    // failed retains the existing failure helper's supported string behavior.
    fn terminal(payload: &Value) -> Option<Self> {
        match payload.get("status").and_then(Value::as_str) {
            Some("completed") => Some(Self::Completed),
            Some(status) if status.eq_ignore_ascii_case("failed") => Some(Self::Failed),
            _ => None,
        }
    }
}
struct Call {
    digest: [u8; 32],
    state: CallState,
    report: bool,
}
struct Step {
    sequence: u64,
    verb: Verb,
    call: Option<String>,
}
struct Attempt {
    id: AttemptId,
    kind: Kind,
    through: u64,
    state: PendingState,
}

/// One in-memory policy owner. Dropping it loses projection history; the future
/// host must not recreate the same RunId and infer that old attempts were unsent.
pub struct Accumulator {
    run: RunId,
    filter: Filter,
    phase: Phase,
    receipts: Vec<[u8; 32]>,
    calls: BTreeMap<String, Call>,
    steps: Vec<Step>,
    failures: Vec<u64>,
    now: u64,
    last_attempt: Option<u64>,
    through: u64,
    started: bool,
    start_accepted: bool,
    done_accepted: bool,
    done_attempted: bool,
    delivery: Option<AnswerDelivery>,
    ordinal: u32,
    pending: Option<Attempt>,
    settled: BTreeMap<u32, AttemptOutcome>,
}
impl Accumulator {
    pub fn new(run: RunId, filter: Filter) -> Self {
        Self {
            run,
            filter,
            phase: Phase::Active,
            receipts: vec![],
            calls: BTreeMap::new(),
            steps: vec![],
            failures: vec![],
            now: 0,
            last_attempt: None,
            through: 0,
            started: false,
            start_accepted: false,
            done_accepted: false,
            done_attempted: false,
            delivery: None,
            ordinal: 0,
            pending: None,
            settled: BTreeMap::new(),
        }
    }
    fn scope(&self, run: &RunId) -> Result<(), Error> {
        if run == &self.run {
            Ok(())
        } else {
            Err(Error::Identity)
        }
    }
    fn clock(&self, now: u64) -> Result<(), Error> {
        if now < self.now {
            Err(Error::Clock)
        } else {
            Ok(())
        }
    }
    fn preflight(
        &self,
        run: &RunId,
        sequence: u64,
        now: u64,
        digest: [u8; 32],
    ) -> Result<bool, Error> {
        self.scope(run)?;
        if self.phase == Phase::Retired {
            return Err(Error::State);
        }
        if sequence == 0 {
            return Err(Error::Order);
        }
        if sequence <= self.receipts.len() as u64 {
            return if self.receipts[sequence as usize - 1] == digest {
                Ok(true)
            } else {
                Err(Error::Conflict)
            };
        }
        self.clock(now)?;
        if self.phase != Phase::Active {
            return Err(Error::State);
        }
        if sequence != self.receipts.len() as u64 + 1 {
            return Err(Error::Order);
        }
        if self.receipts.len() == MAX_EVENTS {
            return Err(Error::Capacity);
        }
        Ok(false)
    }
    fn commit(&mut self, now: u64, digest: [u8; 32]) -> Observation {
        self.now = now;
        self.receipts.push(digest);
        Observation::Recorded
    }
    pub fn observe_hook(
        &mut self,
        run: &RunId,
        sequence: u64,
        now: u64,
        payload: &Value,
    ) -> Result<Observation, Error> {
        let digest = digest("hook", payload)?;
        if self.preflight(run, sequence, now, digest)? {
            return Ok(Observation::Duplicate);
        }
        let object = payload.as_object().ok_or(Error::Shape)?;
        let event = object.get("hook_event_name").and_then(Value::as_str);
        let tool = object.get("tool_name").and_then(Value::as_str);
        let decision = self.filter.decide(event, tool);
        match decision.kind {
            Kind::Start => {
                if self.started {
                    return Err(Error::State);
                }
                self.started = true;
            }
            Kind::Done => self.phase = Phase::Finished,
            Kind::Step => {
                if let Some(verb) = decision.verb {
                    self.steps.push(Step {
                        sequence,
                        verb,
                        call: None,
                    });
                }
            }
        }
        Ok(self.commit(now, digest))
    }
    /// Host observed its own runtime start, without manufacturing a hook frame.
    pub fn start(&mut self, run: &RunId, sequence: u64, now: u64) -> Result<Observation, Error> {
        let digest = digest("host_start", &Value::Null)?;
        if self.preflight(run, sequence, now, digest)? {
            return Ok(Observation::Duplicate);
        }
        if self.started {
            return Err(Error::State);
        }
        self.started = true;
        Ok(self.commit(now, digest))
    }
    pub fn observe_acp(
        &mut self,
        run: &RunId,
        sequence: u64,
        now: u64,
        payload: &Value,
    ) -> Result<Observation, Error> {
        let digest = digest("acp", payload)?;
        if self.preflight(run, sequence, now, digest)? {
            return Ok(Observation::Duplicate);
        }
        payload.as_object().ok_or(Error::Shape)?;
        let kind = payload.get("sessionUpdate").and_then(Value::as_str);
        if matches!(kind, Some("tool_call" | "tool_call_update")) {
            let id = payload
                .get("toolCallId")
                .and_then(Value::as_str)
                .filter(|v| identity(v))
                .ok_or(Error::Shape)?;
            self.record_tool(
                id,
                acp_tool(payload),
                kind == Some("tool_call"),
                CallState::terminal(payload),
                sequence,
                digest,
            )?;
        }
        Ok(self.commit(now, digest))
    }
    pub fn observe_tool(
        &mut self,
        run: &RunId,
        sequence: u64,
        now: u64,
        event: crate::ToolEvent<'_>,
    ) -> Result<Observation, Error> {
        if !identity(event.id) || event.tool.is_some_and(|tool| !identity(tool)) {
            return Err(Error::Shape);
        }
        let status = match event.state {
            crate::ToolState::Pending => "pending",
            crate::ToolState::Completed => "completed",
            crate::ToolState::Failed => "failed",
            crate::ToolState::Unknown => "unknown",
        };
        let digest = digest(
            "typed_tool",
            &json!({"id":event.id,"tool":event.tool,"start":event.start,"status":status}),
        )?;
        if self.preflight(run, sequence, now, digest)? {
            return Ok(Observation::Duplicate);
        }
        let terminal = match event.state {
            crate::ToolState::Completed => Some(CallState::Completed),
            crate::ToolState::Failed => Some(CallState::Failed),
            _ => None,
        };
        self.record_tool(
            event.id,
            event.tool,
            event.start,
            terminal,
            sequence,
            digest,
        )?;
        Ok(self.commit(now, digest))
    }
    fn record_tool(
        &mut self,
        id: &str,
        tool: Option<&str>,
        start: bool,
        terminal: Option<CallState>,
        sequence: u64,
        digest: [u8; 32],
    ) -> Result<(), Error> {
        if start {
            if let Some(call) = self.calls.get(id) {
                if call.digest != digest {
                    return Err(Error::Conflict);
                }
            } else {
                if self.calls.len() == MAX_CALLS {
                    return Err(Error::Capacity);
                }
                let decision = self.filter.decide(Some("PostToolUse"), tool);
                let state = terminal.unwrap_or(CallState::Started);
                if let Some(verb) = decision.verb {
                    self.steps.push(Step {
                        sequence,
                        verb,
                        call: Some(id.into()),
                    });
                }
                if state == CallState::Failed && decision.report {
                    self.failures.push(sequence);
                }
                self.calls.insert(
                    id.into(),
                    Call {
                        digest,
                        state,
                        report: decision.report,
                    },
                );
            }
        } else {
            let call = self.calls.get_mut(id).ok_or(Error::Order)?;
            if let Some(next) = terminal {
                if call.state != CallState::Started && call.state != next {
                    return Err(Error::Conflict);
                }
                if call.state == CallState::Started && next == CallState::Failed && call.report {
                    self.failures.push(sequence);
                }
                if call.state == CallState::Started && next == CallState::Completed {
                    // A previously accepted pending notice is not a receipt
                    // for this new completion observation. Keep one lifetime
                    // contribution, now eligible in the current window.
                    if let Some(step) = self
                        .steps
                        .iter_mut()
                        .find(|s| s.call.as_deref() == Some(id))
                    {
                        step.sequence = sequence;
                    }
                }
                call.state = next;
            }
        }
        Ok(())
    }
    pub fn finish(
        &mut self,
        run: &RunId,
        sequence: u64,
        now: u64,
        delivery: Option<AnswerDelivery>,
    ) -> Result<Observation, Error> {
        let payload = json!({"count":delivery.map(|d| d.count), "proof":delivery.map(|d| match d.proof { DeliveryProof::FinalReplyJournalInspection => "final_reply_journal_inspection", DeliveryProof::MatrixTimelineInspection => "matrix_timeline_inspection" })});
        let digest = digest("host_finish", &payload)?;
        if self.preflight(run, sequence, now, digest)? {
            return Ok(Observation::Duplicate);
        }
        self.phase = Phase::Finished;
        self.delivery = delivery;
        Ok(self.commit(now, digest))
    }
    fn counts_since(&self, sequence: u64) -> Result<(Counts, u32, u32), Error> {
        let mut counts = Counts::default();
        let mut unresolved = 0;
        for step in self.steps.iter().filter(|s| s.sequence > sequence) {
            if let Some(id) = &step.call {
                match self.calls.get(id).map(|c| c.state) {
                    Some(CallState::Completed) => {}
                    Some(CallState::Failed) => continue,
                    _ => {
                        unresolved += 1;
                        continue;
                    }
                }
            }
            counts.add(step.verb, 1)?;
        }
        Ok((
            counts,
            self.failures.iter().filter(|seq| **seq > sequence).count() as u32,
            unresolved,
        ))
    }
    pub fn summary(&self) -> Result<Option<String>, Error> {
        if self.phase == Phase::Retired {
            return Err(Error::State);
        }
        let (counts, failures, unresolved) = self.counts_since(0)?;
        Ok(project_summary(
            if self.phase == Phase::Active {
                Kind::Step
            } else {
                Kind::Done
            },
            &counts,
            failures,
            unresolved,
            self.delivery.map(|d| d.count),
        ))
    }
    pub fn answer_delivery(&self) -> Option<AnswerDelivery> {
        self.delivery
    }
    /// Host scheduling observation without a tool, receipt, attempt or delivery.
    /// A quiet runtime event still fences subsequent backdated submissions.
    pub fn advance_clock(&mut self, run: &RunId, now: u64) -> Result<(), Error> {
        self.scope(run)?;
        self.clock(now)?;
        if self.phase == Phase::Retired {
            return Err(Error::State);
        }
        self.now = now;
        Ok(())
    }
    /// Recovery token only; this does not return text or grant another send.
    pub fn pending_id(&self) -> Option<&AttemptId> {
        self.pending.as_ref().map(|p| &p.id)
    }
    pub fn pending_state(&self) -> Option<PendingState> {
        self.pending.as_ref().map(|p| p.state)
    }
    fn quiet(&mut self, now: u64) -> Result<Option<Emission>, Error> {
        self.now = now;
        Ok(None)
    }
    pub fn claim(&mut self, run: &RunId, now: u64) -> Result<Option<Emission>, Error> {
        self.scope(run)?;
        self.clock(now)?;
        if self.phase == Phase::Retired {
            return Err(Error::State);
        }
        if self.pending.is_some() || self.done_accepted {
            return self.quiet(now);
        }
        let kind = if self.phase == Phase::Finished {
            Kind::Done
        } else if self.started && !self.start_accepted {
            Kind::Start
        } else {
            Kind::Step
        };
        let event = match kind {
            Kind::Start => "start",
            Kind::Step => "PostToolUse",
            Kind::Done => "done",
        };
        if kind != Kind::Step && !self.filter.decide(Some(event), None).report {
            if kind == Kind::Start {
                self.start_accepted = true;
                return self.claim(run, now);
            }
            return self.quiet(now);
        }
        let fresh_transition =
            self.last_attempt.is_none() || (kind == Kind::Done && !self.done_attempted);
        if !fresh_transition
            && self
                .last_attempt
                .is_some_and(|last| ((now - last) as f64) < self.filter.min_interval_ms())
        {
            return self.quiet(now);
        }
        let (counts, failures, unresolved) =
            self.counts_since(if kind == Kind::Done { 0 } else { self.through })?;
        let Some(summary) = project_summary(
            kind,
            &counts,
            failures,
            unresolved,
            self.delivery.map(|d| d.count),
        ) else {
            return self.quiet(now);
        };
        if self.ordinal == MAX_ATTEMPTS {
            return Err(Error::Capacity);
        }
        self.ordinal += 1;
        let id = AttemptId {
            run: self.run.clone(),
            ordinal: self.ordinal,
        };
        self.pending = Some(Attempt {
            id: id.clone(),
            kind,
            through: if kind == Kind::Start {
                0
            } else {
                self.receipts.len() as u64
            },
            state: PendingState::Attempted,
        });
        self.last_attempt = Some(now);
        self.now = now;
        if kind == Kind::Done {
            self.done_attempted = true;
        }
        Ok(Some(Emission {
            id,
            kind,
            text: format!("⏳ {summary}"),
        }))
    }
    pub fn settle(
        &mut self,
        run: &RunId,
        id: &AttemptId,
        now: u64,
        outcome: AttemptOutcome,
    ) -> Result<Observation, Error> {
        self.scope(run)?;
        if id.run != self.run {
            return Err(Error::Identity);
        }
        if let Some(previous) = self.settled.get(&id.ordinal) {
            return if *previous == outcome {
                Ok(Observation::Duplicate)
            } else {
                Err(Error::Conflict)
            };
        }
        self.clock(now)?;
        let pending = self
            .pending
            .as_mut()
            .filter(|p| p.id == *id)
            .ok_or(Error::Identity)?;
        if outcome == AttemptOutcome::Unknown {
            pending.state = PendingState::Uncertain;
            self.now = now;
            return Ok(Observation::Recorded);
        }
        if outcome == AttemptOutcome::ObservedAccepted {
            self.through = self.through.max(pending.through);
            if pending.kind == Kind::Start {
                self.start_accepted = true;
            }
            if pending.kind == Kind::Done {
                self.done_accepted = true;
            }
        }
        self.settled.insert(id.ordinal, outcome);
        self.pending = None;
        self.now = now;
        Ok(Observation::Recorded)
    }
    pub fn retire(&mut self, run: &RunId, now: u64) -> Result<(), Error> {
        self.scope(run)?;
        self.clock(now)?;
        self.phase = Phase::Retired;
        self.now = now;
        if let Some(pending) = self.pending.as_mut() {
            pending.state = PendingState::Uncertain;
        }
        Ok(())
    }
}
fn project_summary(
    kind: Kind,
    counts: &Counts,
    failures: u32,
    unresolved: u32,
    delivered: Option<u32>,
) -> Option<String> {
    if unresolved == 0 || kind == Kind::Start {
        return build_summary(kind, counts, failures, delivered);
    }
    let mut parts = Vec::new();
    if let Some(confirmed) = build_summary(Kind::Step, counts, failures, None) {
        parts.push(confirmed);
    }
    parts.push(format!(
        "{unresolved} attempt{} {}",
        if unresolved == 1 { "" } else { "s" },
        if kind == Kind::Done {
            "unresolved"
        } else {
            "pending"
        }
    ));
    Some(format!(
        "{}{}{}",
        if kind == Kind::Done {
            "finished — "
        } else {
            ""
        },
        parts.join(", "),
        if kind == Kind::Done && delivered == Some(0) {
            ", but sent nothing"
        } else {
            ""
        }
    ))
}
fn digest(kind: &str, value: &Value) -> Result<[u8; 32], Error> {
    let hash = fingerprint(value, 16_384)?;
    let mut digest = Sha256::new();
    digest.update(kind.as_bytes());
    digest.update(hash);
    Ok(digest.finalize().into())
}
