//! Host-selected recurring Matrix inbox routing. These values identify an
//! already verified session and workspace; they convey no Matrix or runner
//! authority by themselves.
use crate::{InvalidInput, project::identifier};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AgentInboxPlan {
    pub session_id: String,
    pub workspace_id: String,
}
impl AgentInboxPlan {
    pub fn validate(&self) -> Result<(), InvalidInput> {
        identifier(&self.session_id, 128)?;
        identifier(&self.workspace_id, 128)?;
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentInboxSelection {
    NoWake,
    Selected {
        dispatch_id: String,
        task_id: String,
        count: usize,
        replayed: bool,
    },
}
