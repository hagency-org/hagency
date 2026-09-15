//! Upload content and receipts are data, never storage or Matrix authority.
use crate::{InvalidInput, attachments::AttachmentMetadata, project::identifier};
use serde::{Deserialize, Serialize};

pub const MAX_UPLOADS: usize = 4096;
pub const MAX_DISPATCH_UPLOADS: usize = 16;
pub const MAX_UPLOAD_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Clone, Serialize)]
pub struct UploadRequest {
    pub call_id: String,
    /// Commitment to the original host-admitted request, not proof of file bytes.
    pub request_digest: String,
    pub metadata: AttachmentMetadata,
}
impl UploadRequest {
    pub fn validate(&self) -> Result<(), InvalidInput> {
        identifier(&self.call_id, 128)?;
        digest(&self.request_digest)?;
        self.metadata.validate()?;
        if self
            .metadata
            .declared_size
            .is_some_and(|v| v > MAX_UPLOAD_BYTES)
        {
            return Err(InvalidInput("upload exceeds byte bound"));
        }
        Ok(())
    }
}
/// Private host storage commitment. It contains no key, filename path or URI.
/// Deserialization is only for the protected domain journal, never HTTP input.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StageCommitment {
    pub namespace_digest: String,
    pub operation_id: String,
    pub receipt_digest: String,
    pub len: u64,
}
impl StageCommitment {
    pub fn validate(&self) -> Result<(), InvalidInput> {
        digest(&self.namespace_digest)?;
        digest(&self.receipt_digest)?;
        if self.operation_id.is_empty()
            || self.operation_id.len() > 128
            || !self
                .operation_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
            || self.len > MAX_UPLOAD_BYTES
        {
            return Err(InvalidInput("invalid upload staging commitment"));
        }
        Ok(())
    }
}
/// Host observations must come from the real retained staging adapter. This
/// enum cannot be decoded from runtime input and proves no physical IO itself.
#[derive(Clone, Copy)]
pub enum UploadStageObservation {
    FileAndDirectorySynced,
    OutcomeUnknown,
}
/// Exact acceptance evidence stays in a private transport journal. These are
/// commitments to that record, not a media URI or room-delivery receipt.
#[derive(Clone, Serialize)]
pub struct UploadAcceptance {
    pub receipt_id: String,
    pub receipt_digest: String,
}
impl UploadAcceptance {
    pub fn validate(&self) -> Result<(), InvalidInput> {
        identifier(&self.receipt_id, 128)?;
        digest(&self.receipt_digest)
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UploadStageState {
    Unbound,
    Bound,
    Staged,
    Unknown,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UploadState {
    Pending,
    Claimed,
    WritePossible,
    Accepted,
}
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct UploadReceipt {
    pub id: String,
    pub stage: UploadStageState,
    pub upload: UploadState,
    pub cancel_requested: bool,
    pub outcome_unknown: bool,
    pub replayed: bool,
}
pub fn digest(value: &str) -> Result<(), InvalidInput> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(InvalidInput("invalid upload commitment digest"));
    }
    Ok(())
}
