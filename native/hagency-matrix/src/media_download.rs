//! Authenticated repository transport only. The host must separately establish
//! descriptor/event provenance and current dispatch visibility before exposure.
use crate::{Error, HostConfig, http::Http};
use hagency_media::{CheckedBytes, Codec, Descriptor};
use std::sync::Arc;
use tokio::{sync::Semaphore, time::Instant};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum MediaDownloadError {
    #[error("invalid bounded Matrix media identifier")]
    MediaId,
    #[error("invalid Matrix media download configuration")]
    Config,
    #[error(transparent)]
    Transport(#[from] Error),
    #[error(transparent)]
    Crypto(#[from] hagency_media::Error),
}

/// Repository path only, not an HTTP URL or evidence of an authenticated event.
/// No Debug/Deserialize or setters; callers retain the original host input.
pub struct MediaId {
    server: String,
    media: String,
}
impl MediaId {
    /// Bounded repository identity only; never an HTTP destination or event proof.
    pub fn to_mxc(&self) -> String {
        format!("mxc://{}/{}", self.server, self.media)
    }
    pub fn new(mxc: &str) -> Result<Self, MediaDownloadError> {
        let invalid = || MediaDownloadError::MediaId;
        if mxc.len() > 517 || !mxc.is_ascii() {
            return Err(invalid());
        }
        let (server, media) = mxc
            .strip_prefix("mxc://")
            .and_then(|s| s.split_once('/'))
            .ok_or_else(invalid)?;
        if server.is_empty()
            || server.len() > 255
            || matches!(server, "." | "..")
            || server
                .bytes()
                .any(|b| b <= 32 || matches!(b, b'%' | b'/' | b'\\' | b'?' | b'#' | b'@'))
            || media.is_empty()
            || media.len() > 255
            || !media
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
        {
            return Err(invalid());
        }
        ruma::ServerName::parse(server).map_err(|_| invalid())?;
        // Validate components directly: pinned ruma's MxcUri validator allows
        // an empty media ID and stores the slash offset in a u8. Do not let a
        // long server trigger its narrowing cast/NonZeroU8 unwrap.
        Ok(Self {
            server: server.into(),
            media: media.into(),
        })
    }
}

#[derive(Clone, Copy)]
pub struct MediaDownloadLimits {
    bytes: usize,
    transfers: usize,
    results: usize,
}
impl MediaDownloadLimits {
    pub fn new(bytes: usize, transfers: usize, results: usize) -> Result<Self, MediaDownloadError> {
        if !(1..=16 * 1024 * 1024).contains(&bytes)
            || !(1..=4).contains(&transfers)
            || !(1..=8).contains(&results)
        {
            return Err(MediaDownloadError::Config);
        }
        Ok(Self {
            bytes,
            transfers,
            results,
        })
    }
}
impl Default for MediaDownloadLimits {
    fn default() -> Self {
        Self {
            bytes: 4 * 1024 * 1024,
            transfers: 2,
            results: 4,
        }
    }
}

struct Inner {
    http: Http,
    codec: Codec,
    active: Semaphore,
    limits: MediaDownloadLimits,
    deadline: std::time::Duration,
}
/// Clones share actual transfer and codec capacity. No SDK, file, domain, room
/// observation or service is created. HostConfig supplies HTTPS and bearer only;
/// this primitive does not assert the credential's user/device via whoami.
#[derive(Clone)]
pub struct MediaDownloader(Arc<Inner>);
impl MediaDownloader {
    pub fn new(
        config: &HostConfig,
        limits: MediaDownloadLimits,
    ) -> Result<Self, MediaDownloadError> {
        if config.endpoint.scheme() != "https" {
            return Err(MediaDownloadError::Config);
        }
        let codec = Codec::new(hagency_media::Limits::new(limits.bytes, limits.results)?);
        Ok(Self(Arc::new(Inner {
            http: Http::new(config)?,
            codec,
            active: Semaphore::new(limits.transfers),
            limits,
            deadline: config.limits.request,
        })))
    }
    /// The caller retains descriptor custody on all outcomes. Network errors
    /// discard incomplete ciphertext, never yielding a partially checked object.
    /// A successful CheckedBytes can be handed to the independent media store.
    pub async fn download(
        &self,
        id: &MediaId,
        descriptor: &Descriptor,
        cancel: &CancellationToken,
    ) -> Result<CheckedBytes, MediaDownloadError> {
        let deadline = Instant::now() + self.0.deadline;
        self.download_until(id, descriptor, cancel, deadline).await
    }
    /// The host coordinator may narrow the absolute deadline, never renew it.
    pub(crate) async fn download_until(
        &self,
        id: &MediaId,
        descriptor: &Descriptor,
        cancel: &CancellationToken,
        deadline: Instant,
    ) -> Result<CheckedBytes, MediaDownloadError> {
        self.download_bounded_until(id, descriptor, cancel, deadline, self.0.limits.bytes)
            .await
    }
    /// A per-operation limit can only reduce the shared transport/codec bound.
    pub(crate) async fn download_bounded_until(
        &self,
        id: &MediaId,
        descriptor: &Descriptor,
        cancel: &CancellationToken,
        deadline: Instant,
        max_bytes: usize,
    ) -> Result<CheckedBytes, MediaDownloadError> {
        if max_bytes == 0 || max_bytes > self.0.limits.bytes {
            return Err(MediaDownloadError::Config);
        }
        let deadline = deadline.min(Instant::now() + self.0.deadline);
        checkpoint(cancel, deadline)?;
        // No waiting queue, and this permit owns the whole response buffer until
        // decryption returns. Future drop releases IO/buffer/permit together.
        let _permit = self.0.active.try_acquire().map_err(|_| Error::Busy)?;
        let bytes = self
            .0
            .http
            .download(
                &[
                    "_matrix", "client", "v1", "media", "download", &id.server, &id.media,
                ],
                max_bytes,
                deadline,
                cancel,
            )
            .await?;
        checkpoint(cancel, deadline)?;
        let checked = self.0.codec.decrypt(descriptor, &bytes)?;
        // Synchronous bounded SDK crypto cannot be hard-cancelled. An overrun
        // or observed cancellation refuses the result after CPU work completes.
        checkpoint(cancel, deadline)?;
        Ok(checked)
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
