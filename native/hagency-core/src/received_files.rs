//! Local receive facts are data, not SDK verification or physical workspace proof.
use crate::{
    InvalidInput, attachments::AttachmentMetadata, project::identifier, replies::matrix_event,
    tasks::clock, uploads::digest,
};
use serde::{Deserialize, Serialize};

pub const MAX_RECEIVED_FILE_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_RECEIVED_FILES: usize = 32;
pub const MAX_WORKSPACE_RECEIVED_FILES: usize = 8;
pub const MAX_RECEIVED_RESERVED_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReceiveInboxPlan {
    pub dispatch_id: String,
    pub session_id: String,
    pub task_id: String,
    pub workspace_id: String,
}
impl ReceiveInboxPlan {
    pub fn validate(&self) -> Result<(), InvalidInput> {
        for id in [
            &self.dispatch_id,
            &self.session_id,
            &self.task_id,
            &self.workspace_id,
        ] {
            identifier(id, 128)?;
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReceiveInboxSelection {
    NoWake,
    Selected {
        dispatch_id: String,
        count: usize,
        replayed: bool,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReceivedFileFacts {
    pub size: u64,
    pub sha256: String,
}
impl ReceivedFileFacts {
    pub fn validate(&self, limit: usize) -> Result<(), InvalidInput> {
        receive_limit(limit)?;
        clock(self.size)?;
        if self.size > limit as u64 {
            return Err(InvalidInput("received file exceeds configured bound"));
        }
        digest(&self.sha256)
    }
}
pub fn receive_limit(limit: usize) -> Result<(), InvalidInput> {
    if !(1..=MAX_RECEIVED_FILE_BYTES).contains(&limit) {
        return Err(InvalidInput("invalid receive byte bound"));
    }
    Ok(())
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReceivedFileState {
    Reserved,
    WritePossible,
    Ready,
    Failed,
    OutcomeUnknown,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReceiveFailure {
    Cancelled,
    SourceRefused,
    WriteRefused,
    OutcomeUnknown,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReceivedFileObservation {
    pub id: String,
    pub event_id: String,
    pub metadata: AttachmentMetadata,
    pub facts: Option<ReceivedFileFacts>,
    pub state: ReceivedFileState,
    pub error_code: Option<ReceiveFailure>,
    pub replayed: bool,
}
impl ReceivedFileObservation {
    pub fn validate(&self) -> Result<(), InvalidInput> {
        identifier(&self.id, 128)?;
        matrix_event(&self.event_id)?;
        self.metadata.validate()?;
        if let Some(facts) = &self.facts {
            facts.validate(MAX_RECEIVED_FILE_BYTES)?;
        }
        if matches!(
            self.state,
            ReceivedFileState::WritePossible | ReceivedFileState::Ready
        ) && self.facts.is_none()
            || matches!(
                self.state,
                ReceivedFileState::Reserved
                    | ReceivedFileState::WritePossible
                    | ReceivedFileState::Ready
            ) && self.error_code.is_some()
            || matches!(
                self.state,
                ReceivedFileState::Failed | ReceivedFileState::OutcomeUnknown
            ) && self.error_code.is_none()
        {
            return Err(InvalidInput("invalid received file observation"));
        }
        Ok(())
    }
}
