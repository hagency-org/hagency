use super::{FileError, FileStatus, FileView, SendFile, types::UnknownOrigin};
use hagency_core::{file_delivery::FileDeliveryRequest, tasks::RunnerCapability};
use hagency_files::Snapshot;
use hagency_matrix::{
    FilePublicationAdmissionFailure, FilePublicationOperation, UploadAdmissionFailure,
    UploadOperation,
};
use hagency_media::Encrypted;
use hagency_media_store::{Media, PreparedEncrypted, RestoredEncrypted};
use hagency_store::{FileDeliveryAdmission, UploadPreparation};
use std::sync::{Arc, Mutex};

pub(super) struct Job {
    pub key: String,
    pub cap: RunnerCapability,
    pub input: SendFile,
    pub request: FileDeliveryRequest,
    pub info: Mutex<Info>,
    pub original: tokio::sync::Mutex<Original>,
}
#[derive(Default)]
pub(super) struct Info {
    pub result: Option<Result<FileView, FileError>>,
    pub id: Option<String>,
    pub live: bool,
    pub releasable: bool,
}
#[derive(Default)]
pub(super) struct Original {
    pub admission: Option<FileDeliveryAdmission>,
    pub preparation: Option<Arc<UploadPreparation>>,
    pub snapshot: Option<Snapshot>,
    pub encrypted: Option<Encrypted>,
    pub prepared: Option<PreparedEncrypted>,
    pub restored: Option<RestoredEncrypted>,
    pub upload: Option<UploadOperation>,
    pub publication: Option<FilePublicationOperation>,
    pub returned: Option<Media>,
    pub rejected_upload: Option<UploadAdmissionFailure>,
    pub rejected_publication: Option<FilePublicationAdmissionFailure>,
}
impl Job {
    pub fn release_when_joined(&self) {
        if let Ok(mut info) = self.info.lock() {
            info.live = false;
            info.releasable = true;
        }
    }
    /// The actual task ended without acknowledging safe custody release.
    pub fn mark_unknown(&self) {
        if let Ok(mut info) = self.info.lock() {
            info.live = false;
            info.releasable = false;
            match info.result.as_mut() {
                Some(Ok(view)) if view.status == FileStatus::Queued => {
                    view.status = FileStatus::OutcomeUnknown;
                    // Name the producer: this unknown is local custody that was
                    // not acknowledged or released, not a durable remote outcome.
                    view.error_code = Some(UnknownOrigin::Custody.code().into());
                }
                // Existing terminal receipts remain durable facts even when
                // local custody cannot be acknowledged or released.
                Some(Ok(_)) => {}
                _ => info.result = Some(Err(FileError::Unknown)),
            }
        }
    }
    pub fn result(&self) -> Result<FileView, FileError> {
        self.info
            .lock()
            .map_err(|_| FileError::Unknown)?
            .result
            .clone()
            .unwrap_or(Err(FileError::Unknown))
    }
    pub fn publish(&self, result: Result<FileView, FileError>, live: bool) {
        if let Ok(mut info) = self.info.lock() {
            if let Ok(value) = &result {
                info.id = Some(value.delivery_id.clone());
            }
            info.result = Some(result);
            info.live = live;
        }
    }
}
