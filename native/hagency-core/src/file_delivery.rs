//! File metadata and historical receipt correlations are data, never SDK proof.
use crate::{
    InvalidInput, attachments::AttachmentMetadata, project::identifier, replies::*, uploads::*,
};
use serde::{Deserialize, Serialize};

pub const MAX_FILE_DELIVERIES: usize = 4096;
pub const MAX_DISPATCH_FILE_DELIVERIES: usize = 16;
pub const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;
pub const MAX_FILE_RECORD_BYTES: usize = 8192;
pub const FILE_MIME: &str = "application/octet-stream";

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FileDeliveryRequest {
    pub call_id: String,
    /// Commitment to the private original selection. No path is retained here.
    pub request_digest: String,
    pub filename: String,
    pub caption: Option<String>,
}
impl FileDeliveryRequest {
    pub fn validate(&self) -> Result<(), InvalidInput> {
        identifier(&self.call_id, 128)?;
        digest(&self.request_digest)?;
        if self.filename.len() > 255 {
            return Err(InvalidInput("invalid file filename"));
        }
        AttachmentMetadata {
            filename: self.filename.clone(),
            mime_type: None,
            declared_size: None,
        }
        .validate()?;
        if self
            .caption
            .as_ref()
            .is_some_and(|v| v.len() > 1000 || v.contains('\0'))
        {
            return Err(InvalidInput("invalid file caption"));
        }
        Ok(())
    }
}
/// Host observation from the original retained snapshot; not filesystem proof.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CapturedFile {
    pub size: u64,
    pub sha256: String,
}
impl CapturedFile {
    pub fn validate(&self) -> Result<(), InvalidInput> {
        if self.size > MAX_FILE_BYTES {
            return Err(InvalidInput("file exceeds product byte bound"));
        }
        digest(&self.sha256)
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FileEventState {
    Pending,
    Claimed,
    WritePossible,
    Delivered,
}
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FileDeliveryStatus {
    Queued,
    OutcomeUnknown,
    Failed,
    Delivered,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FileDeliveryFailure {
    Cancelled,
    SourceRefused,
    StagingRefused,
    PublicationRefused,
}
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct FileDeliveryReceipt {
    pub id: String,
    pub filename: String,
    pub captured: Option<CapturedFile>,
    pub stage: UploadStageState,
    pub upload: UploadState,
    pub event: FileEventState,
    pub status: FileDeliveryStatus,
    pub cancel_requested: bool,
    pub error_code: Option<FileDeliveryFailure>,
    pub event_id: Option<String>,
    pub replayed: bool,
}
/// Bounded protected-journal lookup DATA. No claim, preparation or send authority.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FilePublicationLocator {
    pub delivery_id: String,
    pub fence: u64,
    pub transaction_id: String,
    pub content_digest: String,
    pub upload_id: String,
    pub upload_fence: u64,
    pub stage: StageCommitment,
    pub route: ReplyRoute,
    pub upload_receipt_id: String,
    pub upload_receipt_digest: String,
}
/// Trusted host correlation DATA, never a verified SDK acknowledgement. The
/// publisher must first retain its actual complete private SDK acceptance.
#[derive(Clone, Serialize)]
pub struct FileDeliveryAcceptance {
    pub transaction_id: String,
    pub content_digest: String,
    pub event_id: String,
    pub receipt_id: String,
    pub receipt_digest: String,
}
impl FileDeliveryAcceptance {
    pub fn validate(&self) -> Result<(), InvalidInput> {
        identifier(&self.transaction_id, 128)?;
        identifier(&self.receipt_id, 128)?;
        digest(&self.content_digest)?;
        digest(&self.receipt_digest)?;
        matrix_event(&self.event_id)
    }
}
