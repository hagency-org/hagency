//! Task definitions are data. Host intent/receipt construction and current runner
//! capabilities determine authority; runtime JSON cannot assert transport delivery.
use crate::{InvalidInput, project::identifier, tasks::text};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    P0,
    P1,
    #[default]
    P2,
    P3,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Granularity {
    Epic,
    #[default]
    Task,
    Subtask,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskDefinition {
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub priority: Priority,
    #[serde(default)]
    pub granularity: Granularity,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub parent_id: Option<String>,
}
impl TaskDefinition {
    pub fn validate(&self) -> Result<(), InvalidInput> {
        text(&self.title, 255)?;
        if self.description.len() > 4096 || self.description.contains('\0') {
            return Err(InvalidInput("invalid task description"));
        }
        if self.labels.len() > 32 {
            return Err(InvalidInput("too many task labels"));
        }
        let mut seen = std::collections::BTreeSet::new();
        for label in &self.labels {
            text(label, 64)?;
            if !seen.insert(label) {
                return Err(InvalidInput("duplicate task label"));
            }
        }
        if let Some(id) = &self.parent_id {
            identifier(id, 128)?;
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize)]
pub struct TaskIntent {
    pub request_scope: String,
    pub request_key: String,
    pub assignee_engagement: String,
    pub root_sequence: u64,
    pub input_sequences: Vec<u64>,
    pub definition: TaskDefinition,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Delegation {
    pub call_id: String,
    pub assignee_engagement: String,
    #[serde(default)]
    pub root_sequence: Option<u64>,
    #[serde(default)]
    pub input_sequences: Vec<u64>,
    pub definition: TaskDefinition,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentResult {
    pub task_id: String,
    pub session_id: String,
    pub command_id: String,
    pub transaction_id: String,
    pub activation: String,
    pub replayed: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskNotice {
    pub id: String,
    pub task_id: String,
    pub session_id: String,
    pub sender_engagement: String,
    pub server_name: String,
    pub room_id: String,
    pub thread_root: Option<String>,
    pub transaction_id: String,
    pub body: String,
    pub kind: String,
}
#[derive(Clone, Serialize)]
pub struct NoticeClaim {
    pub notice: TaskNotice,
    pub token: String,
    pub deadline: u64,
}
impl std::fmt::Debug for NoticeClaim {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NoticeClaim")
            .field("id", &self.notice.id)
            .field("deadline", &self.deadline)
            .finish_non_exhaustive()
    }
}
#[derive(Debug, Clone, Serialize)]
pub struct NoticeDelivery {
    pub server_name: String,
    pub room_id: String,
    pub transaction_id: String,
    pub event_id: String,
}
impl NoticeDelivery {
    pub fn validate(&self) -> Result<(), InvalidInput> {
        ruma_common::ServerName::parse(&self.server_name)
            .map_err(|_| InvalidInput("invalid delivery server"))?;
        ruma_common::RoomId::parse(&self.room_id)
            .map_err(|_| InvalidInput("invalid delivery room"))?;
        ruma_common::EventId::parse(&self.event_id)
            .map_err(|_| InvalidInput("invalid delivery event"))?;
        identifier(&self.transaction_id, 128)
    }
}
