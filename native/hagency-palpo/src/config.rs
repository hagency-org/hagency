use crate::Error;
use hagency_core::{JSON_SAFE_MAX, canonical};
use hagency_store::outbound::{Activation, RegistrationIdentity};
use reqwest::{Url, header::HeaderValue};
use std::{net::IpAddr, time::Duration};

/// Finite budgets, validated before any durable activation or network access.
#[derive(Clone)]
pub struct Limits {
    pub poll_wait: Duration,
    pub connect: Duration,
    pub headers: Duration,
    pub request: Duration,
    pub body_idle: Duration,
    pub poll_bytes: usize,
    pub retry_min: Duration,
    pub retry_max: Duration,
    pub idle: Duration,
    pub publication_interval: Duration,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            poll_wait: Duration::from_secs(25),
            connect: Duration::from_secs(5),
            headers: Duration::from_secs(5),
            request: Duration::from_secs(10),
            body_idle: Duration::from_secs(5),
            poll_bytes: 4 * 1024 * 1024 + 16384,
            retry_min: Duration::from_secs(1),
            retry_max: Duration::from_secs(30),
            idle: Duration::from_millis(100),
            publication_interval: Duration::from_secs(15),
        }
    }
}
impl Limits {
    pub(crate) fn validate(&self) -> Result<(), Error> {
        let min = Duration::from_millis(10);
        let max = Duration::from_secs(60);
        if self.poll_wait > Duration::from_secs(25)
            || !(256..=4 * 1024 * 1024 + 16384).contains(&self.poll_bytes)
            || self.headers > self.request
            || self.connect > self.request
            || self.body_idle > self.request
            || self.retry_min > self.retry_max
            || [
                self.connect,
                self.headers,
                self.request,
                self.body_idle,
                self.retry_min,
                self.retry_max,
                self.idle,
                self.publication_interval,
            ]
            .iter()
            .any(|d| !(min..=max).contains(d))
        {
            return Err(Error::Config);
        }
        Ok(())
    }
}

/// Host-only configuration. Never deserialize this from a browser or fixture
/// intake. No Debug projection, mutable token setter, URL credentials or proxies.
pub struct HostConfig {
    pub(crate) endpoint: Url,
    pub(crate) authorization: HeaderValue,
    pub(crate) activation: Activation,
    pub(crate) limits: Limits,
    pub(crate) roots: Vec<reqwest::Certificate>,
}
impl HostConfig {
    pub fn new(
        registration: RegistrationIdentity,
        endpoint: &str,
        machine_token: &str,
        machine_generation: u64,
        limits: Limits,
    ) -> Result<Self, Error> {
        limits.validate()?;
        let fleet = &registration.fleet_id;
        if fleet.len() != 35
            || !fleet.starts_with("hf_")
            || !fleet[3..]
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || machine_generation == 0
            || machine_generation > JSON_SAFE_MAX
            || !(16..=4096).contains(&machine_token.len())
            || !machine_token.bytes().all(|b| (33..=126).contains(&b))
            || endpoint.len() > 2048
        {
            return Err(Error::Config);
        }
        let url = Url::parse(endpoint).map_err(|_| Error::Config)?;
        // Check the original spelling too: URL parsers normalize dot segments,
        // backslashes, whitespace and empty userinfo before these accessors.
        let path = format!("/api/fleet/v2/{fleet}");
        if !endpoint.ends_with(&path)
            || endpoint.bytes().any(|b| b <= 32 || b == b'\\')
            || endpoint.contains('@')
            || endpoint.contains('%')
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || url.path() != path
            || url.port() == Some(0)
            || url.host_str().is_none()
            || !matches!(url.scheme(), "https" | "http")
            || url.as_str() != endpoint
        {
            return Err(Error::Config);
        }
        if url.scheme() == "http" {
            let host = url
                .host_str()
                .ok_or(Error::Config)?
                .trim_matches(['[', ']']);
            if !host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback()) {
                return Err(Error::Config);
            }
        }
        let mut authorization =
            HeaderValue::from_str(&format!("Bearer {machine_token}")).map_err(|_| Error::Config)?;
        authorization.set_sensitive(true);
        let credential_fingerprint = canonical::transport_digest(
            &serde_json::json!({"endpoint":endpoint,"credential":machine_token}),
        )
        .map_err(|_| Error::Config)?;
        Ok(Self {
            endpoint: url,
            authorization,
            activation: Activation {
                registration,
                machine_generation,
                credential_fingerprint,
            },
            limits,
            roots: Vec::new(),
        })
    }

    /// Explicit host trust anchor, useful for an organization's private CA.
    /// Verification of certificate chain and hostname is always enabled.
    pub fn with_root_pem(mut self, pem: &[u8]) -> Result<Self, Error> {
        if pem.len() > 16384 || self.roots.len() >= 4 {
            return Err(Error::Config);
        }
        self.roots
            .push(reqwest::Certificate::from_pem(pem).map_err(|_| Error::Config)?);
        Ok(self)
    }
}
