//! One protected original enrollment. No public request or identity constructor.
use crate::Error;
use hagency_core::{canonical, replies::matrix_user};
use matrix_sdk_crypto::vodozemac::Ed25519PublicKey;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) const KEY: &[u8] = b"hagency.enrollment.v1";
pub(crate) const FIELD: usize = 64 * 1024;
pub(crate) const QUERY: usize = 256 * 1024;
pub(crate) const PLAIN: usize = 4 * 1024 * 1024;
pub(crate) const ENVELOPE: usize = 24 * 1024 * 1024;
pub(crate) const WRITES: usize = 20;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Profile {
    pub anchors: BTreeMap<String, String>,
}
impl Profile {
    pub fn new(anchors: Vec<(String, String)>, own: &str, _server: &str) -> Result<Self, Error> {
        if anchors.is_empty() || anchors.len() > 16 {
            return Err(Error::Config);
        }
        let mut keys = BTreeSet::new();
        let mut map = BTreeMap::new();
        for (user, key) in anchors {
            let parsed = ruma::OwnedUserId::try_from(user.as_str()).map_err(|_| Error::Config)?;
            matrix_user(&user, parsed.server_name().as_str()).map_err(|_| Error::Config)?;
            let public = Ed25519PublicKey::from_base64(&key).map_err(|_| Error::Config)?;
            if user == own
                || public.to_base64() != key
                || !keys.insert(key.clone())
                || map.insert(user, key).is_some()
            {
                return Err(Error::Config);
            }
        }
        let value = Self { anchors: map };
        size(&value, 16 * 1024)?;
        Ok(value)
    }
    pub fn users(&self, own: &str, users: &[String]) -> Result<(), Error> {
        if users.is_empty()
            || users.len() > 17
            || !users.iter().any(|u| u == own)
            || users.windows(2).any(|w| w[0] >= w[1])
            || users
                .iter()
                .any(|u| u != own && !self.anchors.contains_key(u))
        {
            return Err(Error::Recipients);
        }
        Ok(())
    }
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Context {
    pub binding: String,
    pub identity: String,
    pub user: String,
    pub device: String,
    pub profile: Profile,
}
impl Context {
    pub fn marker(&self) -> Result<String, Error> {
        canonical::transport_digest(&serde_json::to_value(self).map_err(|_| Error::Storage)?)
            .map_err(|_| Error::Storage)
    }
}
#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) enum Phase {
    Preparing,
    Writing,
    Query,
    Ready,
    Complete,
}
#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) enum WritePhase {
    Prepared,
    Possible,
    Response,
    Applying,
    Applied,
}
#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) enum Kind {
    Device,
    Signing,
    Signature,
    Claim,
}
impl Kind {
    pub fn path(self) -> &'static [&'static str] {
        match self {
            Self::Device => &["_matrix", "client", "v3", "keys", "upload"],
            Self::Signing => &[
                "_matrix",
                "client",
                "v3",
                "keys",
                "device_signing",
                "upload",
            ],
            Self::Signature => &["_matrix", "client", "v3", "keys", "signatures", "upload"],
            Self::Claim => &["_matrix", "client", "v3", "keys", "claim"],
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Write {
    pub kind: Kind,
    pub id: String,
    pub body: String,
    pub digest: String,
    pub response: Option<Value>,
    pub phase: WritePhase,
}
impl Write {
    pub fn new(kind: Kind, id: String, body: Value) -> Result<Self, Error> {
        let body = encode(&body, FIELD)?;
        size(&body, FIELD)?; // Count the actual embedded string, including escaping.
        Ok(Self {
            kind,
            id,
            digest: crate::outgoing::state::hash(body.as_bytes()),
            body,
            response: None,
            phase: WritePhase::Prepared,
        })
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Query {
    pub id: String,
    pub body: String,
    pub response: Option<Value>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Session {
    pub user: String,
    pub device: String,
    pub curve: String,
    pub before_ids: Vec<String>,
    pub ids: Vec<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Ledger {
    pub context: Context,
    pub users: Vec<String>,
    pub phase: Phase,
    pub writes: Vec<Write>,
    pub initial: Query,
    pub verified: Option<Query>,
    pub public: Option<Value>,
    pub sessions: Vec<Session>,
}
impl Ledger {
    pub fn validate(
        &self,
        binding: &str,
        identity: &str,
        user: &str,
        device: &str,
    ) -> Result<(), Error> {
        let c = &self.context;
        if c.binding != binding || c.identity != identity || c.user != user || c.device != device {
            return Err(Error::Identity);
        }
        let own: ruma::OwnedUserId = user.try_into().map_err(|_| Error::Storage)?;
        let profile = Profile::new(
            c.profile
                .anchors
                .iter()
                .map(|(a, b)| (a.clone(), b.clone()))
                .collect(),
            user,
            own.server_name().as_str(),
        )?;
        if profile != c.profile {
            return Err(Error::Storage);
        }
        profile.users(user, &self.users)?;
        if self.writes.len() > WRITES || self.sessions.len() > 64 {
            return Err(Error::Capacity);
        }
        size(&self.initial, QUERY)?;
        if let Some(query) = &self.verified {
            size(query, QUERY)?;
        }
        let mut ids = BTreeSet::new();
        let mut unapplied = false;
        for w in &self.writes {
            if w.id.is_empty()
                || w.id.len() > 128
                || !ids.insert(&w.id)
                || w.digest != crate::outgoing::state::hash(w.body.as_bytes())
                || serde_json::from_str::<Value>(&w.body).is_err()
            {
                return Err(Error::Storage);
            }
            size(&w.body, FIELD)?;
            size(&w.response, FIELD)?;
            if w.phase == WritePhase::Applied {
                if unapplied || w.response.is_none() {
                    return Err(Error::Storage);
                }
            } else {
                unapplied = true;
            }
            if matches!(w.phase, WritePhase::Response | WritePhase::Applying)
                && w.response.is_none()
            {
                return Err(Error::Storage);
            }
        }
        let mut devices = BTreeSet::new();
        for s in &self.sessions {
            if !self.users.contains(&s.user)
                || !devices.insert((&s.user, &s.device))
                || s.device.is_empty()
                || s.device.len() > 255
                || s.ids.len() > 4
                || s.before_ids.len() > 4
                || s.before_ids.windows(2).any(|ids| ids[0] >= ids[1])
                || s.ids.windows(2).any(|ids| ids[0] >= ids[1])
                || s.before_ids
                    .iter()
                    .any(|id| id.is_empty() || id.len() > 128)
                || s.ids.iter().any(|id| id.is_empty() || id.len() > 128)
                || matrix_sdk_crypto::vodozemac::Curve25519PublicKey::from_base64(&s.curve).is_err()
            {
                return Err(Error::Storage);
            }
        }
        if self.phase == Phase::Complete
            && (unapplied
                || self.writes.len() < 4
                || self.public.is_none()
                || self
                    .verified
                    .as_ref()
                    .and_then(|q| q.response.as_ref())
                    .is_none()
                || self.sessions.is_empty()
                || self.sessions.iter().any(|s| s.ids.is_empty()))
        {
            return Err(Error::Storage);
        }
        self.validate_protocol()?;
        // Every field's serialized contribution is accounted before the full record.
        let metadata = serde_json::json!({"context":c,"users":self.users,"public":self.public,
            "sessions":self.sessions,"writes":self.writes.iter().map(|w|(&w.id,&w.digest)).collect::<Vec<_>>()});
        size(&metadata, FIELD - 4096)?; // Fixed structural fields and separators fit remaining4KiB.
        size(self, PLAIN)?;
        Ok(())
    }

    fn validate_protocol(&self) -> Result<(), Error> {
        let peers = self
            .users
            .iter()
            .filter(|u| **u != self.context.user)
            .collect::<Vec<_>>();
        if self.phase == Phase::Preparing {
            return if self.writes.is_empty()
                && self.public.is_none()
                && self.verified.is_none()
                && self.sessions.is_empty()
            {
                Ok(())
            } else {
                Err(Error::Storage)
            };
        }
        let count = 3 + peers.len();
        if self.writes.len() != count && self.writes.len() != count + 1 {
            return Err(Error::Storage);
        }
        let public = self.public.as_ref().ok_or(Error::Storage)?;
        let initial = self.initial.response.as_ref().ok_or(Error::Storage)?;
        let verified = self.verified.as_ref().and_then(|q| q.response.as_ref());
        let bodies = self
            .writes
            .iter()
            .map(|w| serde_json::from_str::<Value>(&w.body).map_err(|_| Error::Storage))
            .collect::<Result<Vec<_>, _>>()?;
        let device = &bodies[0]["device_keys"];
        if device["user_id"].as_str() != Some(&self.context.user)
            || device["device_id"].as_str() != Some(&self.context.device)
            || device["keys"]
                .as_object()
                .is_none_or(|keys| keys.len() != 2)
            || bodies[0]["one_time_keys"].as_object().is_none_or(|keys| {
                keys.is_empty() || keys.keys().any(|id| !id.starts_with("signed_curve25519:"))
            })
        {
            return Err(Error::Storage);
        }
        for (field, kind) in [
            ("master_key", "master"),
            ("self_signing_key", "self_signing"),
            ("user_signing_key", "user_signing"),
        ] {
            if !same(
                &bodies[1][field],
                &public[kind],
                &["user_id", "usage", "keys"],
            ) {
                return Err(Error::Storage);
            }
        }
        if let Some(query) = verified
            && !same(
                device,
                &query["device_keys"][&self.context.user][&self.context.device],
                &["user_id", "device_id", "algorithms", "keys"],
            )
        {
            return Err(Error::Storage);
        }
        for (index, write) in self.writes.iter().enumerate() {
            let expected = match index {
                0 => Kind::Device,
                1 => Kind::Signing,
                n if n < count => Kind::Signature,
                _ => Kind::Claim,
            };
            if write.kind != expected || bodies[index].get("auth").is_some() {
                return Err(Error::Storage);
            }
            if matches!(write.phase, WritePhase::Prepared | WritePhase::Possible)
                && write.response.is_some()
            {
                return Err(Error::Storage);
            }
            if write.kind == Kind::Signature {
                let user = if index == 2 {
                    &self.context.user
                } else {
                    peers[index - 3]
                };
                let body = bodies[index].as_object().ok_or(Error::Storage)?;
                if body.len() != 1 || !body.contains_key(user) {
                    return Err(Error::Storage);
                }
                let objects = body[user].as_object().ok_or(Error::Storage)?;
                if objects.is_empty() || objects.len() > if index == 2 { 2 } else { 1 } {
                    return Err(Error::Storage);
                }
                for (id, signed) in objects {
                    let (target, final_target) = if index == 2 && id == &self.context.device {
                        (device, verified.map(|q| &q["device_keys"][user][id]))
                    } else {
                        let master = if index == 2 {
                            &public["master"]
                        } else {
                            &initial["master_keys"][user]
                        };
                        if master["keys"].as_object().is_none_or(|keys| {
                            keys.len() != 1 || !keys.values().any(|key| key.as_str() == Some(id))
                        }) {
                            return Err(Error::Storage);
                        }
                        (master, verified.map(|q| &q["master_keys"][user]))
                    };
                    if !signed_fields(signed, target) {
                        return Err(Error::Storage);
                    }
                    if let Some(target) = final_target
                        && (!signed_fields(signed, target) || !signature_subset(signed, target))
                    {
                        return Err(Error::Storage);
                    }
                }
            }
            if write.kind == Kind::Claim {
                let expected = self
                    .sessions
                    .iter()
                    .filter(|s| s.before_ids.is_empty())
                    .map(|s| (s.user.as_str(), s.device.as_str()))
                    .collect::<BTreeSet<_>>();
                let keys = bodies[index]["one_time_keys"]
                    .as_object()
                    .ok_or(Error::Storage)?;
                let mut actual = BTreeSet::new();
                for (user, devices) in keys {
                    for (device, algorithm) in devices.as_object().ok_or(Error::Storage)? {
                        if algorithm.as_str() != Some("signed_curve25519") {
                            return Err(Error::Storage);
                        }
                        actual.insert((user.as_str(), device.as_str()));
                    }
                }
                if actual.is_empty() || actual != expected {
                    return Err(Error::Storage);
                }
            }
            if write.phase == WritePhase::Applied {
                validate_ack(
                    write.kind,
                    &bodies[index],
                    write.response.as_ref().ok_or(Error::Storage)?,
                )?;
            }
        }
        let applied = self.writes.iter().all(|w| w.phase == WritePhase::Applied);
        if self.phase == Phase::Query && (self.writes.len() != count || !applied) {
            return Err(Error::Storage);
        }
        if matches!(self.phase, Phase::Ready | Phase::Complete)
            && (!applied || verified.is_none() || self.sessions.is_empty())
        {
            return Err(Error::Storage);
        }
        if matches!(self.phase, Phase::Ready | Phase::Complete) {
            for session in &self.sessions {
                if session.ids.is_empty()
                    || (!session.before_ids.is_empty() && session.before_ids != session.ids)
                    || (session.before_ids.is_empty()
                        && (self.writes.len() != count + 1 || session.ids.len() != 1))
                {
                    return Err(Error::Storage);
                }
            }
        }
        Ok(())
    }
}

fn same(a: &Value, b: &Value, fields: &[&str]) -> bool {
    a.is_object()
        && b.is_object()
        && fields
            .iter()
            .all(|field| a.get(*field).is_some() && a.get(*field) == b.get(*field))
}
fn signed_fields(signed: &Value, target: &Value) -> bool {
    let required: &[&str] = if target.get("device_id").is_some() {
        &["user_id", "device_id", "algorithms", "keys"]
    } else {
        &["user_id", "usage", "keys"]
    };
    signed.as_object().is_some_and(|fields| {
        fields.contains_key("signatures")
            && fields
                .iter()
                .filter(|(field, _)| field.as_str() != "signatures" && field.as_str() != "unsigned")
                .all(|(field, value)| target.get(field) == Some(value))
    }) && same(signed, target, required)
}
fn signature_subset(signed: &Value, target: &Value) -> bool {
    signed["signatures"].as_object().is_some_and(|users| {
        !users.is_empty()
            && users.iter().all(|(user, keys)| {
                keys.as_object().is_some_and(|keys| {
                    !keys.is_empty()
                        && keys
                            .iter()
                            .all(|(key, value)| target["signatures"][user].get(key) == Some(value))
                })
            })
    })
}
fn validate_ack(kind: Kind, body: &Value, value: &Value) -> Result<(), Error> {
    if !value.is_object()
        || value
            .get("failures")
            .is_some_and(|v| v.as_object().is_none_or(|m| !m.is_empty()))
    {
        return Err(Error::Storage);
    }
    match kind {
        Kind::Device => {
            let count = body["one_time_keys"]
                .as_object()
                .ok_or(Error::Storage)?
                .len();
            if value["one_time_key_counts"]["signed_curve25519"]
                .as_u64()
                .is_none_or(|n| n < count as u64)
            {
                return Err(Error::Storage);
            }
        }
        Kind::Signing if value.as_object().is_none_or(|m| !m.is_empty()) => {
            return Err(Error::Storage);
        }
        Kind::Claim => {
            let expected = body["one_time_keys"].as_object().ok_or(Error::Storage)?;
            let actual = value["one_time_keys"].as_object().ok_or(Error::Storage)?;
            if actual.keys().collect::<BTreeSet<_>>() != expected.keys().collect::<BTreeSet<_>>() {
                return Err(Error::Storage);
            }
            for (user, devices) in expected {
                let expected = devices.as_object().ok_or(Error::Storage)?;
                let actual = actual[user].as_object().ok_or(Error::Storage)?;
                if actual.keys().collect::<BTreeSet<_>>()
                    != expected.keys().collect::<BTreeSet<_>>()
                {
                    return Err(Error::Storage);
                }
                for keys in actual.values() {
                    if keys.as_object().is_none_or(|keys| {
                        keys.len() != 1
                            || keys.iter().any(|(id, key)| {
                                !id.starts_with("signed_curve25519:")
                                    || !key.is_object()
                                    || key.get("signatures").is_none()
                            })
                    }) {
                        return Err(Error::Storage);
                    }
                }
            }
        }
        _ => {}
    }
    Ok(())
}
pub(crate) struct Packet {
    pub index: usize,
    pub kind: Kind,
    pub body: String,
}
pub(crate) enum View {
    Absent,
    Complete,
    Query(String),
    Write(Packet),
    Verify,
    Ready,
    Unit,
}

pub(crate) fn reserve() -> Result<Vec<u8>, Error> {
    let worst = WRITES
        .checked_mul(FIELD * 2)
        .and_then(|n| n.checked_add(2 * QUERY + FIELD))
        .ok_or(Error::Capacity)?;
    if worst > PLAIN {
        return Err(Error::Capacity);
    }
    let mut reservation = Vec::new();
    reservation
        .try_reserve_exact(PLAIN + ENVELOPE)
        .map_err(|_| Error::Capacity)?;
    Ok(reservation)
}
struct Counter {
    used: usize,
    cap: usize,
}
impl std::io::Write for Counter {
    fn write(&mut self, value: &[u8]) -> std::io::Result<usize> {
        if value.len() > self.cap.saturating_sub(self.used) {
            return Err(std::io::Error::other("bounded enrollment"));
        }
        self.used += value.len();
        Ok(value.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
pub(crate) fn size(value: &impl Serialize, cap: usize) -> Result<usize, Error> {
    let mut count = Counter { used: 0, cap };
    serde_json::to_writer(&mut count, value).map_err(|_| Error::Capacity)?;
    Ok(count.used)
}
pub(crate) fn encode(value: &impl Serialize, cap: usize) -> Result<String, Error> {
    size(value, cap)?;
    serde_json::to_string(value).map_err(|_| Error::Storage)
}
