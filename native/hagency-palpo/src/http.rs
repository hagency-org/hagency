use crate::{Error, HostConfig, Limits, wire};
use reqwest::{
    Client, Method, Url,
    header::{self, HeaderMap, HeaderValue},
};
use serde_json::Value;
use std::{future::Future, io, net::ToSocketAddrs, sync::Arc, time::Duration};
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
    pub(crate) fn new(config: &HostConfig) -> Result<Self, Error> {
        let mut headers = HeaderMap::new();
        headers.insert(header::AUTHORIZATION, config.authorization.clone());
        headers.insert(header::ACCEPT, HeaderValue::from_static("application/json"));
        headers.insert(
            header::ACCEPT_ENCODING,
            HeaderValue::from_static("identity"),
        );
        headers.insert(header::CONNECTION, HeaderValue::from_static("close"));
        headers.insert(
            "x-hagency-generation",
            HeaderValue::from_str(&config.activation.machine_generation.to_string())
                .map_err(|_| Error::Config)?,
        );
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
        suffix: &'static str,
        query: Option<&[(&str, &str)]>,
        body: Option<String>,
        cancel: &CancellationToken,
    ) -> Result<Response, Error> {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let poll = suffix == "poll";
        if !matches!(suffix, "poll" | "ack" | "updates")
            || body.as_ref().is_some_and(|b| b.len() > 1024 * 1024)
        {
            return Err(Error::Config);
        }
        let mut url = self.base.clone();
        url.set_path(&format!("{}/{suffix}", self.base.path()));
        if let Some(query) = query {
            url.query_pairs_mut().extend_pairs(query.iter().copied());
        }
        let mut request = self
            .client
            .request(if poll { Method::GET } else { Method::POST }, url);
        if let Some(body) = body {
            request = request
                .header(header::CONTENT_TYPE, "application/json")
                .body(body);
        }
        let extra = if poll {
            self.limits.poll_wait
        } else {
            Duration::ZERO
        };
        let begin = Instant::now();
        let deadline = begin + extra + self.limits.request;
        let mut response = wait(cancel, begin + extra + self.limits.headers, request.send())
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
        let cap = if poll {
            self.limits.poll_bytes
        } else {
            64 * 1024
        };
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
