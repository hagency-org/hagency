use crate::{Error, HostConfig, Limits, wire};
use reqwest::{
    Client, Url,
    header::{self, HeaderMap, HeaderValue},
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{future::Future, io, net::ToSocketAddrs, sync::Arc};
use tokio::{
    sync::Semaphore,
    time::{Instant, timeout_at},
};
use tokio_util::sync::CancellationToken;

/// OS DNS cannot always be interrupted. A permit lives in the blocking job,
/// so cancelled lookups cannot accumulate unlimited abandoned resolver work.
struct Resolver {
    host: String,
    permits: Arc<Semaphore>,
}
impl reqwest::dns::Resolve for Resolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let allowed = name.as_str() == self.host;
        let host = self.host.clone();
        let permit = self.permits.clone().try_acquire_owned();
        Box::pin(async move {
            let denied = || io::Error::other("bounded host resolver unavailable");
            if !allowed {
                return Err(denied().into());
            }
            let permit = permit.map_err(|_| denied())?;
            let addresses = tokio::task::spawn_blocking(move || {
                let _permit = permit;
                (host.as_str(), 0)
                    .to_socket_addrs()
                    .map(|addresses| addresses.take(16).collect::<Vec<_>>())
                    .map_err(|_| denied())
            })
            .await
            .map_err(|_| denied())??;
            Ok(Box::new(addresses.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

pub(crate) struct Http {
    /// Writes, uploads and downloads: a fresh connection each, closed after use.
    client: Client,
    /// JSON GETs only: keep-alive reuse (ADR174 amendment). A write never rides
    /// a reused connection, so a stale one cannot make a write uncertain; a
    /// write whose dial never connected is redialled, never re-sent.
    reader: Client,
    base: Url,
    limits: Limits,
}
/// First wait before redialling a JSON request whose connection never existed;
/// doubled per attempt inside the original request deadline.
const CONNECT_RETRY: std::time::Duration = std::time::Duration::from_millis(100);
/// How one attempt ended, for the only caller allowed to repeat it.
enum Failed {
    /// The dial failed before a connection existed: no request byte left.
    Connect,
    Other(Error),
}
impl From<Error> for Failed {
    fn from(error: Error) -> Self {
        Self::Other(error)
    }
}
impl From<Failed> for Error {
    fn from(failed: Failed) -> Self {
        match failed {
            Failed::Connect => Self::Transport,
            Failed::Other(error) => error,
        }
    }
}
/// reqwest also calls a TLS failure "connect", but an endpoint that fails
/// verification is refused, never redialled. tokio-rustls reports those as an
/// InvalidData io::Error, wrapped again by the connector; io::Error::source()
/// skips a wrapped error, so descend through get_ref() as well. Timeouts,
/// refusals, resets and resolution failures remain.
fn connect_phase(error: &reqwest::Error) -> bool {
    fn verification(error: &(dyn std::error::Error + 'static)) -> bool {
        if let Some(io) = error.downcast_ref::<std::io::Error>()
            && (io.kind() == std::io::ErrorKind::InvalidData
                || io.get_ref().is_some_and(|inner| verification(inner)))
        {
            return true;
        }
        error.source().is_some_and(verification)
    }
    error.is_connect() && !verification(error)
}
/// Host-owned shared cadence, independent of credentials and Matrix authority.
/// Keep one Arc across the fleet; no process-global endpoint registry or timers.
pub struct RequestPacing {
    endpoint: Url,
    interval: std::time::Duration,
    next: tokio::sync::Mutex<Option<Instant>>,
}
impl RequestPacing {
    pub fn new(endpoint: &str, interval: std::time::Duration) -> Result<Self, Error> {
        if !(std::time::Duration::from_millis(10)..=std::time::Duration::from_secs(1))
            .contains(&interval)
        {
            return Err(Error::Config);
        }
        Ok(Self {
            endpoint: crate::config::host_endpoint(endpoint)?,
            interval,
            next: tokio::sync::Mutex::new(None),
        })
    }
    pub(crate) fn check_endpoint(&self, endpoint: &Url) -> Result<(), Error> {
        if self.endpoint == *endpoint {
            Ok(())
        } else {
            Err(Error::Config)
        }
    }
    async fn enter(&self, cancel: &CancellationToken, deadline: Instant) -> Result<(), Error> {
        let mut next = wait(cancel, deadline, self.next.lock()).await?;
        if let Some(at) = *next {
            wait(cancel, deadline, tokio::time::sleep_until(at)).await?;
        }
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let now = Instant::now();
        if now >= deadline {
            return Err(Error::Timeout);
        }
        *next = Some(now + self.interval);
        Ok(())
    }
}
/// Exact complete body of a validated HTTP200 encrypted-upload response. Only
/// the actual bounded transport constructs this value. It carries no room,
/// dispatch, sender verification, persistence or current-execution authority.
/// The original UploadAttempt or staged upload owner retains its finite result slot.
pub struct UploadResponse {
    body: Vec<u8>,
    body_sha256: [u8; 32],
    media_id: crate::MediaId,
}
impl UploadResponse {
    /// One content identity plus Palpo's optional non-authoritative metadata.
    /// Use the same strict grammar for original HTTP and protected SDK reopen.
    pub(crate) fn parse_media_id(bytes: &[u8]) -> Result<crate::MediaId, Error> {
        if bytes.len() > 4096 {
            return Err(Error::BodyTooLarge);
        }
        let body = wire::json(bytes)?;
        let object = body.as_object().ok_or(Error::Wire)?;
        if object
            .keys()
            .any(|key| key != "content_uri" && key != "blurhash")
            || object
                .get("blurhash")
                .is_some_and(|value| !value.is_null() && !value.is_string())
        {
            return Err(Error::Wire);
        }
        let mxc = object
            .get("content_uri")
            .and_then(Value::as_str)
            .ok_or(Error::Wire)?;
        crate::MediaId::new(mxc).map_err(|_| Error::Wire)
    }

    pub fn body(&self) -> &[u8] {
        &self.body
    }
    /// SHA256 of original BODY bytes, not headers, status or a JSON re-encoding.
    pub fn body_sha256(&self) -> &[u8; 32] {
        &self.body_sha256
    }
    pub fn media_id(&self) -> &crate::MediaId {
        &self.media_id
    }
}

pub(crate) struct Response {
    pub status: u16,
    pub value: Option<Value>,
    retry_delay: Option<std::time::Duration>,
}
impl Response {
    pub fn success(self) -> Result<Value, Error> {
        match self.status {
            200 => self.value.ok_or(Error::InvalidJson),
            300..=399 => Err(Error::Redirect),
            401 | 403 => Err(Error::Unauthorized),
            status => Err(Error::Remote(status)),
        }
    }
}
impl Http {
    /// Build only the fixed encrypted upload path. The host already holds
    /// bounded attempt/transfer custody; this method performs no network I/O.
    pub(crate) fn prepare_upload(
        &self,
        ciphertext: &[u8],
        cap: usize,
    ) -> Result<reqwest::Request, Error> {
        if ciphertext.len() > cap {
            return Err(Error::BodyTooLarge);
        }
        let mut body = Vec::new();
        body.try_reserve_exact(ciphertext.len())
            .map_err(|_| Error::Capacity)?;
        body.extend_from_slice(ciphertext);
        let mut url = self.base.clone();
        url.path_segments_mut()
            .map_err(|_| Error::Config)?
            .clear()
            .extend(["_matrix", "media", "v3", "upload"]);
        self.client
            .post(url)
            .header(header::CONTENT_TYPE, "application/octet-stream")
            .header(header::CONTENT_LENGTH, body.len())
            .body(body)
            .build()
            .map_err(|_| Error::Config)
    }
    /// Caller marks WritePossible before polling this future. A failure cannot
    /// establish non-delivery; existing JSON request behavior remains unchanged.
    pub(crate) async fn upload(
        &self,
        request: reqwest::Request,
        deadline: Instant,
        cancel: &CancellationToken,
    ) -> Result<UploadResponse, Error> {
        const CAP: usize = 4096;
        self.pace(cancel, deadline).await?;
        let mut response = wait(
            cancel,
            deadline.min(Instant::now() + self.limits.headers),
            self.client.execute(request),
        )
        .await?
        .map_err(|_| Error::Transport)?;
        let headers = response.headers();
        if headers.len() > 64
            || headers
                .iter()
                .map(|(k, v)| k.as_str().len() + v.len())
                .sum::<usize>()
                > 16384
            || [
                header::CONTENT_LENGTH,
                header::TRANSFER_ENCODING,
                header::CONTENT_ENCODING,
                header::CONTENT_TYPE,
            ]
            .iter()
            .any(|name| headers.get_all(name).iter().count() > 1)
            || headers
                .get(header::CONTENT_ENCODING)
                .is_some_and(|v| v != "identity")
            || headers
                .get(header::TRANSFER_ENCODING)
                .is_some_and(|v| v != "chunked")
            || (headers.contains_key(header::CONTENT_LENGTH)
                && headers.contains_key(header::TRANSFER_ENCODING))
        {
            return Err(Error::Headers);
        }
        let declared = headers
            .get(header::CONTENT_LENGTH)
            .map(|v| {
                let value = v.to_str().map_err(|_| Error::Headers)?;
                if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(Error::Headers);
                }
                value.parse::<u64>().map_err(|_| Error::Headers)
            })
            .transpose()?;
        match response.status().as_u16() {
            200 => {}
            300..=399 => return Err(Error::Redirect),
            401 | 403 => return Err(Error::Unauthorized),
            status => return Err(Error::Remote(status)),
        }
        if headers
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(';').next())
            .is_none_or(|v| v.trim() != "application/json")
        {
            return Err(Error::Headers);
        }
        if declared.is_some_and(|n| n > CAP as u64) {
            return Err(Error::BodyTooLarge);
        }
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(CAP).map_err(|_| Error::Capacity)?;
        loop {
            let next = wait(
                cancel,
                deadline.min(Instant::now() + self.limits.body_idle),
                response.chunk(),
            )
            .await?
            .map_err(|_| Error::Transport)?;
            let Some(next) = next else {
                break;
            };
            if next.len() > CAP.saturating_sub(bytes.len()) {
                return Err(Error::BodyTooLarge);
            }
            bytes.extend_from_slice(&next);
        }
        if declared.is_some_and(|n| n != bytes.len() as u64) {
            return Err(Error::Transport);
        }
        let media_id = UploadResponse::parse_media_id(&bytes)?;
        let body_sha256 = Sha256::digest(&bytes).into();
        Ok(UploadResponse {
            body: bytes,
            body_sha256,
            media_id,
        })
    }
    /// Binary repository GET. JSON request/response behavior below is unchanged.
    /// The caller already holds a finite transfer permit and absolute deadline.
    pub(crate) async fn download(
        &self,
        segments: &[&str],
        cap: usize,
        deadline: Instant,
        cancel: &CancellationToken,
    ) -> Result<Vec<u8>, Error> {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let mut url = self.base.clone();
        url.path_segments_mut()
            .map_err(|_| Error::Config)?
            .clear()
            .extend(segments);
        self.pace(cancel, deadline).await?;
        let mut response = wait(
            cancel,
            deadline.min(Instant::now() + self.limits.headers),
            self.client
                .get(url)
                .header(header::ACCEPT, "application/octet-stream")
                .send(),
        )
        .await?
        .map_err(|_| Error::Transport)?;
        let headers = response.headers();
        if headers.len() > 64
            || headers
                .iter()
                .map(|(k, v)| k.as_str().len() + v.len())
                .sum::<usize>()
                > 16384
            || [
                header::CONTENT_LENGTH,
                header::TRANSFER_ENCODING,
                header::CONTENT_ENCODING,
                header::CONTENT_TYPE,
            ]
            .iter()
            .any(|name| headers.get_all(name).iter().count() > 1)
            || headers
                .get(header::CONTENT_ENCODING)
                .is_some_and(|v| v != "identity")
            || headers
                .get(header::TRANSFER_ENCODING)
                .is_some_and(|v| v != "chunked")
            || (headers.contains_key(header::CONTENT_LENGTH)
                && headers.contains_key(header::TRANSFER_ENCODING))
        {
            return Err(Error::Headers);
        }
        let declared = headers
            .get(header::CONTENT_LENGTH)
            .map(|v| {
                let text = v.to_str().map_err(|_| Error::Headers)?;
                if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(Error::Headers);
                }
                text.parse::<u64>().map_err(|_| Error::Headers)
            })
            .transpose()?;
        match response.status().as_u16() {
            200 => {}
            300..=399 => return Err(Error::Redirect),
            401 | 403 => return Err(Error::Unauthorized),
            status => return Err(Error::Remote(status)),
        }
        if declared.is_some_and(|n| n > cap as u64) {
            return Err(Error::BodyTooLarge);
        }
        let mut bytes = Vec::new();
        // Request checked capacity without geometric Vec growth. Allocator
        // rounding/overhead is separate from the enforced logical byte cap.
        bytes.try_reserve_exact(cap).map_err(|_| Error::Capacity)?;
        loop {
            let chunk = wait(
                cancel,
                deadline.min(Instant::now() + self.limits.body_idle),
                response.chunk(),
            )
            .await?
            .map_err(|_| Error::Transport)?;
            let Some(chunk) = chunk else {
                break;
            };
            if chunk.len() > cap.saturating_sub(bytes.len()) {
                return Err(Error::BodyTooLarge);
            }
            bytes.extend_from_slice(&chunk);
        }
        if declared.is_some_and(|n| n != bytes.len() as u64) {
            return Err(Error::Transport);
        }
        Ok(bytes)
    }

    pub(crate) fn new(config: &HostConfig) -> Result<Self, Error> {
        Self::for_host(
            &config.endpoint,
            Some(&config.authorization),
            &config.limits,
            &config.roots,
        )
    }
    /// Crate-private construction for the fixed account provisioner. There is
    /// deliberately no public mutable credential/endpoint selector.
    pub(crate) fn for_host(
        endpoint: &Url,
        authorization: Option<&HeaderValue>,
        limits: &Limits,
        roots: &[reqwest::Certificate],
    ) -> Result<Self, Error> {
        if let Some(pacing) = &limits.request_pacing {
            pacing.check_endpoint(endpoint)?;
        }
        let mut headers = HeaderMap::new();
        if let Some(authorization) = authorization {
            headers.insert(header::AUTHORIZATION, authorization.clone());
        }
        headers.insert(header::ACCEPT, HeaderValue::from_static("application/json"));
        headers.insert(
            header::ACCEPT_ENCODING,
            HeaderValue::from_static("identity"),
        );
        let host: String = endpoint.host_str().ok_or(Error::Config)?.into();
        let build = |reuse: bool| {
            let mut headers = headers.clone();
            if !reuse {
                headers.insert(header::CONNECTION, HeaderValue::from_static("close"));
            }
            let mut builder = Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .retry(reqwest::retry::never())
                .referer(false)
                .http1_only()
                .pool_max_idle_per_host(if reuse { 2 } else { 0 })
                // Far below any server's idle close, so a reused connection
                // is one the peer still holds open.
                .pool_idle_timeout(std::time::Duration::from_secs(10))
                .connect_timeout(limits.connect)
                .default_headers(headers)
                .dns_resolver(Arc::new(Resolver {
                    host: host.clone(),
                    permits: Arc::new(Semaphore::new(3)),
                }));
            for root in roots {
                builder = builder.add_root_certificate(root.clone());
            }
            builder.build().map_err(|_| Error::Config)
        };
        Ok(Self {
            client: build(false)?,
            reader: build(true)?,
            base: endpoint.clone(),
            limits: limits.clone(),
        })
    }

    pub(crate) async fn request(
        &self,
        segments: &[&str],
        query: Option<&[(&str, &str)]>,
        cancel: &CancellationToken,
    ) -> Result<Response, Error> {
        let deadline = Instant::now() + self.limits.request;
        for attempt in 0..4 {
            // One budget of four attempts, whether the last one ended in a
            // complete 429 or never reached the peer at all.
            let outcome = match self
                .perform_once(
                    reqwest::Method::GET,
                    segments,
                    query,
                    None,
                    cancel,
                    deadline,
                )
                .await
            {
                Ok(response) if response.status != 429 => return Ok(response),
                Ok(response) => Ok(response),
                Err(Failed::Connect) => Err(Error::Transport),
                Err(Failed::Other(error)) => return Err(error),
            };
            let delay = match &outcome {
                Ok(response) => response.retry_delay,
                Err(_) => Some(CONNECT_RETRY),
            }
            .and_then(|delay| delay.checked_mul(1 << attempt));
            let next = delay
                .filter(|_| attempt < 3)
                .and_then(|delay| Instant::now().checked_add(delay))
                .filter(|next| *next < deadline);
            let Some(next) = next else {
                return outcome;
            };
            wait(cancel, deadline, tokio::time::sleep_until(next)).await?;
        }
        unreachable!("finite GET loop always returns its last outcome")
    }
    pub(crate) async fn post(
        &self,
        segments: &[&str],
        body: String,
        cancel: &CancellationToken,
    ) -> Result<Response, Error> {
        self.perform(
            reqwest::Method::POST,
            segments,
            None,
            Some(body),
            cancel,
            Instant::now() + self.limits.request,
        )
        .await
    }
    pub(crate) async fn put(
        &self,
        segments: &[&str],
        body: String,
        cancel: &CancellationToken,
    ) -> Result<Response, Error> {
        self.perform(
            reqwest::Method::PUT,
            segments,
            None,
            Some(body),
            cancel,
            Instant::now() + self.limits.request,
        )
        .await
    }
    /// A JSON write is sent at most once. Only a dial that failed before any
    /// connection existed is repeated: no byte of the request left, so there is
    /// nothing to send twice. Any other failure, and a complete 429, ends it.
    async fn perform(
        &self,
        method: reqwest::Method,
        segments: &[&str],
        query: Option<&[(&str, &str)]>,
        body: Option<String>,
        cancel: &CancellationToken,
        deadline: Instant,
    ) -> Result<Response, Error> {
        for attempt in 0..4 {
            match self
                .perform_once(
                    method.clone(),
                    segments,
                    query,
                    body.clone(),
                    cancel,
                    deadline,
                )
                .await
            {
                Err(Failed::Connect) => {}
                outcome => return Ok(outcome?),
            }
            let next = CONNECT_RETRY
                .checked_mul(1 << attempt)
                .filter(|_| attempt < 3)
                .and_then(|delay| Instant::now().checked_add(delay))
                .filter(|next| *next < deadline);
            let Some(next) = next else {
                return Err(Error::Transport);
            };
            wait(cancel, deadline, tokio::time::sleep_until(next)).await?;
        }
        unreachable!("finite write loop always returns its last outcome")
    }
    async fn perform_once(
        &self,
        method: reqwest::Method,
        segments: &[&str],
        query: Option<&[(&str, &str)]>,
        body: Option<String>,
        cancel: &CancellationToken,
        deadline: Instant,
    ) -> Result<Response, Failed> {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled.into());
        }
        let mut url = self.base.clone();
        url.path_segments_mut()
            .map_err(|_| Error::Config)?
            .clear()
            .extend(segments);
        if let Some(query) = query {
            url.query_pairs_mut().extend_pairs(query.iter().copied());
        }
        let client = if method == reqwest::Method::GET {
            &self.reader
        } else {
            &self.client
        };
        let mut request = client.request(method, url);
        if let Some(body) = body {
            if body.len() > self.limits.bytes {
                return Err(Error::BodyTooLarge.into());
            }
            request = request
                .header(header::CONTENT_TYPE, "application/json")
                .body(body);
        }
        self.pace(cancel, deadline).await?;
        let mut response = wait(
            cancel,
            deadline.min(Instant::now() + self.limits.headers),
            request.send(),
        )
        .await?
        .map_err(|error| {
            if connect_phase(&error) {
                Failed::Connect
            } else {
                Failed::Other(Error::Transport)
            }
        })?;
        let status = response.status().as_u16();
        // Hyper 1.11's HTTP/1 parser itself has a 417792-byte header buffer cap.
        // The accepted header projection here is stricter, 16 KiB / 64 fields.
        let headers = response.headers();
        if headers.len() > 64
            || headers
                .iter()
                .map(|(k, v)| k.as_str().len() + v.len())
                .sum::<usize>()
                > 16384
            || headers
                .get_all(header::CONTENT_ENCODING)
                .iter()
                .any(|v| v != "identity")
            || headers.get_all(header::CONTENT_TYPE).iter().count() > 1
        {
            return Err(Error::Headers.into());
        }
        if (300..400).contains(&status) {
            return Err(Error::Redirect.into());
        }
        let json_type = headers
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(';').next())
            .is_some_and(|v| v.trim() == "application/json");
        if status == 200 && !json_type {
            return Err(Error::Headers.into());
        }
        let retry_header = if status == 429 {
            Some(
                headers
                    .get_all(header::RETRY_AFTER)
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>(),
            )
        } else {
            None
        };
        let cap = self.limits.bytes;
        if response.content_length().is_some_and(|n| n > cap as u64) {
            return Err(Error::BodyTooLarge.into());
        }
        let mut bytes = Vec::new();
        loop {
            let next = wait(
                cancel,
                deadline.min(Instant::now() + self.limits.body_idle),
                response.chunk(),
            )
            .await?
            .map_err(|_| Error::Transport)?;
            let Some(chunk) = next else {
                break;
            };
            if chunk.len() > cap.saturating_sub(bytes.len()) {
                return Err(Error::BodyTooLarge.into());
            }
            bytes.extend_from_slice(&chunk);
        }
        let value = if json_type {
            match wire::json(&bytes) {
                Ok(v) => Some(v),
                Err(error) if status == 200 => return Err(error.into()),
                Err(_) => None,
            }
        } else {
            None
        };
        let retry_delay = retry_header
            .as_ref()
            .and_then(|headers| read_retry_delay(headers, value.as_ref()));
        Ok(Response {
            status,
            value,
            retry_delay,
        })
    }
    async fn pace(&self, cancel: &CancellationToken, deadline: Instant) -> Result<(), Error> {
        if let Some(pacing) = &self.limits.request_pacing {
            pacing.enter(cancel, deadline).await?;
        }
        Ok(())
    }
}
async fn wait<T>(
    cancel: &CancellationToken,
    deadline: Instant,
    future: impl Future<Output = T>,
) -> Result<T, Error> {
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(Error::Cancelled),
        result = timeout_at(deadline, future) => result.map_err(|_| Error::Timeout),
    }
}

fn read_retry_delay(headers: &[HeaderValue], value: Option<&Value>) -> Option<std::time::Duration> {
    use std::time::Duration;
    if headers.len() > 1 {
        return None;
    }
    let seconds = if let Some(value) = headers.first() {
        let value = value.to_str().ok()?;
        if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        Some(value.parse::<u64>().ok()?)
    } else {
        None
    };
    let milliseconds = value
        .and_then(|value| value.get("retry_after_ms"))
        .map(Value::as_u64);
    if milliseconds == Some(None) {
        return None;
    }
    if seconds.is_none() && milliseconds.is_none() {
        return Some(Duration::from_secs(1));
    }
    Some(
        Duration::from_secs(seconds.unwrap_or(0))
            .max(Duration::from_millis(milliseconds.flatten().unwrap_or(0)))
            .max(Duration::from_millis(10)),
    )
}

#[cfg(test)]
#[path = "http/rate_limit_tests.rs"]
mod rate_limit_tests;

#[cfg(test)]
#[path = "http/pacing_tests.rs"]
mod pacing_tests;

#[cfg(test)]
#[path = "http/connect_retry_tests.rs"]
mod connect_retry_tests;
