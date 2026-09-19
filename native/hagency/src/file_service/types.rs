use hagency_core::{
    attachments::AttachmentMetadata, canonical, file_delivery::*, project::identifier,
    tasks::RunnerCapability,
};
use hagency_files::RelativeFile;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SendFile {
    pub call_id: String,
    pub path: String,
    pub filename: Option<String>,
    pub caption: Option<String>,
}
impl SendFile {
    pub fn validate(&self) -> Result<(), FileError> {
        self.request().map(|_| ())
    }
    pub(super) fn request(&self) -> Result<FileDeliveryRequest, FileError> {
        RelativeFile::new(&self.path).map_err(|_| FileError::Invalid)?;
        let filename = self
            .filename
            .clone()
            .unwrap_or_else(|| self.path.rsplit('/').next().unwrap_or_default().to_owned());
        let digest = canonical::payload_digest(&serde_json::json!([
            "native_file_selection_v1",
            self.path,
            filename,
            self.caption,
            FILE_MIME
        ]))
        .map_err(|_| FileError::Invalid)?;
        let request = FileDeliveryRequest {
            call_id: self.call_id.clone(),
            request_digest: digest,
            filename,
            caption: self.caption.clone(),
        };
        request.validate().map_err(|_| FileError::Invalid)?;
        if serde_json::to_vec(self)
            .map_err(|_| FileError::Invalid)?
            .len()
            > 16 * 1024
        {
            return Err(FileError::Invalid);
        }
        Ok(request)
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FileStatus {
    Queued,
    OutcomeUnknown,
    Failed,
    Delivered,
}
/// Which producer an `outcome_unknown` code came from (ADR-101 amendment
/// 2026-09-12). The receipt-derived projection keeps the base code; local
/// custody that was neither acknowledged nor released carries the custody
/// label. Both stay `FileStatus::OutcomeUnknown` and neither is terminal.
pub(crate) enum UnknownOrigin {
    /// The durable receipt is not a terminal outcome (from_receipt fallback).
    Remote,
    /// The local job's custody was neither acknowledged nor released.
    Custody,
}
impl UnknownOrigin {
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::Remote => "outcome_unknown",
            Self::Custody => "outcome_unknown_custody",
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct FileView {
    pub delivery_id: String,
    pub filename: String,
    pub status: FileStatus,
    pub replayed: bool,
    pub error_code: Option<String>,
}
impl FileView {
    pub fn validate(&self) -> Result<(), FileError> {
        identifier(&self.delivery_id, 128).map_err(|_| FileError::Invalid)?;
        AttachmentMetadata {
            filename: self.filename.clone(),
            mime_type: None,
            declared_size: None,
        }
        .validate()
        .map_err(|_| FileError::Invalid)?;
        if self.filename.len() > 255 {
            return Err(FileError::Invalid);
        }
        let valid = match self.status {
            FileStatus::Queued | FileStatus::Delivered => self.error_code.is_none(),
            // A closed set: the base code or the custody-labelled variant. No
            // other string may be attached to OutcomeUnknown.
            FileStatus::OutcomeUnknown => matches!(
                self.error_code.as_deref(),
                Some(code)
                    if code == UnknownOrigin::Remote.code() || code == UnknownOrigin::Custody.code()
            ),
            FileStatus::Failed => matches!(
                self.error_code.as_deref(),
                Some("cancelled" | "source_refused" | "staging_refused" | "publication_refused")
            ),
        };
        if !valid
            || serde_json::to_vec(self)
                .map_err(|_| FileError::Invalid)?
                .len()
                > 4096
        {
            return Err(FileError::Invalid);
        }
        Ok(())
    }
    pub(super) fn from_receipt(receipt: FileDeliveryReceipt, locally_running: bool) -> Self {
        let status = match receipt.status {
            // Exact historical acceptance outranks a retained cancellation
            // request. The public receipt omits that internal failure history.
            FileDeliveryStatus::Delivered
                if receipt.event == FileEventState::Delivered && receipt.event_id.is_some() =>
            {
                FileStatus::Delivered
            }
            FileDeliveryStatus::Failed if receipt.error_code.is_some() => FileStatus::Failed,
            FileDeliveryStatus::Queued if locally_running && receipt.error_code.is_none() => {
                FileStatus::Queued
            }
            _ => FileStatus::OutcomeUnknown,
        };
        let error_code = match status {
            FileStatus::OutcomeUnknown => Some(UnknownOrigin::Remote.code()),
            FileStatus::Failed => Some(match receipt.error_code {
                Some(FileDeliveryFailure::Cancelled) => "cancelled",
                Some(FileDeliveryFailure::SourceRefused) => "source_refused",
                Some(FileDeliveryFailure::StagingRefused) => "staging_refused",
                Some(FileDeliveryFailure::PublicationRefused) => "publication_refused",
                None => UnknownOrigin::Remote.code(),
            }),
            _ => None,
        }
        .map(str::to_owned);
        Self {
            delivery_id: receipt.id,
            filename: receipt.filename,
            status,
            replayed: receipt.replayed,
            error_code,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FileError {
    Invalid,
    Unauthorized,
    Conflict,
    Busy,
    Unavailable,
    Unknown,
}
impl FileError {
    pub fn code(self) -> &'static str {
        match self {
            Self::Invalid => "invalid_file_operation",
            Self::Unauthorized => "file_scope_required",
            Self::Conflict => "idempotency_conflict",
            Self::Busy => "busy",
            Self::Unavailable => "unavailable",
            Self::Unknown => "outcome_unknown",
        }
    }
}
impl From<hagency_store::Error> for FileError {
    fn from(error: hagency_store::Error) -> Self {
        use hagency_store::Error;
        match error {
            Error::Invalid(_) => Self::Invalid,
            Error::RunnerAuthority | Error::NotFound => Self::Unauthorized,
            Error::Conflict => Self::Conflict,
            Error::Busy | Error::Capacity => Self::Busy,
            Error::OutcomeUnknown => Self::Unknown,
            _ => Self::Unavailable,
        }
    }
}
pub(super) fn capability(cap: &RunnerCapability) -> Result<(), FileError> {
    identifier(&cap.dispatch_id, 128).map_err(|_| FileError::Invalid)?;
    identifier(&cap.runner_id, 128).map_err(|_| FileError::Invalid)?;
    hagency_core::uploads::digest(&cap.secret).map_err(|_| FileError::Invalid)?;
    if cap.fence == 0 || cap.fence > hagency_core::JSON_SAFE_MAX {
        return Err(FileError::Invalid);
    }
    Ok(())
}
pub(super) fn key(cap: &RunnerCapability, call: &str) -> Result<String, FileError> {
    capability(cap)?;
    canonical::digest(&serde_json::json!([
        "file_job_v1",
        cap.dispatch_id,
        cap.runner_id,
        cap.fence,
        cap.secret,
        call
    ]))
    .map_err(|_| FileError::Invalid)
}
pub(super) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
