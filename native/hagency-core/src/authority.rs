//! Pure verification of observations supplied by an authenticated Matrix adapter.
//! Observation structs deliberately have no Deserialize implementation and are
//! never accepted by HTTP. A verified value proves checks against an observation,
//! not the network authenticity of an arbitrary JSON document.
use crate::{
    InvalidInput, JSON_SAFE_MAX,
    allocation::Tokens,
    canonical,
    project::{self, AgentDefinition},
};
use ruma_common::{RoomId, ServerName, UserId};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Registration {
    pub fleet_id: String,
    pub generation: u64,
    pub server_name: String,
    pub reception_room_id: String,
    pub representative_mxid: String,
    pub approval_bot_mxid: String,
}
fn valid_user(id: &str, server: &str) -> bool {
    UserId::parse(id).is_ok_and(|u| u.server_name().as_str().eq_ignore_ascii_case(server))
}
fn valid_room(id: &str, server: &str) -> bool {
    RoomId::parse(id).is_ok()
        && id
            .split_once(':')
            .is_some_and(|(_, s)| s.eq_ignore_ascii_case(server))
}
impl Registration {
    pub fn validate(&self) -> Result<(), InvalidInput> {
        let suffix = self
            .fleet_id
            .strip_prefix("hf_")
            .ok_or(InvalidInput("invalid fleet"))?;
        if suffix.len() != 32
            || !suffix
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || self.generation == 0
            || self.generation > JSON_SAFE_MAX
            || <&ServerName>::try_from(self.server_name.as_str()).is_err()
            || !valid_room(&self.reception_room_id, &self.server_name)
            || !valid_user(&self.representative_mxid, &self.server_name)
            || !valid_user(&self.approval_bot_mxid, &self.server_name)
            || self.representative_mxid
                != format!("@{}_representative:{}", self.fleet_id, self.server_name)
            || self.approval_bot_mxid == self.representative_mxid
        {
            return Err(InvalidInput("invalid fleet registration"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRequest {
    pub v: u8,
    pub fleet_id: String,
    pub request_id: String,
    pub requester_mxid: String,
    pub source_room_id: String,
    pub target_project_id: String,
    pub target_room_id: String,
    pub owner_mxid: String,
    pub owner_dm_room_id: String,
    pub role: String,
    pub requested_tokens: Tokens,
    pub rate_per_day: Option<Tokens>,
    pub auth_version: u8,
    pub source_event_id: String,
    pub agent_definition: AgentDefinition,
}
impl ProjectRequest {
    pub fn validate(&self, registration: &Registration) -> Result<(), InvalidInput> {
        registration.validate()?;
        project::identifier(&self.request_id, 96)?;
        project::identifier(&self.target_project_id, 128)?;
        project::role(&self.role)?;
        self.agent_definition.validate()?;
        let server = &registration.server_name;
        if self.v != 1
            || self.auth_version != 1
            || self.fleet_id != registration.fleet_id
            || self.source_room_id != registration.reception_room_id
            || self.source_room_id == self.target_room_id
            || self.owner_dm_room_id == self.source_room_id
            || self.owner_dm_room_id == self.target_room_id
            || !valid_user(&self.requester_mxid, server)
            || !valid_user(&self.owner_mxid, server)
            || !valid_room(&self.target_room_id, server)
            || !valid_room(&self.owner_dm_room_id, server)
            || self.owner_mxid == registration.approval_bot_mxid
            || self
                .owner_mxid
                .starts_with(&format!("@{}_", registration.fleet_id))
            || u64::from(self.requested_tokens) == 0
            || self.rate_per_day.is_some_and(|n| u64::from(n) == 0)
            || !self.source_event_id.starts_with('$')
            || self.source_event_id.len() < 2
            || self.source_event_id.len() > 255
            || self
                .source_event_id
                .chars()
                .any(|c| c.is_whitespace() || c.is_control())
        {
            return Err(InvalidInput("invalid scoped project request"));
        }
        Ok(())
    }
    pub fn digest(&self) -> Result<String, InvalidInput> {
        canonical::digest(&serde_json::to_value(self).map_err(|_| InvalidInput("invalid request"))?)
    }
    pub fn engagement_id(&self) -> Result<String, InvalidInput> {
        let key = serde_json::to_vec(&[&self.fleet_id, &self.request_id])
            .map_err(|_| InvalidInput("invalid identity"))?;
        Ok(format!("en_{}", &project::hash(&key)[..32]))
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RoomObservation {
    pub room_id: String,
    pub joined: BTreeSet<String>,
    pub invite_only: bool,
    pub encryption: Option<String>,
    pub powers: BTreeMap<String, i64>,
    pub default_power: i64,
    pub invite_power: i64,
    pub binding: Option<Value>,
    pub name: Option<String>,
}
impl RoomObservation {
    fn power(&self, user: &str) -> i64 {
        self.powers.get(user).copied().unwrap_or(self.default_power)
    }
}
#[derive(Debug, Clone, Serialize)]
pub struct SourceObservation {
    pub event_id: String,
    pub room_id: String,
    pub sender: String,
    pub event_type: String,
    pub content: Value,
}
#[derive(Debug, Clone, Serialize)]
pub struct RequestObservation {
    pub registration_generation: u64,
    pub observed_at_ms: u64,
    pub source: SourceObservation,
    pub reception: RoomObservation,
    pub project: RoomObservation,
    pub owner_room: RoomObservation,
}

/// Cannot be deserialized or constructed from caller-provided boolean flags.
#[derive(Debug, Clone)]
pub struct VerifiedRequest {
    registration: Registration,
    request: ProjectRequest,
    observed_at_ms: u64,
    project_name: Option<String>,
    audit: Value,
}
impl VerifiedRequest {
    /// Private audit evidence. Never included in an Engagement projection.
    pub fn audit(&self) -> &Value {
        &self.audit
    }
    pub fn request(&self) -> &ProjectRequest {
        &self.request
    }
    pub fn registration(&self) -> &Registration {
        &self.registration
    }
    pub fn project_name(&self) -> Option<&str> {
        self.project_name.as_deref()
    }
    pub fn check_fresh(&self, now_ms: u64) -> Result<(), InvalidInput> {
        if now_ms > JSON_SAFE_MAX
            || now_ms < self.observed_at_ms
            || now_ms - self.observed_at_ms > 30_000
        {
            return Err(InvalidInput("project authority observation expired"));
        }
        Ok(())
    }
}

pub fn verify_request(
    registration: &Registration,
    request: ProjectRequest,
    observation: RequestObservation,
) -> Result<VerifiedRequest, InvalidInput> {
    request.validate(registration)?;
    let audit = serde_json::to_value(&observation)
        .map_err(|_| InvalidInput("invalid authority observation"))?;
    if serde_json::to_vec(&audit)
        .map_err(|_| InvalidInput("invalid authority observation"))?
        .len()
        > 64 * 1024
    {
        return Err(InvalidInput("authority observation exceeds 64 KiB"));
    }
    let bad = || InvalidInput("project authority observation does not match");
    let RequestObservation {
        registration_generation,
        observed_at_ms,
        source,
        reception,
        project,
        owner_room,
    } = observation;
    if registration_generation != registration.generation
        || observed_at_ms > JSON_SAFE_MAX
        || source.event_id != request.source_event_id
        || source.room_id != request.source_room_id
        || source.sender != request.requester_mxid
        || source.event_type != "com.hagency.engagement.request.v1"
    {
        return Err(bad());
    }
    let mut content = source.content;
    let object = content.as_object_mut().ok_or_else(bad)?;
    // Private room metadata is intentionally absent from public source events.
    object.insert(
        "ownerDmRoomId".into(),
        Value::String(request.owner_dm_room_id.clone()),
    );
    object.insert("sourceEventId".into(), Value::String(source.event_id));
    let observed_request: ProjectRequest = serde_json::from_value(content).map_err(|_| bad())?;
    observed_request.validate(registration)?;
    if observed_request.digest()? != request.digest()? {
        return Err(bad());
    }
    let representative = &registration.representative_mxid;
    let owner = &request.owner_mxid;
    let requester = &request.requester_mxid;
    if reception.room_id != request.source_room_id
        || !reception.invite_only
        || reception.encryption.is_some()
        || !reception.joined.contains(requester)
        || !reception.joined.contains(representative)
        || project.room_id != request.target_room_id
        || !project.invite_only
        || project.encryption.is_some()
        || !project.joined.contains(requester)
        || !project.joined.contains(owner)
        || !project.joined.contains(representative)
        || project.power(requester) < project.invite_power
        || project.power(owner) < 100
        || project.power(representative) < project.invite_power
    {
        return Err(bad());
    }
    let binding = project.binding.as_ref().ok_or_else(bad)?;
    if binding["v"] != 1
        || binding["purpose"] != "project"
        || binding["fleetId"] != request.fleet_id
        || binding["projectId"] != request.target_project_id
        || binding["ownerMxid"] != *owner
        || binding["authVersion"] != 1
    {
        return Err(bad());
    }
    if owner_room.room_id != request.owner_dm_room_id
        || !owner_room.invite_only
        || owner_room.encryption.as_deref() != Some("m.megolm.v1.aes-sha2")
        || owner_room.joined
            != BTreeSet::from([owner.clone(), registration.approval_bot_mxid.clone()])
    {
        return Err(bad());
    }
    Ok(VerifiedRequest {
        registration: registration.clone(),
        request,
        observed_at_ms,
        project_name: project
            .name
            .map(|s| s.trim().chars().take(255).collect::<String>())
            .filter(|s| !s.is_empty()),
        audit,
    })
}
