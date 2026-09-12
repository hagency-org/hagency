//! Canonical tasks and private runner protocol. Host construction is not ingress authorization.
use crate::{InvalidInput, JSON_SAFE_MAX, project::identifier};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    Created,
    Accepted,
    InProgress,
    Blocked,
    Done,
}
impl TaskState {
    pub fn permits(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Created, Self::Accepted)
                | (Self::Accepted, Self::InProgress)
                | (Self::InProgress, Self::Blocked | Self::Done)
                | (Self::Blocked, Self::InProgress)
        )
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub session_id: String,
    pub creator_session_id: Option<String>,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub priority: crate::task_intents::Priority,
    #[serde(default)]
    pub granularity: crate::task_intents::Granularity,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub parent_id: Option<String>,
    pub status: TaskState,
    pub execution_epoch: u64,
    pub created_at: u64,
    pub updated_at: u64,
    pub started_at: Option<u64>,
    pub completed_at: Option<u64>,
    pub heartbeat_at: Option<u64>,
    pub waiting_reason: Option<String>,
    pub waiting_until: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionBinding {
    pub id: String,
    pub engagement_id: String,
    pub room_id: String,
    pub thread_root: Option<String>,
}
impl SessionBinding {
    pub fn validate(&self) -> Result<(), InvalidInput> {
        identifier(&self.id, 128)?;
        identifier(&self.engagement_id, 128)?;
        ruma_common::RoomId::parse(&self.room_id)
            .map_err(|_| InvalidInput("invalid session room"))?;
        if let Some(root) = &self.thread_root {
            ruma_common::EventId::parse(root)
                .map_err(|_| InvalidInput("invalid session thread"))?;
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceLease {
    pub id: String,
    pub exclusive: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DispatchInput {
    pub id: String,
    pub session_id: String,
    pub task_id: Option<String>,
    pub resources: Vec<ResourceLease>,
    pub payload: Value,
}
impl DispatchInput {
    pub fn validate(&self) -> Result<(), InvalidInput> {
        identifier(&self.id, 128)?;
        identifier(&self.session_id, 128)?;
        if let Some(task) = &self.task_id {
            identifier(task, 128)?;
        }
        if self.resources.len() > 16 {
            return Err(InvalidInput("too many dispatch resources"));
        }
        let mut seen = std::collections::BTreeSet::new();
        for resource in &self.resources {
            identifier(&resource.id, 128)?;
            if !seen.insert(&resource.id) {
                return Err(InvalidInput("duplicate dispatch resource"));
            }
        }
        if !self.payload.is_object()
            || serde_json::to_vec(self)
                .map_err(|_| InvalidInput("invalid dispatch"))?
                .len()
                > 64 * 1024
        {
            return Err(InvalidInput("dispatch exceeds object payload limit"));
        }
        crate::canonical::payload_digest(&self.payload)?;
        Ok(())
    }
}
/// Never include this object in console projections or tracing. Only the private runner receives it.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunnerCapability {
    pub dispatch_id: String,
    pub runner_id: String,
    pub fence: u64,
    pub secret: String,
}
impl std::fmt::Debug for RunnerCapability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunnerCapability")
            .field("dispatch_id", &self.dispatch_id)
            .field("runner_id", &self.runner_id)
            .field("fence", &self.fence)
            .finish_non_exhaustive()
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum TaskMutation {
    Accept,
    Transition {
        status: TaskState,
        waiting_reason: Option<String>,
        waiting_until: Option<String>,
    },
    Comment {
        text: String,
    },
    Execution {
        heartbeat: bool,
        #[serde(default, skip_serializing_if = "TextPatch::is_missing")]
        waiting_reason: TextPatch,
        #[serde(default, skip_serializing_if = "TextPatch::is_missing")]
        waiting_until: TextPatch,
    },
}
/// A missing patch leaves state alone; explicit null clears it.
#[derive(Debug, Clone, Default)]
pub enum TextPatch {
    #[default]
    Missing,
    Value(Option<String>),
}
impl TextPatch {
    pub fn is_missing(&self) -> bool {
        matches!(self, Self::Missing)
    }
}
impl Serialize for TextPatch {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Missing => s.serialize_none(),
            Self::Value(value) => value.serialize(s),
        }
    }
}
impl<'de> Deserialize<'de> for TextPatch {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(Self::Value(Option::<String>::deserialize(d)?))
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MutationResult {
    pub task: Task,
    pub replayed: bool,
}
/// The service supplies the clock when this command reaches the domain writer.
/// Runtime commands have no operator, session-admission or process-control variant.
#[derive(Debug, Clone, Serialize)]
pub enum RunnerCommand {
    CompleteTaskWithReply(crate::completions::CompleteTaskWithReply),
    SubmitFinalReply(crate::replies::FinalReply),
    FinalReply {
        id: String,
    },
    CreateWorkflow(crate::workflows::WorkflowRequest),
    Workflow {
        id: String,
    },
    Workflows {
        after: String,
        limit: usize,
    },
    CancelWorkflow {
        id: String,
        input: crate::workflows::WorkflowCancel,
    },
    WorkflowResult {
        id: String,
        input: crate::workflows::WorkflowResultRequest,
    },
    WorkflowDependencies {
        id: String,
        after: u64,
        limit: usize,
    },
    WorkflowDependency {
        id: String,
        node_id: String,
    },
    SendPeer(crate::peers::PeerSend),
    PeerInbox {
        after: u64,
        limit: usize,
    },
    OpenConversation(crate::conversations::ConversationRequest),
    ChangeConversation {
        id: String,
        change: crate::conversations::ConversationChange,
    },
    Conversation {
        id: String,
    },
    Delegate(crate::task_intents::Delegation),
    Check,
    Task {
        id: String,
    },
    Tasks {
        after: String,
        limit: usize,
    },
    Comments {
        id: String,
        after: u64,
        limit: usize,
    },
    Inbox {
        after: u64,
        limit: usize,
    },
    Mutate {
        id: String,
        call_id: String,
        operation: TaskMutation,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskComment {
    pub sequence: u64,
    pub author: String,
    pub text: String,
    pub created_at: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskEvent {
    pub sequence: u64,
    pub task_id: String,
    pub kind: String,
    pub task: Task,
}
pub fn clock(now: u64) -> Result<(), InvalidInput> {
    if now > JSON_SAFE_MAX {
        Err(InvalidInput("clock exceeds integer contract"))
    } else {
        Ok(())
    }
}
pub fn text(value: &str, limit: usize) -> Result<(), InvalidInput> {
    if value.trim().is_empty() || value.len() > limit || value.contains('\0') {
        Err(InvalidInput("text is empty or too long"))
    } else {
        Ok(())
    }
}
