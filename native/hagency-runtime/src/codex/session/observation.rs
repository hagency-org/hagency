//! Redacted observations minted only after the real session state accepted an
//! event. Textual peer IDs alone cannot establish a source connection.
use super::{ItemPhase, Outcome, Update};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[derive(Clone)]
pub struct ObservationSource {
    pub(super) live: Arc<AtomicBool>,
    pub(super) thread: String,
    pub(super) turn: String,
}
impl PartialEq for ObservationSource {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.live, &other.live)
            && self.thread == other.thread
            && self.turn == other.turn
    }
}
impl Eq for ObservationSource {}
impl ObservationSource {
    pub fn is_retired(&self) -> bool {
        !self.live.load(Ordering::Acquire)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ToolKind {
    Command,
    FileChange,
    Unsupported,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ToolResult {
    Pending,
    Completed,
    Failed,
    Unconfirmed,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TurnResult {
    Completed,
    Failed,
    Interrupted,
    Unknown,
}

#[derive(Clone, PartialEq, Eq)]
pub struct ToolEvidence {
    id: String,
    kind: ToolKind,
    phase: ItemPhase,
    result: ToolResult,
}
impl ToolEvidence {
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn kind(&self) -> ToolKind {
        self.kind
    }
    pub fn phase(&self) -> ItemPhase {
        self.phase
    }
    pub fn result(&self) -> ToolResult {
        self.result
    }
}
#[derive(Clone, PartialEq, Eq)]
pub enum ObservationKind {
    Ignored,
    Invalidated,
    Usage(super::UsageEvidence),
    Tool(ToolEvidence),
    TurnEnded(TurnResult),
}

#[derive(Default)]
pub(super) struct EvidenceTracker(BTreeMap<String, ToolEvidence>);
impl EvidenceTracker {
    pub(super) fn project(
        &mut self,
        update: &Update,
        params: &Value,
        outcome: Option<&Outcome>,
    ) -> ObservationKind {
        let observation = project(update, params, outcome);
        if let ObservationKind::Tool(tool) = &observation {
            // The session's item bound and exact item lifecycle precede this.
            self.0.insert(tool.id.clone(), tool.clone());
        }
        if matches!(observation, ObservationKind::TurnEnded(_)) {
            for item in params["turn"]["items"].as_array().into_iter().flatten() {
                let Some(previous) = item["id"].as_str().and_then(|id| self.0.get(id)) else {
                    continue;
                };
                if previous.kind != ToolKind::Unsupported
                    && (item.get("status").is_some() || item.get("exitCode").is_some())
                    && evidence(previous.kind, ItemPhase::Complete, item) != previous.result
                {
                    return ObservationKind::Invalidated;
                }
            }
        }
        observation
    }
}

/// No Deserialize, public constructor, Debug or automatic content projection.
/// Clone permits exact local replay; private fields cannot be rebound or edited.
#[derive(Clone, PartialEq, Eq)]
pub struct Observation {
    pub(super) source: ObservationSource,
    pub(super) sequence: u64,
    pub(super) kind: ObservationKind,
}
impl Observation {
    pub fn source(&self) -> &ObservationSource {
        &self.source
    }
    pub fn sequence(&self) -> u64 {
        self.sequence
    }
    pub fn kind(&self) -> &ObservationKind {
        &self.kind
    }
}

pub(super) fn project(
    update: &Update,
    params: &Value,
    outcome: Option<&Outcome>,
) -> ObservationKind {
    match update {
        Update::Item { id, kind, phase } => {
            let category = match kind.as_str() {
                "commandExecution" => ToolKind::Command,
                "fileChange" => ToolKind::FileChange,
                "mcpToolCall"
                | "dynamicToolCall"
                | "collabAgentToolCall"
                | "webSearch"
                | "imageView"
                | "imageGeneration"
                | "sleep" => ToolKind::Unsupported,
                _ => return ObservationKind::Ignored,
            };
            let result = evidence(category, *phase, &params["item"]);
            ObservationKind::Tool(ToolEvidence {
                id: id.clone(),
                kind: category,
                phase: *phase,
                result,
            })
        }
        Update::TurnEnded => ObservationKind::TurnEnded(match outcome {
            Some(Outcome::Completed { .. }) => TurnResult::Completed,
            Some(Outcome::Failed) => TurnResult::Failed,
            Some(Outcome::Interrupted) => TurnResult::Interrupted,
            _ => TurnResult::Unknown,
        }),
        _ => ObservationKind::Ignored,
    }
}

fn evidence(kind: ToolKind, phase: ItemPhase, item: &Value) -> ToolResult {
    let status = item.get("status").and_then(Value::as_str);
    if kind == ToolKind::Unsupported {
        return ToolResult::Unconfirmed;
    }
    if phase == ItemPhase::Active {
        return if status == Some("inProgress") {
            ToolResult::Pending
        } else {
            ToolResult::Unconfirmed
        };
    }
    if kind == ToolKind::Command {
        let exit = match item.get("exitCode") {
            None | Some(Value::Null) => None,
            Some(value) => match value.as_i64().and_then(|n| i32::try_from(n).ok()) {
                Some(n) => Some(n),
                None => return ToolResult::Unconfirmed,
            },
        };
        return match (status, exit) {
            (Some("completed"), Some(0)) => ToolResult::Completed,
            (Some("failed" | "declined"), _) => ToolResult::Failed,
            // An inconsistent status/exit pair is not trustworthy success.
            _ => ToolResult::Unconfirmed,
        };
    }
    match status {
        Some("completed") => ToolResult::Completed,
        Some("failed" | "declined") => ToolResult::Failed,
        _ => ToolResult::Unconfirmed,
    }
}
