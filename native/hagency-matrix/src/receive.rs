//! Current dispatch authority around retained manifests and bounded decryption.
//! Host-local memory only: no cache path, MCP endpoint or enduring export grant.
use crate::{
    CancellationToken, Collector, Error, MediaDownloadError, MediaDownloadLimits, MediaDownloader,
};
use hagency_core::{attachments::AttachmentMetadata, tasks::RunnerCapability};
use hagency_media::CheckedBytes;
use hagency_store::{AttachmentTicket, DomainStore};
use std::{
    sync::{Arc, OnceLock},
    time::Duration,
};
use tokio::{
    sync::{OwnedSemaphorePermit, Semaphore},
    time::Instant,
};

const MAX_RESULTS: usize = 4;
const MAX_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ReceiveError {
    #[error(transparent)]
    Authority(#[from] Error),
    #[error(transparent)]
    Download(#[from] MediaDownloadError),
}

pub(crate) struct Receiver {
    downloader: OnceLock<Result<MediaDownloader, MediaDownloadError>>,
    results: Arc<Semaphore>,
}
impl Receiver {
    pub(crate) fn new() -> Self {
        Self {
            downloader: OnceLock::new(),
            results: Arc::new(Semaphore::new(MAX_RESULTS)),
        }
    }
}

/// Complete checked bytes from an authenticated retained manifest, scoped at
/// return time. Later revocation cannot erase a trusted holder's memory; delayed
/// export requires current authority again. No serialization or public constructor.
pub struct ReceivedAttachment {
    checked: CheckedBytes,
    scope: ReceivedScope,
    deadline: Instant,
    cancel: CancellationToken,
    _permit: OwnedSemaphorePermit,
}
impl ReceivedAttachment {
    pub fn bytes(&self) -> &[u8] {
        self.checked.bytes()
    }
    pub fn digest(&self) -> &[u8; 32] {
        self.checked.digest()
    }
    /// Validated syntax only; MIME and declared size are sender observations.
    pub fn metadata(&self) -> &AttachmentMetadata {
        self.scope.ticket.metadata()
    }
    /// Host association data, not a public verification or filesystem grant.
    pub fn ticket(&self) -> &AttachmentTicket {
        self.scope.ticket()
    }
    /// Rechecks the original writer and original total receive deadline.
    pub async fn revalidate(&self) -> Result<(), ReceiveError> {
        self.scope.revalidate(&self.cancel, self.deadline).await
    }
    /// Releases plaintext and both result permits. The returned scope supports
    /// only current read-only checks; it cannot download or authorize a write.
    pub fn into_scope(self) -> ReceivedScope {
        self.scope
    }
}

/// Original receive association for a trusted host's retained cache object.
/// No bytes, path, replacement writer, constructor, Clone, Debug or serde.
pub struct ReceivedScope {
    domain: DomainStore,
    cap: RunnerCapability,
    ticket: AttachmentTicket,
    read_budget: Duration,
}
impl ReceivedScope {
    pub fn ticket(&self) -> &AttachmentTicket {
        &self.ticket
    }
    /// A later cache read has its own bounded response deadline. This checks
    /// current captured authority only, not the cache's physical file identity.
    pub async fn revalidate(
        &self,
        cancel: &CancellationToken,
        deadline: Instant,
    ) -> Result<(), ReceiveError> {
        let deadline = deadline.min(Instant::now() + self.read_budget);
        checkpoint(cancel, deadline)?;
        tokio::select! {
            biased;
            _ = cancel.cancelled() => Err(Error::Cancelled.into()),
            _ = tokio::time::sleep_until(deadline) => Err(Error::Timeout.into()),
            result = self.domain.revalidate_attachment(self.cap.clone(), self.ticket.clone()) => {
                checkpoint(cancel, deadline)?;
                result.map_err(Error::from).map_err(ReceiveError::from)
            }
        }
    }
}

impl Collector {
    /// Uses prior authenticated collector observations and current domain truth,
    /// not a claim that remote room membership cannot change between observations.
    pub async fn receive_attachment(
        &self,
        cap: RunnerCapability,
        event_id: String,
        cancel: &CancellationToken,
    ) -> Result<ReceivedAttachment, ReceiveError> {
        let deadline = Instant::now() + self.inner.config.limits.sdk;
        self.receive_attachment_until(cap, event_id, cancel, deadline, MAX_BYTES)
            .await
    }
    /// Host-owned operation deadline is captured before queueing. Both this
    /// deadline and byte bound may only narrow existing SDK/transport limits.
    pub async fn receive_attachment_until(
        &self,
        cap: RunnerCapability,
        event_id: String,
        cancel: &CancellationToken,
        deadline: Instant,
        max_bytes: usize,
    ) -> Result<ReceivedAttachment, ReceiveError> {
        let deadline = deadline.min(Instant::now() + self.inner.config.limits.sdk);
        checkpoint(cancel, deadline)?;
        if !(1..=MAX_BYTES).contains(&max_bytes) {
            return Err(MediaDownloadError::Config.into());
        }
        let permit = self
            .inner
            .receiver
            .results
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Capacity)?;
        tokio::select! {
            biased;
            _ = cancel.cancelled() => Err(Error::Cancelled.into()),
            _ = tokio::time::sleep_until(deadline) => Err(Error::Timeout.into()),
            result = self.receive(cap, event_id, cancel, deadline, max_bytes, permit) => {
                checkpoint(cancel, deadline)?;
                result
            }
        }
    }
    async fn receive(
        &self,
        cap: RunnerCapability,
        event_id: String,
        cancel: &CancellationToken,
        deadline: Instant,
        max_bytes: usize,
        permit: OwnedSemaphorePermit,
    ) -> Result<ReceivedAttachment, ReceiveError> {
        let ticket = self
            .inner
            .domain
            .authorize_attachment(cap.clone(), event_id)
            .await
            .map_err(Error::from)?;
        if ticket
            .metadata()
            .declared_size
            .is_some_and(|size| size > max_bytes as u64)
        {
            return Err(MediaDownloadError::Transport(Error::BodyTooLarge).into());
        }
        let handle = self
            .attachment_manifest(cap.clone(), ticket.clone(), cancel)
            .await?;
        // Lookup's Owner mutex and busy permit are gone before network waits:
        // intake and authenticated negative observations must remain runnable.
        let downloader = self
            .inner
            .receiver
            .downloader
            .get_or_init(|| {
                MediaDownloader::new(&self.inner.config, MediaDownloadLimits::default())
            })
            .as_ref()
            .map_err(|e| *e)?;
        let checked = downloader
            .download_bounded_until(
                handle.media_id(),
                handle.descriptor(),
                cancel,
                deadline,
                max_bytes,
            )
            .await?;
        let scope = ReceivedScope {
            domain: self.inner.domain.clone(),
            cap,
            ticket,
            read_budget: self.inner.config.limits.sdk,
        };
        scope.revalidate(cancel, deadline).await?;
        checkpoint(cancel, deadline)?;
        Ok(ReceivedAttachment {
            checked,
            scope,
            deadline,
            cancel: cancel.clone(),
            _permit: permit,
        })
    }
}
fn checkpoint(cancel: &CancellationToken, deadline: Instant) -> Result<(), Error> {
    if cancel.is_cancelled() {
        Err(Error::Cancelled)
    } else if Instant::now() >= deadline {
        Err(Error::Timeout)
    } else {
        Ok(())
    }
}
