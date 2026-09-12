//! Typed upstream observations for one host-owned turn, never domain authority.
mod driver;
mod observation;
mod state;
mod task_mcp;
mod usage;
pub use driver::{
    ApprovalControlPolicy, ControlUpdate, PreparedApproval, PreparedUpdate, SessionDriver,
};
pub use observation::{
    Observation, ObservationKind, ObservationSource, ToolEvidence, ToolKind, ToolResult, TurnResult,
};
pub use task_mcp::{TASK_MCP_ENV, TASK_MCP_TOOLS, TaskMcp};
pub use usage::{UsageBreakdown, UsageDiagnostics, UsageEvidence};

use super::transport;
use serde_json::{Value, json};
use std::path::{Component, PathBuf};

pub const MAX_TEXT_BYTES: usize = 64 * 1024;
pub const MAX_ITEMS: usize = 128;
pub const MAX_EVENTS: usize = 4096;
pub const MAX_DEFERRED: usize = 16;
pub const MAX_DEFERRED_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("invalid native Codex session settings")]
    Settings,
    #[error("invalid native Codex session state")]
    State,
    #[error("native Codex session identity mismatch")]
    Scope,
    #[error("invalid native Codex lifecycle observation")]
    Malformed,
    #[error("native Codex session capacity exceeded")]
    Capacity,
    #[error("native Codex reported a different execution policy")]
    Policy,
    #[error("native Codex RPC was rejected with code {0}")]
    Rejected(i64),
    #[error("native Codex server request has no authority adapter")]
    UnsupportedRequest,
    #[error("native Codex notification is unsupported")]
    UnsupportedEvent,
    #[error("native Codex session operation was cancelled")]
    Cancelled,
    #[error("native Codex session transport failed: {0}")]
    Transport(transport::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    New,
    Initializing,
    Ready,
    OpeningThread,
    ThreadReady,
    StartingTurn,
    Running,
    Ended,
}

/// Upstream observations only. None establish canonical completion or cleanup.
pub enum Outcome {
    Completed { text: String },
    Failed,
    Interrupted,
    UnsupportedRequest,
    Unknown { reason: Error },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterruptDisposition {
    Acknowledged,
    Rejected { code: i64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemPhase {
    Active,
    Complete,
}

pub enum Update {
    Approval(super::approval::ApprovalRequest),
    /// Upstream callback ended, possibly by cancellation and before applying.
    ApprovalResolved {
        id: super::RequestId,
    },
    Notice,
    TurnStarted,
    ThreadStatus,
    Item {
        id: String,
        kind: String,
        phase: ItemPhase,
    },
    TextDelta {
        item_id: String,
        delta: String,
    },
    Progress,
    Retrying,
    TurnEnded,
}

/// A host-recorded upstream thread, not a Matrix/Hagency session credential.
pub struct ResumeThreadId(String);
impl ResumeThreadId {
    pub fn new(id: String) -> Result<Self, Error> {
        valid_id(&id)?;
        Ok(Self(id))
    }
}

/// Host-only settings. No Deserialize, arbitrary configuration map, network
/// enablement, approval-policy override or YOLO switch exists here.
pub struct Settings {
    cwd: String,
    model: String,
    effort: String,
    read_only: bool,
    task_mcp: Option<TaskMcp>,
}
impl Settings {
    pub fn new(cwd: PathBuf, model: String, effort: String) -> Result<Self, Error> {
        if !cwd.is_absolute()
            || cwd
                .components()
                .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
        {
            return Err(Error::Settings);
        }
        let cwd = cwd
            .to_str()
            .filter(|v| super::text(v, 4096))
            .ok_or(Error::Settings)?
            .to_owned();
        if !super::text(&model, 128)
            || model.trim() != model
            || !super::text(&effort, 32)
            || effort.trim() != effort
        {
            return Err(Error::Settings);
        }
        Ok(Self {
            cwd,
            model,
            effort,
            read_only: false,
            task_mcp: None,
        })
    }
    pub fn read_only(mut self) -> Self {
        self.read_only = true;
        self
    }
    pub fn is_read_only(&self) -> bool {
        self.read_only
    }
    pub fn cwd(&self) -> &str {
        &self.cwd
    }
    pub fn model(&self) -> &str {
        &self.model
    }
    pub fn effort(&self) -> &str {
        &self.effort
    }
    fn mode(&self) -> &'static str {
        if self.read_only {
            "read-only"
        } else {
            "workspace-write"
        }
    }
    fn policy(&self) -> Value {
        if self.read_only {
            json!({ "type": "readOnly", "networkAccess": false })
        } else {
            json!({ "type": "workspaceWrite", "writableRoots": [self.cwd], "networkAccess": false })
        }
    }
    fn thread_request(&self, resume: Option<&str>) -> Value {
        let mut params = json!({ "cwd": self.cwd, "model": self.model, "sandbox": self.mode(),
            "approvalPolicy": "on-request", "approvalsReviewer": "user" });
        if let Some(helper) = &self.task_mcp {
            params["config"] = helper.config(&self.cwd);
            params["developerInstructions"] = helper.guidance().into();
        }
        if let Some(id) = resume {
            params["threadId"] = id.into();
            params["excludeTurns"] = true.into();
        } else {
            params["ephemeral"] = true.into();
            params["serviceName"] = "hagency".into();
        }
        params
    }
    fn turn_request(&self, thread: &str, input: String) -> Value {
        json!({ "threadId": thread, "input": [{ "type": "text", "text": input, "text_elements": [] }],
            "cwd": self.cwd, "model": self.model, "effort": self.effort,
            "approvalPolicy": "on-request", "approvalsReviewer": "user", "sandboxPolicy": self.policy() })
    }
}

fn valid_id(value: &str) -> Result<(), Error> {
    if super::text(value, 256) {
        Ok(())
    } else {
        Err(Error::Malformed)
    }
}
fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str, Error> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or(Error::Malformed)
}
fn id<'a>(value: &'a Value, key: &str) -> Result<&'a str, Error> {
    let value = string(value, key)?;
    valid_id(value)?;
    Ok(value)
}
fn object<'a>(value: &'a Value, key: &str) -> Result<&'a Value, Error> {
    value
        .get(key)
        .filter(|v| v.is_object())
        .ok_or(Error::Malformed)
}
