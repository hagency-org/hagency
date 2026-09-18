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
    /// Optional original host-wide scheduler. Clones share its finite cadence.
    pub request_pacing: Option<std::sync::Arc<crate::RequestPacing>>,
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
            request_pacing: None,
        }
    }
}
impl Limits {
    pub(crate) fn validate(&self) -> Result<(), Error> {
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
    /// The original warm factory's shared scheduling guard; never authority.
    pub(crate) factory_rooms: Option<std::sync::Arc<tokio::sync::Mutex<()>>>,
    pub(crate) as_guard: Option<std::sync::Arc<crate::token_provision::application_service::Guard>>,
    pub(crate) provisioning: Option<std::sync::Arc<crate::TokenProvisioningHost>>,
    pub(crate) enrollment: Option<crate::enrollment::state::Profile>,
    pub(crate) approval: bool,
    pub(crate) endpoint: Url,
    pub(crate) authorization: HeaderValue,
    pub(crate) identity: HostIdentity,
    pub(crate) rooms: Vec<HostRoom>,
    /// The pre-project reception room, observed for the provisioning ingress
    /// only. It is never published (`collect_room_observation` skips
    /// `observe_matrix_room` for it); its authority facts are carried in
    /// memory for `verify_request`. Absent when the host has no recorded
    /// registration naming one — bootstrap refuses to start in that case.
    pub(crate) reception_room: Option<HostRoom>,
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
        let url = host_endpoint(endpoint)?;
        if let Some(pacing) = &limits.request_pacing {
            pacing.check_endpoint(&url)?;
        }
        let mut authorization =
            HeaderValue::from_str(&format!("Bearer {token}")).map_err(|_| Error::Config)?;
        authorization.set_sensitive(true);
        Ok(Self {
            factory_rooms: None,
            as_guard: None,
            provisioning: None,
            enrollment: None,
            approval: false,
            endpoint: url,
            authorization,
            identity,
            rooms,
            reception_room: None,
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
    /// The host's engagement id, for the bootstrap fill that resolves the
    /// recorded registration (and its reception room) from the store.
    pub fn engagement_id(&self) -> &str {
        &self.identity.transport.engagement_id
    }
    /// Explicit private account-stage capability for the unfinished inline
    /// factory. Never a physical-completion or route observation setter.
    pub fn with_token_account_provisioning(
        mut self,
        host: crate::TokenProvisioningHost,
    ) -> Result<Self, Error> {
        if self.provisioning.is_some() {
            return Err(Error::Config);
        }
        host.bind(&self)?;
        self.factory_rooms = host.room_coordinator();
        self.provisioning = Some(std::sync::Arc::new(host));
        Ok(self)
    }
    /// Set the pre-project reception room. It is not a `rooms` member (those
    /// carry a project/owner/fleet that do not exist before admission) but is
    /// observed alongside them, never published. Setting it twice is refused.
    pub fn with_reception_room(&mut self, room: HostRoom) -> Result<(), Error> {
        matrix_room(&room.room_id, &self.identity.server_name).map_err(|_| Error::Config)?;
        if room.generation == 0 || room.generation > JSON_SAFE_MAX {
            return Err(Error::Config);
        }
        if self.reception_room.is_some() {
            return Err(Error::Config);
        }
        self.reception_room = Some(room);
        Ok(())
    }
    /// Every room the collector observes: the ordinary scopes plus the
    /// pre-project reception room, in that order.
    pub(crate) fn observed_rooms(&self) -> impl Iterator<Item = &HostRoom> {
        self.rooms.iter().chain(self.reception_room.iter())
    }
    pub(crate) fn binding(&self) -> Result<String, Error> {
        // Neither access token nor changing transport/room observation generation is SDK identity.
        let identity = canonical::transport_digest(&serde_json::json!({"origin":self.endpoint.as_str(),"registration":self.identity.registration_fingerprint,"registration_generation":self.identity.transport.registration_generation,"engagement":self.identity.transport.engagement_id,"server":self.identity.server_name,"account":self.identity.transport.sender_mxid,"device":self.identity.transport.device_id,"rooms":self.rooms.iter().map(|r|&r.room_id).collect::<BTreeSet<_>>()})).map_err(|_|Error::Config)?;
        if let Some(guard) = &self.as_guard {
            canonical::transport_digest(&serde_json::json!([
                "appservice-private-device-v1",
                identity,
                guard.binding()
            ]))
            .map_err(|_| Error::Config)
        } else if self.approval {
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

/// Shared Host-only origin grammar. This changes no existing collector policy.
pub(crate) fn host_endpoint(endpoint: &str) -> Result<Url, Error> {
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
    Ok(url)
}
