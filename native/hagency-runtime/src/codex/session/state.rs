use super::{
    Error, ItemPhase, MAX_EVENTS, MAX_ITEMS, MAX_TEXT_BYTES, Outcome, Phase, Settings, Update, id,
    object, string,
};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

struct Item {
    kind: String,
    phase: ItemPhase,
    text: String,
    message_phase: Option<String>,
}
pub(super) struct State {
    pub phase: Phase,
    pub thread: Option<String>,
    pub turn: Option<String>,
    pub outcome: Option<Outcome>,
    pub interrupt_sent: bool,
    items: BTreeMap<String, Item>,
    final_ids: Vec<String>,
    text_bytes: usize,
    events: usize,
}
impl Default for State {
    fn default() -> Self {
        Self {
            phase: Phase::New,
            thread: None,
            turn: None,
            outcome: None,
            interrupt_sent: false,
            items: BTreeMap::new(),
            final_ids: Vec::new(),
            text_bytes: 0,
            events: 0,
        }
    }
}
impl State {
    pub fn item_count(&self) -> usize {
        self.items.len()
    }
    pub fn text_bytes(&self) -> usize {
        self.text_bytes
    }
    pub fn event_count(&self) -> usize {
        self.events
    }
    pub fn failed(&mut self, error: Error) {
        self.phase = Phase::Ended;
        self.outcome = Some(match error {
            Error::UnsupportedRequest => Outcome::UnsupportedRequest,
            Error::Rejected(_) => Outcome::Failed,
            _ => Outcome::Unknown { reason: error },
        });
    }
    pub fn observe_thread(
        &mut self,
        result: &Value,
        settings: &Settings,
        expected: Option<&str>,
    ) -> Result<String, Error> {
        let thread = object(result, "thread")?;
        let observed = id(thread, "id")?;
        if expected.is_some_and(|id| id != observed) {
            return Err(Error::Scope);
        }
        if string(result, "cwd")? != settings.cwd
            || string(thread, "cwd")? != settings.cwd
            || string(result, "model")? != settings.model
        {
            return Err(Error::Scope);
        }
        if string(result, "approvalPolicy")? != "on-request"
            || string(result, "approvalsReviewer")? != "user"
        {
            return Err(Error::Policy);
        }
        let sandbox = object(result, "sandbox")?;
        let kind = if settings.read_only {
            "readOnly"
        } else {
            "workspaceWrite"
        };
        if string(sandbox, "type")? != kind
            || sandbox.get("networkAccess").is_some_and(|v| v != false)
        {
            return Err(Error::Policy);
        }
        if let Some(roots) = sandbox.get("writableRoots") {
            let roots = roots.as_array().ok_or(Error::Policy)?;
            if (settings.read_only && !roots.is_empty())
                || roots
                    .iter()
                    .any(|root| root.as_str() != Some(settings.cwd.as_str()))
            {
                return Err(Error::Policy);
            }
        }
        if string(object(thread, "status")?, "type")? != "idle" {
            return Err(Error::State);
        }
        if !thread
            .get("turns")
            .and_then(Value::as_array)
            .ok_or(Error::Malformed)?
            .is_empty()
        {
            return Err(Error::Scope);
        }
        self.thread = Some(observed.into());
        self.phase = Phase::ThreadReady;
        Ok(observed.into())
    }
    pub fn observe_turn(&mut self, result: &Value) -> Result<String, Error> {
        let turn = object(result, "turn")?;
        let observed = id(turn, "id")?;
        if !matches!(
            string(turn, "status")?,
            "inProgress" | "completed" | "failed" | "interrupted"
        ) {
            return Err(Error::Malformed);
        }
        if !turn
            .get("items")
            .and_then(Value::as_array)
            .ok_or(Error::Malformed)?
            .is_empty()
        {
            return Err(Error::Malformed);
        }
        // Even a terminal status in the start response is not a completion event.
        self.turn = Some(observed.into());
        self.phase = Phase::Running;
        Ok(observed.into())
    }

    pub fn notification(&mut self, method: &str, params: &Value) -> Result<Update, Error> {
        if self.phase == Phase::Ended {
            return Err(Error::Scope);
        }
        self.events = self
            .events
            .checked_add(1)
            .filter(|&n| n <= MAX_EVENTS)
            .ok_or(Error::Capacity)?;
        scope(method, params, self.thread.as_deref(), self.turn.as_deref())?;
        match method {
            "warning" | "configWarning" => Ok(Update::Notice),
            "thread/started" => Ok(Update::ThreadStatus),
            "thread/status/changed" => {
                if !matches!(
                    string(object(params, "status")?, "type")?,
                    "idle" | "active"
                ) {
                    return Err(Error::UnsupportedEvent);
                }
                Ok(Update::ThreadStatus)
            }
            "serverRequest/resolved" => Ok(Update::Progress),
            "turn/started" => {
                if self.phase != Phase::Running
                    || string(object(params, "turn")?, "status")? != "inProgress"
                {
                    return Err(Error::State);
                }
                Ok(Update::TurnStarted)
            }
            "turn/completed" => self.complete(object(params, "turn")?),
            "item/started" => self.item(params, false),
            "item/completed" => self.item(params, true),
            "item/agentMessage/delta" => self.delta(params),
            "error" => {
                if self.phase != Phase::Running {
                    return Err(Error::State);
                }
                let message = string(object(params, "error")?, "message")?;
                if message.len() > 4096 {
                    return Err(Error::Capacity);
                }
                match params.get("willRetry").and_then(Value::as_bool) {
                    Some(true) => Ok(Update::Retrying),
                    Some(false) => {
                        self.phase = Phase::Ended;
                        self.outcome = Some(Outcome::Failed);
                        Ok(Update::TurnEnded)
                    }
                    None => Err(Error::Malformed),
                }
            }
            "turn/diff/updated"
            | "turn/plan/updated"
            | "thread/tokenUsage/updated"
            | "thread/compacted"
            | "turn/moderationMetadata" => {
                if self.phase != Phase::Running {
                    return Err(Error::State);
                }
                Ok(Update::Progress)
            }
            "item/plan/delta"
            | "item/commandExecution/outputDelta"
            | "item/fileChange/outputDelta"
            | "item/mcpToolCall/progress"
            | "item/reasoning/summaryTextDelta"
            | "item/reasoning/summaryPartAdded"
            | "item/reasoning/textDelta" => {
                let item_id = id(params, "itemId")?;
                let item = self.items.get(item_id).ok_or(Error::Scope)?;
                if item.phase != ItemPhase::Active {
                    return Err(Error::Scope);
                }
                let kind = method.split('/').nth(1).ok_or(Error::Malformed)?;
                if item.kind != kind {
                    return Err(Error::Scope);
                }
                Ok(Update::Progress)
            }
            _ => Err(Error::UnsupportedEvent),
        }
    }

    pub fn terminal_suffix(&mut self, method: &str, params: &Value) -> Result<(), Error> {
        self.events = self
            .events
            .checked_add(1)
            .filter(|&n| n <= MAX_EVENTS)
            .ok_or(Error::Capacity)?;
        scope(method, params, self.thread.as_deref(), self.turn.as_deref())?;
        match method {
            "warning" | "configWarning" | "serverRequest/resolved" => Ok(()),
            "thread/status/changed" if string(object(params, "status")?, "type")? == "idle" => {
                Ok(())
            }
            _ => Err(Error::Scope),
        }
    }

    fn item(&mut self, params: &Value, complete: bool) -> Result<Update, Error> {
        if self.phase != Phase::Running {
            return Err(Error::State);
        }
        let timestamp = if complete {
            "completedAtMs"
        } else {
            "startedAtMs"
        };
        if params
            .get(timestamp)
            .and_then(Value::as_i64)
            .is_none_or(|n| n < 0)
        {
            return Err(Error::Malformed);
        }
        let value = object(params, "item")?;
        let item_id = id(value, "id")?;
        let kind = string(value, "type")?;
        if !matches!(
            kind,
            "userMessage"
                | "agentMessage"
                | "plan"
                | "reasoning"
                | "commandExecution"
                | "fileChange"
                | "mcpToolCall"
                | "dynamicToolCall"
                | "collabAgentToolCall"
                | "webSearch"
                | "imageView"
                | "imageGeneration"
                | "enteredReviewMode"
                | "exitedReviewMode"
                | "contextCompaction"
                | "sleep"
        ) {
            return Err(Error::UnsupportedEvent);
        }
        if !complete && (self.items.contains_key(item_id) || self.items.len() >= MAX_ITEMS) {
            return Err(if self.items.contains_key(item_id) {
                Error::Scope
            } else {
                Error::Capacity
            });
        }
        let existing = self.items.get(item_id);
        if complete
            && existing.is_none_or(|item| item.phase != ItemPhase::Active || item.kind != kind)
        {
            return Err(Error::Scope);
        }
        let (text, message_phase) = if kind == "agentMessage" {
            let text = string(value, "text")?.to_owned();
            let phase = match value.get("phase") {
                None | Some(Value::Null) => None,
                Some(Value::String(phase))
                    if matches!(phase.as_str(), "commentary" | "final_answer") =>
                {
                    Some(phase.clone())
                }
                _ => return Err(Error::Malformed),
            };
            if let Some(previous) = existing {
                if previous.message_phase.is_some() && previous.message_phase != phase {
                    return Err(Error::Scope);
                }
                if !text.starts_with(&previous.text) {
                    return Err(Error::Malformed);
                }
            }
            (text, phase)
        } else {
            (String::new(), None)
        };
        let old = existing.map_or(0, |item| item.text.len());
        self.text_bytes = self
            .text_bytes
            .checked_sub(old)
            .and_then(|n| n.checked_add(text.len()))
            .filter(|&n| n <= MAX_TEXT_BYTES)
            .ok_or(Error::Capacity)?;
        if complete && kind == "agentMessage" && message_phase.as_deref() != Some("commentary") {
            self.final_ids.push(item_id.into());
        }
        let phase = if complete {
            ItemPhase::Complete
        } else {
            ItemPhase::Active
        };
        self.items.insert(
            item_id.into(),
            Item {
                kind: kind.into(),
                phase,
                text,
                message_phase,
            },
        );
        Ok(Update::Item {
            id: item_id.into(),
            kind: kind.into(),
            phase,
        })
    }

    fn delta(&mut self, params: &Value) -> Result<Update, Error> {
        let item_id = id(params, "itemId")?;
        let delta = string(params, "delta")?;
        let item = self.items.get_mut(item_id).ok_or(Error::Scope)?;
        if self.phase != Phase::Running
            || item.kind != "agentMessage"
            || item.phase != ItemPhase::Active
        {
            return Err(Error::Scope);
        }
        let total = self
            .text_bytes
            .checked_add(delta.len())
            .filter(|&n| n <= MAX_TEXT_BYTES)
            .ok_or(Error::Capacity)?;
        item.text.push_str(delta);
        self.text_bytes = total;
        Ok(Update::TextDelta {
            item_id: item_id.into(),
            delta: delta.into(),
        })
    }

    fn complete(&mut self, turn: &Value) -> Result<Update, Error> {
        if self.phase != Phase::Running {
            return Err(Error::State);
        }
        let items = turn
            .get("items")
            .and_then(Value::as_array)
            .ok_or(Error::Malformed)?;
        if items.len() > MAX_ITEMS {
            return Err(Error::Capacity);
        }
        let mut seen = BTreeSet::new();
        for value in items {
            let observed = id(value, "id")?;
            if !seen.insert(observed) {
                return Err(Error::Scope);
            }
            let item = self.items.get(observed).ok_or(Error::Scope)?;
            if string(value, "type")? != item.kind {
                return Err(Error::Scope);
            }
            if item.kind == "agentMessage" && string(value, "text")? != item.text {
                return Err(Error::Malformed);
            }
        }
        self.outcome = Some(match string(turn, "status")? {
            "completed" => {
                if turn.get("error").is_some_and(|v| !v.is_null()) {
                    return Err(Error::Malformed);
                }
                if self
                    .items
                    .values()
                    .any(|item| item.phase == ItemPhase::Active)
                {
                    return Err(Error::State);
                }
                let mut text = String::new();
                for id in &self.final_ids {
                    let item = self.items.get(id).ok_or(Error::Scope)?;
                    if !text.is_empty() {
                        text.push_str("\n\n");
                    }
                    text.push_str(&item.text);
                    if text.len() > MAX_TEXT_BYTES {
                        return Err(Error::Capacity);
                    }
                }
                Outcome::Completed { text }
            }
            "failed" => {
                if string(object(turn, "error")?, "message")?.len() > 4096 {
                    return Err(Error::Capacity);
                }
                Outcome::Failed
            }
            "interrupted" => Outcome::Interrupted,
            _ => return Err(Error::Malformed),
        });
        self.phase = Phase::Ended;
        Ok(Update::TurnEnded)
    }
}

/// Optional expected identities are absent only during the corresponding RPC;
/// no notification is acted upon until both required identities are known.
pub(super) fn scope(
    method: &str,
    params: &Value,
    thread: Option<&str>,
    turn: Option<&str>,
) -> Result<(), Error> {
    if !params.is_object() {
        return Err(Error::Malformed);
    }
    if matches!(method, "warning" | "configWarning") {
        for (key, expected) in [("threadId", thread), ("turnId", turn)] {
            if let Some(value) = params.get(key).filter(|v| !v.is_null()) {
                let observed = value.as_str().ok_or(Error::Malformed)?;
                if expected != Some(observed) {
                    return Err(Error::Scope);
                }
            }
        }
        return Ok(());
    }
    let observed_thread = if method == "thread/started" {
        id(object(params, "thread")?, "id")?
    } else {
        id(params, "threadId")?
    };
    if thread.is_some_and(|expected| expected != observed_thread) {
        return Err(Error::Scope);
    }
    if matches!(
        method,
        "thread/started" | "thread/status/changed" | "serverRequest/resolved"
    ) {
        return Ok(());
    }
    let observed_turn = if matches!(method, "turn/started" | "turn/completed") {
        id(object(params, "turn")?, "id")?
    } else {
        id(params, "turnId")?
    };
    if turn.is_some_and(|expected| expected != observed_turn) {
        return Err(Error::Scope);
    }
    Ok(())
}
