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
    client: Client,
    base: Url,
    limits: Limits,
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
        let body = wire::json(&bytes)?;
        let object = body
            .as_object()
            .filter(|v| v.len() == 1)
            .ok_or(Error::Wire)?;
        let mxc = object
            .get("content_uri")
            .and_then(Value::as_str)
            .ok_or(Error::Wire)?;
        let media_id = crate::MediaId::new(mxc).map_err(|_| Error::Wire)?;
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
        let mut headers = HeaderMap::new();
        headers.insert(header::AUTHORIZATION, config.authorization.clone());
        headers.insert(header::ACCEPT, HeaderValue::from_static("application/json"));
        headers.insert(
            header::ACCEPT_ENCODING,
            HeaderValue::from_static("identity"),
        );
        headers.insert(header::CONNECTION, HeaderValue::from_static("close"));
        let mut builder = Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .referer(false)
            .http1_only()
            .pool_max_idle_per_host(0)
            .connect_timeout(config.limits.connect)
            .default_headers(headers)
            .dns_resolver(Arc::new(Resolver {
                host: config.endpoint.host_str().ok_or(Error::Config)?.into(),
                permits: Arc::new(Semaphore::new(3)),
            }));
        for root in &config.roots {
            builder = builder.add_root_certificate(root.clone());
        }
        Ok(Self {
            client: builder.build().map_err(|_| Error::Config)?,
            base: config.endpoint.clone(),
            limits: config.limits.clone(),
        })
    }

    pub(crate) async fn request(
        &self,
        segments: &[&str],
        query: Option<&[(&str, &str)]>,
        cancel: &CancellationToken,
    ) -> Result<Response, Error> {
        self.perform(reqwest::Method::GET, segments, query, None, cancel)
            .await
    }
    pub(crate) async fn post(
        &self,
        segments: &[&str],
        body: String,
        cancel: &CancellationToken,
    ) -> Result<Response, Error> {
        self.perform(reqwest::Method::POST, segments, None, Some(body), cancel)
            .await
    }
    pub(crate) async fn put(
        &self,
        segments: &[&str],
        body: String,
        cancel: &CancellationToken,
    ) -> Result<Response, Error> {
        self.perform(reqwest::Method::PUT, segments, None, Some(body), cancel)
            .await
    }
    async fn perform(
        &self,
        method: reqwest::Method,
        segments: &[&str],
        query: Option<&[(&str, &str)]>,
        body: Option<String>,
        cancel: &CancellationToken,
    ) -> Result<Response, Error> {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let mut url = self.base.clone();
        url.path_segments_mut()
            .map_err(|_| Error::Config)?
            .clear()
            .extend(segments);
        if let Some(query) = query {
            url.query_pairs_mut().extend_pairs(query.iter().copied());
        }
        let mut request = self.client.request(method, url);
        if let Some(body) = body {
            if body.len() > self.limits.bytes {
                return Err(Error::BodyTooLarge);
            }
            request = request
                .header(header::CONTENT_TYPE, "application/json")
                .body(body);
        }
        let begin = Instant::now();
        let deadline = begin + self.limits.request;
        let mut response = wait(cancel, begin + self.limits.headers, request.send())
            .await?
            .map_err(|_| Error::Transport)?;
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
            return Err(Error::Headers);
        }
        if (300..400).contains(&status) {
            return Err(Error::Redirect);
        }
        let json_type = headers
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(';').next())
            .is_some_and(|v| v.trim() == "application/json");
        if status == 200 && !json_type {
            return Err(Error::Headers);
        }
        let cap = self.limits.bytes;
        if response.content_length().is_some_and(|n| n > cap as u64) {
            return Err(Error::BodyTooLarge);
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
                return Err(Error::BodyTooLarge);
            }
            bytes.extend_from_slice(&chunk);
        }
        let value = if json_type {
            match wire::json(&bytes) {
                Ok(v) => Some(v),
                Err(error) if status == 200 => return Err(error),
                Err(_) => None,
            }
        } else {
            None
        };
        Ok(Response { status, value })
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
