//! Runner graph commands contain definitions and data, never host identity.
use crate::{InvalidInput, canonical, graphs::*, project::identifier};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowRequest {
    pub call_id: String,
    pub conversation_id: String,
    pub definition: GraphDefinition,
}
impl WorkflowRequest {
    pub fn validate(&self) -> Result<(), InvalidInput> {
        identifier(&self.call_id, 512)?;
        identifier(&self.conversation_id, 128)?;
        self.definition.validate()?;
        for node in &self.definition.nodes {
            identifier(&node.assignee, 128)?;
        }
        if canonical::encode_payload(
            &serde_json::to_value(self).map_err(|_| InvalidInput("invalid graph request"))?,
        )?
        .len()
            > 64 * 1024
        {
            return Err(InvalidInput("graph request exceeds 64 KiB"));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkflowOutcome {
    Complete { result: Value },
    Failed { error: String },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowResultRequest {
    pub call_id: String,
    pub node_id: String,
    pub outcome: WorkflowOutcome,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowCancel {
    pub call_id: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DependencyRequest {
    pub node_id: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowNode {
    pub task_id: String,
    pub session_id: String,
    pub message_sequence: Option<u64>,
    pub completed_epoch: Option<u64>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workflow {
    pub id: String,
    pub conversation_id: String,
    pub creator_session_id: String,
    pub parent_task_id: Option<String>,
    pub created_at: u64,
    pub graph: Graph,
    pub nodes: BTreeMap<String, WorkflowNode>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowReceipt {
    pub workflow: WorkflowView,
    pub replayed: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowNodeView {
    pub node_id: String,
    pub binding: WorkflowNode,
    pub state: NodeStatus,
    pub error: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowView {
    pub id: String,
    pub conversation_id: String,
    pub creator_session_id: String,
    pub parent_task_id: Option<String>,
    pub created_at: u64,
    pub definition: GraphDefinition,
    pub state: GraphStatus,
    pub nodes: Vec<WorkflowNodeView>,
}
impl Workflow {
    pub fn view(&self) -> WorkflowView {
        WorkflowView {
            id: self.id.clone(),
            conversation_id: self.conversation_id.clone(),
            creator_session_id: self.creator_session_id.clone(),
            parent_task_id: self.parent_task_id.clone(),
            created_at: self.created_at,
            definition: self.graph.definition.clone(),
            state: self.graph.status,
            nodes: self
                .graph
                .definition
                .nodes
                .iter()
                .map(|n| WorkflowNodeView {
                    node_id: n.id.clone(),
                    binding: self.nodes[&n.id].clone(),
                    state: self.graph.progress[&n.id].status,
                    error: self.graph.progress[&n.id].error.clone(),
                })
                .collect(),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowSummary {
    pub id: String,
    pub conversation_id: String,
    pub label: String,
    pub state: GraphStatus,
    pub created_at: u64,
    pub node_count: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowResultReceipt {
    pub graph_id: String,
    pub node_id: String,
    pub task_id: String,
    pub state: NodeStatus,
    pub graph_state: GraphStatus,
    pub execution_epoch: u64,
    pub replayed: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyRef {
    pub sequence: u64,
    pub node_id: String,
    pub task_id: String,
    pub execution_epoch: u64,
    pub digest: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyValue {
    pub dependency: DependencyRef,
    pub result: Value,
}
