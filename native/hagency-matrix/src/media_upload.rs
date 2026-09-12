//! Encrypted upload transport, not authenticated event or room-send authority.
//! An uncertain POST is not safely retryable: Matrix provides no transaction ID.
use crate::{Error, HostConfig, MediaId, UploadResponse, http::Http};
use hagency_media::Encrypted;
use std::sync::Arc;
use tokio::{
    sync::{OwnedSemaphorePermit, Semaphore},
    time::Instant,
};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum MediaUploadError {
    #[error("invalid encrypted media upload configuration")]
    Config,
    #[error("upload attempt may already have written and cannot be sent again")]
    Terminal,
    #[error(transparent)]
    Transport(#[from] Error),
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UploadState {
    Prepared,
    /// Conservatively unknown, including when a pending send future was dropped.
    WritePossible,
    /// Exact repository response observed; no event or room delivery is implied.
    Accepted,
}
#[derive(Clone, Copy)]
pub struct MediaUploadLimits {
    bytes: usize,
    active: usize,
    attempts: usize,
}
impl MediaUploadLimits {
    pub fn new(bytes: usize, active: usize, attempts: usize) -> Result<Self, MediaUploadError> {
        if !(1..=16 * 1024 * 1024).contains(&bytes)
            || !(1..=4).contains(&active)
            || !(1..=8).contains(&attempts)
        {
            return Err(MediaUploadError::Config);
        }
        Ok(Self {
            bytes,
            active,
            attempts,
        })
    }
}
impl Default for MediaUploadLimits {
    fn default() -> Self {
        Self {
            bytes: 4 * 1024 * 1024,
            active: 2,
            attempts: 4,
        }
    }
}
struct Inner {
    http: Http,
    limits: MediaUploadLimits,
    active: Semaphore,
    attempts: Arc<Semaphore>,
    deadline: std::time::Duration,
}
/// Clones share finite attempts and active requests. No SDK or state directory
/// is opened; HostConfig credentials are not independently verified by whoami.
#[derive(Clone)]
pub struct MediaUploader(Arc<Inner>);
impl MediaUploader {
    pub fn new(config: &HostConfig, limits: MediaUploadLimits) -> Result<Self, MediaUploadError> {
        if config.endpoint.scheme() != "https" {
            return Err(MediaUploadError::Config);
        }
        Ok(Self(Arc::new(Inner {
            http: Http::new(config)?,
            limits,
            active: Semaphore::new(limits.active),
            attempts: Arc::new(Semaphore::new(limits.attempts)),
            deadline: config.limits.request,
        })))
    }
    /// Borrows original codec custody without copying or transmitting its key.
    /// A new attempt is NOT an idempotent retry of any uncertain earlier POST.
    pub fn prepare<'a>(&self, media: &'a Encrypted) -> Result<UploadAttempt<'a>, MediaUploadError> {
        if media.ciphertext().len() > self.0.limits.bytes {
            return Err(Error::BodyTooLarge.into());
        }
        let slot = self
            .0
            .attempts
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Busy)?;
        Ok(UploadAttempt {
            uploader: self.0.clone(),
            media,
            _slot: slot,
            state: UploadState::Prepared,
            observed: None,
            failure: None,
        })
    }
}
/// Caller-held outcome and borrowed exact ciphertext/descriptor. Dropping this
/// handle loses its in-memory outcome marker, never proves remote absence and
/// does not release or replace the caller's Encrypted object. No Debug/Serialize.
pub struct UploadAttempt<'a> {
    uploader: Arc<Inner>,
    media: &'a Encrypted,
    _slot: OwnedSemaphorePermit,
    state: UploadState,
    observed: Option<UploadResponse>,
    failure: Option<Error>,
}
impl UploadAttempt<'_> {
    pub fn state(&self) -> UploadState {
        self.state
    }
    pub fn failure(&self) -> Option<Error> {
        self.failure
    }
    /// Stored under this attempt's finite slot; no separately unbounded receipt.
    pub fn media_id(&self) -> Option<&MediaId> {
        if self.state == UploadState::Accepted {
            self.observed.as_ref().map(UploadResponse::media_id)
        } else {
            None
        }
    }
    /// Complete checked HTTP response under the original finite attempt slot.
    /// This may remain after the final cancellation/deadline check refused
    /// current success. It is historical evidence, never permission to resend,
    /// publish a room event or treat a cancelled task as successful.
    pub fn observed_response(&self) -> Option<&UploadResponse> {
        self.observed.as_ref()
    }
    pub async fn send(&mut self, cancel: &CancellationToken) -> Result<(), MediaUploadError> {
        if self.state != UploadState::Prepared {
            return Err(MediaUploadError::Terminal);
        }
        if cancel.is_cancelled() {
            return Err(Error::Cancelled.into());
        }
        let inner = self.uploader.clone();
        let deadline = Instant::now() + inner.deadline;
        let _active = inner.active.try_acquire().map_err(|_| Error::Busy)?;
        // Both permits precede this bounded HTTP-owned copy. The original codec
        // object remains borrowed and unchanged through request cancellation.
        let request = inner
            .http
            .prepare_upload(self.media.ciphertext(), inner.limits.bytes)?;
        self.state = UploadState::WritePossible;
        // Must precede first HTTP polling. Any future drop leaves the marker;
        // no Drop handler resets it or pretends the server received no bytes.
        match inner.http.upload(request, deadline, cancel).await {
            Ok(response) => {
                // Preserve actual observed bytes before checking current success.
                // No await separates ownership transfer from the final check.
                self.observed = Some(response);
                // Bounded synchronous JSON/MXC work is not hard-cancellable.
                // Recheck at the actual acceptance boundary, including a
                // scheduler delay after the final network readiness event.
                let refusal = if cancel.is_cancelled() {
                    Some(Error::Cancelled)
                } else if Instant::now() >= deadline {
                    Some(Error::Timeout)
                } else {
                    None
                };
                if let Some(error) = refusal {
                    self.failure = Some(error);
                    return Err(error.into());
                }
                self.state = UploadState::Accepted;
                Ok(())
            }
            Err(error) => {
                self.failure = Some(error);
                Err(error.into())
            }
        }
    }
}
