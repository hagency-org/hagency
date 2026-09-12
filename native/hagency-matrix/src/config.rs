use crate::Error;
use hagency_core::{JSON_SAFE_MAX, canonical, replies::*};
use reqwest::{Url, header::HeaderValue};
use serde::Serialize;
use std::{collections::BTreeSet, net::IpAddr, path::PathBuf, time::Duration};

#[derive(Clone)]
pub struct Limits {
    pub connect: Duration,
    pub headers: Duration,
    pub request: Duration,
    pub body_idle: Duration,
    pub sdk: Duration,
    pub bytes: usize,
    pub events: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            connect: Duration::from_secs(5),
            headers: Duration::from_secs(5),
            request: Duration::from_secs(15),
            body_idle: Duration::from_secs(5),
            sdk: Duration::from_secs(20),
            bytes: 1024 * 1024,
            events: 1000,
        }
    }
}
impl Limits {
    fn validate(&self) -> Result<(), Error> {
        if [
            self.connect,
            self.headers,
            self.request,
            self.body_idle,
            self.sdk,
        ]
        .iter()
        .any(|d| !(Duration::from_millis(10)..=Duration::from_secs(60)).contains(d))
            || self.headers > self.request
            || self.connect > self.request
            || self.body_idle > self.request
            || !(256..=1024 * 1024).contains(&self.bytes)
            || !(1..=1000).contains(&self.events)
        {
            return Err(Error::Config);
        }
        Ok(())
    }
}
/// Host registration and Matrix account/device incarnation. Never a Palpo machine generation.
#[derive(Clone, Serialize)]
pub struct HostIdentity {
    pub server_name: String,
    pub registration_fingerprint: String,
    pub transport: MatrixTransportObservation,
}
/// Positive scope generation is coordinated by the host across all room members.
#[derive(Clone, Serialize)]
pub struct HostRoom {
    pub room_id: String,
    pub generation: u64,
    pub privacy: RoomPrivacy,
}
/// Constructed by the process host only. No Deserialize, Debug or credential setters.
pub struct HostConfig {
    pub(crate) enrollment: Option<crate::enrollment::state::Profile>,
    pub(crate) approval: bool,
    pub(crate) endpoint: Url,
    pub(crate) authorization: HeaderValue,
    pub(crate) identity: HostIdentity,
    pub(crate) rooms: Vec<HostRoom>,
    pub(crate) root: PathBuf,
    pub(crate) key: [u8; 32],
    pub(crate) limits: Limits,
    pub(crate) roots: Vec<reqwest::Certificate>,
}
impl HostConfig {
    pub fn new(
        identity: HostIdentity,
        endpoint: &str,
        token: &str,
        root: PathBuf,
        key: [u8; 32],
        rooms: Vec<HostRoom>,
        limits: Limits,
    ) -> Result<Self, Error> {
        limits.validate()?;
        let t = &identity.transport;
        if [t.generation, t.registration_generation]
            .iter()
            .any(|n| *n == 0 || *n > JSON_SAFE_MAX)
            || t.engagement_id.is_empty()
            || t.engagement_id.len() > 128
            || t.device_id.is_empty()
            || t.device_id.len() > 255
            || t.device_id.chars().any(char::is_control)
            || identity.registration_fingerprint.len() != 64
            || !identity
                .registration_fingerprint
                .bytes()
                .all(|b| b.is_ascii_hexdigit())
            || rooms.is_empty()
            || rooms.len() > 16
            || token.len() < 16
            || token.len() > 4096
            || !token.bytes().all(|b| (33..=126).contains(&b))
        {
            return Err(Error::Config);
        }
        matrix_user(&t.sender_mxid, &identity.server_name).map_err(|_| Error::Config)?;
        let mut ids = BTreeSet::new();
        for r in &rooms {
            matrix_room(&r.room_id, &identity.server_name).map_err(|_| Error::Config)?;
            if r.generation == 0 || r.generation > JSON_SAFE_MAX || !ids.insert(r.room_id.clone()) {
                return Err(Error::Config);
            }
            if let RoomPrivacy::Direct { human_mxid } = &r.privacy {
                matrix_user(human_mxid, &identity.server_name).map_err(|_| Error::Config)?;
                if human_mxid == &t.sender_mxid {
                    return Err(Error::Config);
                }
            }
        }
        let url = Url::parse(endpoint).map_err(|_| Error::Config)?;
        if endpoint.len() > 2048
            || endpoint.bytes().any(|b| b <= 32 || b == b'\\')
            || endpoint.contains(['@', '%'])
            || url.as_str() != endpoint
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || url.path() != "/"
            || url.port() == Some(0)
            || url.host_str().is_none()
            || !matches!(url.scheme(), "https" | "http")
        {
            return Err(Error::Config);
        }
        if url.scheme() == "http"
            && !url
                .host_str()
                .unwrap_or("")
                .trim_matches(['[', ']'])
                .parse::<IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
        {
            return Err(Error::Config);
        }
        let mut authorization =
            HeaderValue::from_str(&format!("Bearer {token}")).map_err(|_| Error::Config)?;
        authorization.set_sensitive(true);
        Ok(Self {
            enrollment: None,
            approval: false,
            endpoint: url,
            authorization,
            identity,
            rooms,
            root,
            key,
            limits,
            roots: vec![],
        })
    }
    pub fn with_root_pem(mut self, pem: &[u8]) -> Result<Self, Error> {
        if pem.len() > 16384 || self.roots.len() >= 4 {
            return Err(Error::Config);
        }
        self.roots
            .push(reqwest::Certificate::from_pem(pem).map_err(|_| Error::Config)?);
        Ok(self)
    }
    pub(crate) fn binding(&self) -> Result<String, Error> {
        // Neither access token nor changing transport/room observation generation is SDK identity.
        let identity = canonical::transport_digest(&serde_json::json!({"origin":self.endpoint.as_str(),"registration":self.identity.registration_fingerprint,"registration_generation":self.identity.transport.registration_generation,"engagement":self.identity.transport.engagement_id,"server":self.identity.server_name,"account":self.identity.transport.sender_mxid,"device":self.identity.transport.device_id,"rooms":self.rooms.iter().map(|r|&r.room_id).collect::<BTreeSet<_>>()})).map_err(|_|Error::Config)?;
        if self.approval {
            canonical::transport_digest(&serde_json::json!(["approval-reader-v1", identity]))
                .map_err(|_| Error::Config)
        } else {
            Ok(identity)
        }
    }
    /// Explicit ordinary fresh-account enrollment. Public peer masters must be
    /// provisioned by the host outside the Matrix key-query channel.
    pub fn with_fresh_account_enrollment(
        mut self,
        anchors: Vec<(String, String)>,
    ) -> Result<Self, Error> {
        if self.approval || self.endpoint.scheme() != "https" {
            return Err(Error::Config);
        }
        self.enrollment = Some(crate::enrollment::state::Profile::new(
            anchors,
            &self.identity.transport.sender_mxid,
            &self.identity.server_name,
        )?);
        Ok(self)
    }
}
