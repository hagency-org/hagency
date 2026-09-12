//! Protected approval cursor custody. Only the owned SDK derives Proof values.
use crate::{Error, wire};
use hagency_core::{approvals::*, canonical};
use matrix_sdk_base::sync::SyncResponse;
use matrix_sdk_common::deserialized_responses::{
    AlgorithmInfo, TimelineEventKind, VerificationState,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
pub(crate) const MAX_BATCHES: usize = 64;
pub(crate) const MAX_OUTCOMES: usize = 256;
pub(crate) const MAX_EVENTS: usize = 100;
#[derive(Clone, Serialize, Deserialize)]
#[serde(remote = "ApprovalRoomAuthority")]
struct AuthorityDef {
    engagement_id: String,
    fleet_id: String,
    project_id: String,
    registration_generation: u64,
    server_name: String,
    room_id: String,
    project_room_id: String,
    owner_mxid: String,
    bot_mxid: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(remote = "ApprovalIntakeTarget")]
struct TargetDef {
    #[serde(with = "AuthorityDef")]
    authority: ApprovalRoomAuthority,
    device_id: String,
    room_generation: u64,
    binding_generation: u64,
    request_id: String,
    request_digest: String,
    expires_at: u64,
    reusable_scope: bool,
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Target(#[serde(with = "TargetDef")] pub ApprovalIntakeTarget);
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Room {
    #[serde(with = "AuthorityDef")]
    pub authority: ApprovalRoomAuthority,
    pub device: String,
    pub generation: u64,
}
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Phase {
    Prepared,
    Applying,
    Derived,
    Quarantined,
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Batch {
    pub token: String,
    pub digest: String,
    pub identity: String,
    pub targets: Vec<Target>,
    pub rooms: Vec<Room>,
    pub raw: Value,
    pub keys: Value,
    pub query_id: String,
    pub phase: Phase,
    pub events: Vec<Event>,
    pub acknowledgements: Vec<Outcome>,
    pub filtered: usize,
}
#[derive(Clone, Serialize, Deserialize)]
struct Proof {
    sender: String,
    device: String,
    session: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Event {
    pub source: String,
    pub digest: String,
    proof_digest: String,
    replay: Option<Outcome>,
    pub event_id: String,
    pub room_id: String,
    immutable: Value,
    server_name: String,
    target: Option<Target>,
    choice: Option<ApprovalChoice>,
    proof: Option<Proof>,
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) enum Outcome {
    Accepted {
        request_id: String,
        choice: ApprovalChoice,
    },
    Rejected,
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Tombstone {
    pub source: String,
    pub digest: String,
    pub outcome: Outcome,
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Receipt {
    pub token: String,
    pub digest: String,
    pub targets_digest: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Detail {
    version: u64,
    kind: String,
    agent: String,
    project: String,
    project_room_id: String,
    request_id: String,
    input_digest: String,
    action: String,
}
impl Event {
    pub fn input(&self) -> Option<ApprovalVerdictInput> {
        let t = &self.target.as_ref()?.0;
        let p = self.proof.as_ref()?;
        Some(ApprovalVerdictInput {
            target: t.clone(),
            source_digest: hash(&json!([self.digest, self.proof_digest])).ok()?,
            verdict: OwnerVerdictObservation {
                request_id: t.request_id.clone(),
                request_digest: t.request_digest.clone(),
                binding_generation: t.binding_generation,
                server_name: t.authority.server_name.clone(),
                room_id: self.room_id.clone(),
                sender_mxid: p.sender.clone(),
                event_id: self.event_id.clone(),
                encrypted: true,
                choice: self.choice?,
            },
        })
    }
    pub fn outcome(&self) -> Outcome {
        match self.input() {
            Some(v) => Outcome::Accepted {
                request_id: v.verdict.request_id,
                choice: v.verdict.choice,
            },
            None => Outcome::Rejected,
        }
    }
}
impl Batch {
    pub fn new(
        raw: Value,
        keys: Value,
        targets: Vec<Target>,
        rooms: Vec<Room>,
        query_id: String,
        identity: String,
    ) -> Result<Self, Error> {
        let token = raw
            .get("next_batch")
            .and_then(Value::as_str)
            .ok_or(Error::Wire)?
            .to_string();
        let digest = hash(&raw)?;
        Ok(Self {
            token,
            digest,
            identity,
            targets,
            rooms,
            raw,
            keys,
            query_id,
            phase: Phase::Prepared,
            events: vec![],
            acknowledgements: vec![],
            filtered: 0,
        })
    }
    pub fn derive(
        &mut self,
        sync: SyncResponse,
        devices: &BTreeSet<(String, String)>,
        history: &[Tombstone],
    ) -> Result<(), Error> {
        if sync
            .rooms
            .left
            .values()
            .any(|r| !r.timeline.events.is_empty())
        {
            return Err(Error::Unsupported);
        }
        let expected = self.raw_events()?;
        let mut events = vec![];
        let mut filtered = 0;
        let mut seen = BTreeSet::new();
        let mut order: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (room, update) in sync.rooms.joined {
            if update.timeline.limited {
                return Err(Error::Unsupported);
            }
            let scopes: Vec<_> = self
                .rooms
                .iter()
                .filter(|r| r.authority.room_id == room.as_str())
                .collect();
            if scopes.is_empty() {
                return Err(Error::Generation);
            }
            let scope = scopes[0];
            for timeline in update.timeline.events {
                if seen.len() >= MAX_EVENTS {
                    return Err(Error::Capacity);
                }
                timeline.raw().deserialize().map_err(|_| Error::Wire)?;
                let value = wire::json(timeline.raw().json().get().as_bytes())?;
                let field = |key: &str| value.get(key).and_then(Value::as_str).ok_or(Error::Wire);
                let event_id = field("event_id")?;
                if !seen.insert((room.to_string(), event_id.to_owned()))
                    || value
                        .get("room_id")
                        .is_some_and(|v| v.as_str() != Some(room.as_str()))
                {
                    return Err(Error::Conflict);
                }
                order
                    .entry(room.to_string())
                    .or_default()
                    .push(event_id.to_owned());
                let proof = match &timeline.kind {
                    TimelineEventKind::UnableToDecrypt { .. } => return Err(Error::Unsupported),
                    TimelineEventKind::PlainText { .. } => None,
                    TimelineEventKind::Decrypted(d) => {
                        let i = &d.encryption_info;
                        let device = i.sender_device.as_ref().map(ToString::to_string);
                        let session = match &i.algorithm_info {
                            AlgorithmInfo::MegolmV1AesSha2 {
                                session_id: Some(s),
                                ..
                            } => Some(s.clone()),
                            _ => None,
                        };
                        if matches!(i.verification_state, VerificationState::Verified)
                            && i.forwarder.is_none()
                            && i.sender.as_str() == field("sender")?
                            && i.sender.as_str() == scope.authority.owner_mxid
                            && device.as_ref().is_some_and(|d| {
                                devices.contains(&(i.sender.to_string(), d.clone()))
                            })
                        {
                            Some(Proof {
                                sender: i.sender.to_string(),
                                device: device.ok_or(Error::Unsupported)?,
                                session: session.ok_or(Error::Unsupported)?,
                            })
                        } else {
                            None
                        }
                    }
                };
                let content = value.get("content").ok_or(Error::Wire)?;
                if field("type")? != "m.room.message"
                    || content.get("msgtype").and_then(Value::as_str)
                        != Some("com.agentchat.approval.verdict.v1")
                {
                    filtered += 1;
                }
                let source = hash(&json!([
                    scope.authority.server_name,
                    room.as_str(),
                    event_id
                ]))?;
                // Unsigned transport age is not immutable message content.
                let immutable = json!({"event_id":event_id,"sender":value.get("sender"),"type":value.get("type"),"origin_server_ts":value.get("origin_server_ts"),"content":content,"redacted":value.get("unsigned").is_some_and(|v|v.get("redacted_because").is_some())});
                let proof_digest = hash(&json!([
                    scope.authority.server_name,
                    room.as_str(),
                    immutable,
                    proof
                ]))?;
                let raw = expected
                    .get(&(room.to_string(), event_id.to_owned()))
                    .ok_or(Error::Unsupported)?;
                let digest = immutable_wire(raw)?;
                // Ciphertext identity outlives target plans and SDK trust changes.
                let replay = if let Some(old) = history.iter().find(|t| t.source == source) {
                    if old.digest != digest {
                        return Err(Error::Conflict);
                    }
                    Some(old.outcome.clone())
                } else {
                    None
                };
                let (target, choice) = select(
                    &self.targets,
                    room.as_str(),
                    &scope.authority.owner_mxid,
                    &immutable,
                    proof.is_some(),
                );
                events.push(Event {
                    source,
                    digest,
                    proof_digest,
                    replay,
                    event_id: event_id.into(),
                    room_id: room.to_string(),
                    immutable,
                    server_name: scope.authority.server_name.clone(),
                    target,
                    choice,
                    proof,
                });
            }
        }
        if expected.keys().cloned().collect::<BTreeSet<_>>() != seen || order != self.raw_order()? {
            return Err(Error::Unsupported);
        }
        self.events = events;
        self.filtered = filtered;
        self.phase = Phase::Derived;
        Ok(())
    }
    pub fn receipt(&self) -> Result<Receipt, Error> {
        if self.phase != Phase::Derived || self.acknowledgements.len() != self.events.len() {
            return Err(Error::OutcomeUnknown);
        }
        Ok(Receipt {
            token: self.token.clone(),
            digest: self.digest.clone(),
            targets_digest: hash(&json!([self.targets, self.rooms]))?,
        })
    }
    pub fn validate(&self, identity: &str, user: &str, device: &str) -> Result<(), Error> {
        if self.identity != identity
            || self.token.is_empty()
            || self.token.len() > 4096
            || self.digest != hash(&self.raw)?
            || self.targets.len() > 64
            || self.rooms.is_empty()
            || self.rooms.len() > 64
            || self.events.len() > MAX_EVENTS
            || self.acknowledgements.len() > self.events.len()
            || self.query_id.is_empty()
            || self.query_id.len() > 255
            || self.raw.get("next_batch").and_then(Value::as_str) != Some(&self.token)
            || self
                .rooms
                .iter()
                .any(|r| r.authority.bot_mxid != user || r.device != device || r.generation == 0)
            || (matches!(self.phase, Phase::Prepared | Phase::Applying)
                && (!self.events.is_empty() || !self.acknowledgements.is_empty()))
        {
            return Err(Error::Storage);
        }
        crate::outgoing::state::encode(&self.keys, 256 * 1024)?;
        crate::outgoing::state::encode(
            &serde_json::to_value(self).map_err(|_| Error::Storage)?,
            3 * 1024 * 1024,
        )?;
        let expected = if self.phase == Phase::Derived || !self.events.is_empty() {
            self.raw_events()?
        } else {
            BTreeMap::new()
        };
        if self.phase == Phase::Derived && self.events.len() != expected.len() {
            return Err(Error::Storage);
        }
        let mut room_ids = BTreeSet::new();
        for r in &self.rooms {
            let a = &r.authority;
            if !room_ids.insert(&a.engagement_id)
                || a.registration_generation == 0
                || a.owner_mxid == a.bot_mxid
                || a.room_id == a.project_room_id
                || [
                    &a.engagement_id,
                    &a.fleet_id,
                    &a.project_id,
                    &a.server_name,
                    &a.room_id,
                    &a.project_room_id,
                    &a.owner_mxid,
                    &a.bot_mxid,
                ]
                .iter()
                .any(|s| s.is_empty() || s.len() > 512)
                || self.rooms.iter().any(|other| {
                    other.authority.room_id == a.room_id
                        && (other.authority.owner_mxid != a.owner_mxid
                            || other.authority.server_name != a.server_name
                            || other.generation != r.generation)
                })
            {
                return Err(Error::Storage);
            }
        }
        let mut target_ids = BTreeSet::new();
        for Target(t) in &self.targets {
            if !target_ids.insert(&t.request_id)
                || t.request_id.strip_prefix("approval_").is_none_or(|id| {
                    id.len() != 40
                        || !id
                            .bytes()
                            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
                })
                || !crate::outgoing::state::digest(&t.request_digest)
                || t.binding_generation == 0
                || t.expires_at == 0
                || !self.rooms.iter().any(|r| {
                    r.authority == t.authority
                        && r.device == t.device_id
                        && r.generation == t.room_generation
                })
            {
                return Err(Error::Storage);
            }
        }
        let mut seen = BTreeSet::new();
        for e in &self.events {
            if !seen.insert(&e.source)
                || e.source != hash(&json!([e.server_name, e.room_id, e.event_id]))?
                || e.proof_digest != hash(&json!([e.server_name, e.room_id, e.immutable, e.proof]))?
                || e.digest
                    != immutable_wire(
                        expected
                            .get(&(e.room_id.clone(), e.event_id.clone()))
                            .ok_or(Error::Storage)?,
                    )?
                || e.immutable.get("event_id").and_then(Value::as_str) != Some(e.event_id.as_str())
                || !self.rooms.iter().any(|r| {
                    r.authority.room_id == e.room_id && r.authority.server_name == e.server_name
                })
                || !crate::outgoing::state::digest(&e.source)
                || !crate::outgoing::state::digest(&e.digest)
                || e.target.is_some() != e.choice.is_some()
            {
                return Err(Error::Storage);
            }
            let raw = expected
                .get(&(e.room_id.clone(), e.event_id.clone()))
                .ok_or(Error::Storage)?;
            if ["sender", "origin_server_ts"]
                .iter()
                .any(|key| raw.get(*key) != e.immutable.get(*key))
                || (e.proof.is_some()
                    && raw.get("type").and_then(Value::as_str) != Some("m.room.encrypted"))
                || (raw.get("type").and_then(Value::as_str) != Some("m.room.encrypted")
                    && ["type", "content"]
                        .iter()
                        .any(|key| raw.get(*key) != e.immutable.get(*key)))
            {
                return Err(Error::Storage);
            }
            let owner = &self
                .rooms
                .iter()
                .find(|r| r.authority.room_id == e.room_id)
                .ok_or(Error::Storage)?
                .authority
                .owner_mxid;
            let (expected_target, expected_choice) = select(
                &self.targets,
                &e.room_id,
                owner,
                &e.immutable,
                e.proof.is_some(),
            );
            if expected_target.as_ref().map(|t| &t.0) != e.target.as_ref().map(|t| &t.0)
                || expected_choice != e.choice
            {
                return Err(Error::Storage);
            }
            if let Some(input) = e.input() {
                if !self.targets.iter().any(|t| t.0 == input.target)
                    || e.proof.as_ref().is_none_or(|p| {
                        p.device.is_empty()
                            || p.session.is_empty()
                            || p.sender != input.target.authority.owner_mxid
                            || e.immutable.get("sender").and_then(Value::as_str)
                                != Some(p.sender.as_str())
                            || self
                                .keys
                                .get("device_keys")
                                .and_then(|v| v.get(&p.sender))
                                .and_then(|v| v.get(&p.device))
                                .is_none()
                    })
                    || e.room_id != input.target.authority.room_id
                {
                    return Err(Error::Storage);
                }
            } else if e.target.is_some() {
                return Err(Error::Storage);
            }
        }
        if self.phase == Phase::Derived {
            let mut order: BTreeMap<String, Vec<String>> = BTreeMap::new();
            for e in &self.events {
                order
                    .entry(e.room_id.clone())
                    .or_default()
                    .push(e.event_id.clone());
            }
            if order != self.raw_order()? {
                return Err(Error::Storage);
            }
        }
        for (event, outcome) in self.events.iter().zip(&self.acknowledgements) {
            if !event.permits(outcome) {
                return Err(Error::Storage);
            }
        }
        Ok(())
    }
    fn raw_order(&self) -> Result<BTreeMap<String, Vec<String>>, Error> {
        let mut order = BTreeMap::new();
        if let Some(rooms) = self.raw.pointer("/rooms/join").and_then(Value::as_object) {
            for (room, update) in rooms {
                if let Some(events) = update.pointer("/timeline/events").and_then(Value::as_array) {
                    for event in events {
                        order.entry(room.clone()).or_insert_with(Vec::new).push(
                            event
                                .get("event_id")
                                .and_then(Value::as_str)
                                .ok_or(Error::Wire)?
                                .to_owned(),
                        );
                    }
                }
            }
        }
        Ok(order)
    }
    fn raw_events(&self) -> Result<BTreeMap<(String, String), Value>, Error> {
        let mut entries = BTreeMap::new();
        if let Some(rooms) = self.raw.pointer("/rooms/join").and_then(Value::as_object) {
            for (room, update) in rooms {
                if update.pointer("/timeline/limited").and_then(Value::as_bool) == Some(true) {
                    return Err(Error::Unsupported);
                }
                if let Some(events) = update.pointer("/timeline/events").and_then(Value::as_array) {
                    for raw in events {
                        let id = raw
                            .get("event_id")
                            .and_then(Value::as_str)
                            .ok_or(Error::Wire)?;
                        ruma::EventId::parse(id).map_err(|_| Error::Wire)?;
                        if raw.get("room_id").is_some_and(|r| r.as_str() != Some(room))
                            || entries.len() >= MAX_EVENTS
                        {
                            return Err(Error::Capacity);
                        }
                        if entries
                            .insert((room.clone(), id.into()), raw.clone())
                            .is_some()
                        {
                            return Err(Error::Conflict);
                        }
                    }
                }
            }
        }
        Ok(entries)
    }
}
impl Event {
    pub(crate) fn permits(&self, outcome: &Outcome) -> bool {
        match &self.replay {
            Some(old) => old == outcome,
            None => *outcome == Outcome::Rejected || *outcome == self.outcome(),
        }
    }
    pub(crate) fn replay(&self) -> Option<&Outcome> {
        self.replay.as_ref()
    }
}
fn immutable_wire(raw: &Value) -> Result<String, Error> {
    let mut value = raw.as_object().ok_or(Error::Wire)?.clone();
    value.remove("unsigned");
    hash(&Value::Object(value))
}
pub(crate) fn hash(value: &Value) -> Result<String, Error> {
    canonical::transport_digest(value).map_err(|_| Error::Wire)
}
pub(crate) enum Command {
    Read,
    Start(Box<Batch>),
    Apply,
    Ack(usize, Outcome),
    Finish,
    Quarantine,
}
pub(crate) struct View {
    pub batch: Option<Batch>,
    pub receipts: Vec<Receipt>,
    pub outcomes: Vec<Tombstone>,
}

fn select(
    targets: &[Target],
    room: &str,
    owner: &str,
    event: &Value,
    verified: bool,
) -> (Option<Target>, Option<ApprovalChoice>) {
    let candidate = || -> Option<(Target, ApprovalChoice)> {
        if !verified
            || event.get("redacted") != Some(&Value::Bool(false))
            || event.get("type").and_then(Value::as_str) != Some("m.room.message")
        {
            return None;
        }
        let content = event.get("content")?.as_object()?;
        if content.get("msgtype")?.as_str() != Some("com.agentchat.approval.verdict.v1")
            || content
                .get("m.relates_to")
                .is_some_and(|v| v.get("rel_type").and_then(Value::as_str) == Some("m.replace"))
            || !content.keys().all(|k| {
                ["msgtype", "body", "com.agentchat.approval", "m.relates_to"].contains(&k.as_str())
            })
        {
            return None;
        }
        let d: Detail =
            serde_json::from_value(content.get("com.agentchat.approval")?.clone()).ok()?;
        if d.version != 1 || d.kind != "verdict" {
            return None;
        }
        let choice = match d.action.as_str() {
            "approve_once" => ApprovalChoice::Once,
            "approve_task" => ApprovalChoice::Task,
            "approve_always" => ApprovalChoice::Always,
            "deny" => ApprovalChoice::Deny,
            _ => return None,
        };
        let t = targets.iter().find(|t| t.0.request_id == d.request_id)?;
        let a = &t.0.authority;
        if d.input_digest != t.0.request_digest
            || d.agent != a.engagement_id
            || d.project != a.project_id
            || d.project_room_id != a.project_room_id
            || a.room_id != room
            || a.owner_mxid != owner
            || (!t.0.reusable_scope
                && !matches!(choice, ApprovalChoice::Once | ApprovalChoice::Deny))
        {
            return None;
        }
        Some((t.clone(), choice))
    };
    match candidate() {
        Some((t, c)) => (Some(t), Some(c)),
        None => (None, None),
    }
}
