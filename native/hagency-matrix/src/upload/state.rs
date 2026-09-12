use super::*;
use hagency_media_store::{Kind, RestoredEncrypted, SyncEvidence};
use hagency_store::{UploadClaim, UploadSend};
use std::collections::BTreeMap;
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore};

const MAX_HELD: usize = 2;

/// Original host-owned inputs. This type contains no caller-selected URL or room.
/// Construction proves association, not current permission to contact Matrix.
pub struct StagedUpload {
    pub(super) cap: RunnerCapability,
    pub(super) claim: UploadClaim,
    pub(super) send: Option<UploadSend>,
    pub(super) media: RestoredEncrypted,
}
impl StagedUpload {
    pub fn new(
        cap: RunnerCapability,
        claim: UploadClaim,
        send: UploadSend,
        media: RestoredEncrypted,
    ) -> Result<Self, UploadAdmissionFailure> {
        let input = Self {
            cap,
            claim,
            send: Some(send),
            media,
        };
        if !input.matches() {
            return Err(UploadAdmissionFailure::new(Error::Conflict, input));
        }
        Ok(input)
    }
    pub(super) fn matches(&self) -> bool {
        // RunnerCapability fields are public host data. Bound them before any
        // retained admission or clone, using original upload admission syntax.
        if hagency_core::project::identifier(&self.cap.dispatch_id, 128).is_err()
            || hagency_core::project::identifier(&self.cap.runner_id, 128).is_err()
            || hagency_core::uploads::digest(&self.cap.secret).is_err()
            || self.cap.fence == 0
        {
            return false;
        }
        let Some(send) = &self.send else {
            return false;
        };
        let receipt = self.media.receipt();
        let stage = send.stage();
        send.matches_claim(&self.claim)
            && receipt.kind() == Kind::Encrypted
            && receipt.sync_evidence() == SyncEvidence::FileAndDirectorySynced
            && stage.namespace_digest == hex(self.media.namespace_digest())
            && stage.operation_id == receipt.operation().as_str()
            && stage.receipt_digest == hex(receipt.digest())
            && stage.len == receipt.len() as u64
            && receipt.len() == self.media.ciphertext().len()
    }
    /// Recover original unadmitted custody; no replacement grant is created.
    pub fn into_parts(self) -> (RunnerCapability, UploadClaim, UploadSend, RestoredEncrypted) {
        // Public values have not entered an operation and always retain Send.
        (
            self.cap,
            self.claim,
            self.send.expect("unadmitted send retained"),
            self.media,
        )
    }
}
/// No Debug/serde: a refusal returns the original ciphertext and unique grant.
pub struct UploadAdmissionFailure {
    error: Error,
    input: Box<StagedUpload>,
}
impl UploadAdmissionFailure {
    pub(super) fn new(error: Error, input: StagedUpload) -> Self {
        Self {
            error,
            input: Box::new(input),
        }
    }
    pub fn error(&self) -> Error {
        self.error
    }
    pub fn into_input(self) -> StagedUpload {
        *self.input
    }
}
fn hex(bytes: &[u8; 32]) -> String {
    use std::fmt::Write;
    let mut result = String::with_capacity(64);
    for byte in bytes {
        write!(result, "{byte:02x}").expect("String write");
    }
    result
}

pub(crate) struct Registry {
    pub(super) slots: Arc<Semaphore>,
    pub(super) entries: std::sync::Mutex<Entries>,
    #[cfg(test)]
    pub(super) gate: std::sync::atomic::AtomicU8,
    #[cfg(test)]
    pub(super) reached: tokio::sync::Notify,
    #[cfg(test)]
    pub(super) proceed: tokio::sync::Notify,
}
pub(super) struct Entries {
    pub closed: bool,
    pub jobs: BTreeMap<String, Arc<Job>>,
    pub publications: BTreeMap<String, Arc<super::publication::Job>>,
}
impl Registry {
    pub(crate) fn new() -> Self {
        Self {
            slots: Arc::new(Semaphore::new(MAX_HELD)),
            entries: std::sync::Mutex::new(Entries {
                closed: false,
                jobs: BTreeMap::new(),
                publications: BTreeMap::new(),
            }),
            #[cfg(test)]
            gate: std::sync::atomic::AtomicU8::new(0),
            #[cfg(test)]
            reached: tokio::sync::Notify::new(),
            #[cfg(test)]
            proceed: tokio::sync::Notify::new(),
        }
    }
    pub(crate) fn close(&self) -> Result<(), Error> {
        let mut entries = self.entries.lock().map_err(|_| Error::Storage)?;
        if !entries.jobs.is_empty() || !entries.publications.is_empty() {
            return Err(Error::Busy);
        }
        entries.closed = true;
        Ok(())
    }
    pub(super) fn find(&self, id: &str) -> Result<Option<Arc<Job>>, Error> {
        selector(id)?;
        Ok(self
            .entries
            .lock()
            .map_err(|_| Error::Storage)?
            .jobs
            .get(id)
            .cloned())
    }
    pub(super) fn release(&self, id: &str) -> Result<(), Error> {
        self.entries
            .lock()
            .map_err(|_| Error::Storage)?
            .jobs
            .remove(id);
        Ok(())
    }
}
pub(super) struct Job {
    pub id: String,
    pub state: Mutex<State>,
    pub _slot: OwnedSemaphorePermit,
    pub fence_error: std::sync::Mutex<Option<Error>>,
    #[cfg(test)]
    pub fence_finished: tokio::sync::Notify,
}
pub(super) struct State {
    pub input: StagedUpload,
    pub attempted: bool,
    pub http_started: bool,
    pub reference: Option<Reference>,
    pub response: Option<UploadResponse>,
    pub outcome: Option<Result<UploadReceipt, Error>>,
}
pub(super) fn selector(id: &str) -> Result<(), Error> {
    if id.len() != 39
        || !id.starts_with("upload_")
        || !id[7..]
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        Err(Error::Config)
    } else {
        Ok(())
    }
}
