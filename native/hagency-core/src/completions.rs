//! Explicit task completion content is data, never runtime or send authority.
use crate::{InvalidInput, project::identifier, tasks::text};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompleteTaskWithReply {
    pub id: String,
    pub call_id: String,
    pub body: String,
}
impl CompleteTaskWithReply {
    pub fn validate(&self) -> Result<(), InvalidInput> {
        identifier(&self.id, 128)?;
        identifier(&self.call_id, 512)?;
        text(&self.body, 32 * 1024)
    }
}
impl std::fmt::Debug for CompleteTaskWithReply {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompleteTaskWithReply")
            .field("id", &self.id)
            .field("call_id", &self.call_id)
            .field("body_bytes", &self.body.len())
            .finish()
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompletionState {
    Held,
    Ready,
    Cancelled,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct CompletionReceipt {
    pub id: String,
    pub task_id: String,
    pub execution_epoch: u64,
    pub state: CompletionState,
    pub replayed: bool,
}
