//! Final content is runtime data. Matrix routing and transport observations are
//! constructed only by authenticated host adapters, never decoded from HTTP.
use crate::{InvalidInput, JSON_SAFE_MAX, project::identifier, tasks::text};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RoomPrivacy {
    Group {},
    Direct { human_mxid: String },
}

#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct MatrixTransportObservation {
    pub engagement_id: String,
    pub registration_generation: u64,
    pub generation: u64,
    pub sender_mxid: String,
    pub device_id: String,
}

/// Host-only captured identity. No external endpoint can submit negative
/// transport evidence or choose which newer incarnation it should retire.
#[derive(Clone, Serialize)]
pub struct MatrixTransportInvalidation {
    pub expected: MatrixTransportObservation,
    pub reason: String,
}

#[derive(Clone, Serialize)]
pub struct MatrixTransportState {
    pub observation: MatrixTransportObservation,
    pub available: bool,
}

#[derive(Clone, Serialize)]
pub struct MatrixRoomState {
    pub generation: u64,
    pub available: bool,
}

#[derive(Clone, Serialize)]
pub struct MatrixRoomObservation {
    pub engagement_id: String,
    pub registration_generation: u64,
    pub transport_generation: u64,
    pub room_id: String,
    pub generation: u64,
    pub privacy: RoomPrivacy,
    pub joined: BTreeSet<String>,
    pub invite_only: bool,
    pub encrypted: bool,
}

/// An authenticated adapter must record loss/absence of room evidence. This is
/// a negative observation, never permission to infer a new privacy classification.
#[derive(Clone, Serialize)]
pub struct MatrixRoomInvalidation {
    pub engagement_id: String,
    pub registration_generation: u64,
    pub transport_generation: u64,
    pub room_id: String,
    pub generation: u64,
    pub reason: String,
}

/// A frozen host-only delivery address. Never return this through console or
/// runtime receipt APIs. Encrypted rooms require encrypted transport delivery.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReplyRoute {
    pub session_id: String,
    pub session_generation: u64,
    pub engagement_id: String,
    pub fleet_id: String,
    pub project_id: String,
    pub registration_generation: u64,
    pub server_name: String,
    pub room_id: String,
    pub room_generation: u64,
    pub sender_mxid: String,
    pub device_id: String,
    pub transport_generation: u64,
    pub owner_mxid: String,
    pub privacy: RoomPrivacy,
    pub encrypted: bool,
    pub thread_root: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FinalReply {
    pub call_id: String,
    pub body: String,
}
impl std::fmt::Debug for FinalReply {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FinalReply")
            .field("call_id", &self.call_id)
            .field("body_bytes", &self.body.len())
            .finish()
    }
}
impl FinalReply {
    pub fn validate(&self) -> Result<(), InvalidInput> {
        identifier(&self.call_id, 512)?;
        text(&self.body, 32 * 1024)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplyState {
    Pending,
    Claimed,
    Sending,
    Uncertain,
    Delivered,
    Cancelled,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct ReplyReceipt {
    pub id: String,
    pub task_id: String,
    pub execution_epoch: u64,
    pub state: ReplyState,
    pub replayed: bool,
}

/// Host transport capability; its secret is never persisted or exposed to a runner.
#[derive(Clone)]
pub struct ReplyClaim {
    pub id: String,
    pub fence: u64,
    pub secret: String,
}
#[derive(Clone, Serialize)]
pub struct ReplySend {
    pub id: String,
    pub transaction_id: String,
    pub digest: String,
    pub route: ReplyRoute,
    pub body: String,
}
#[derive(Clone, Serialize)]
pub struct ReplyDeliveryObservation {
    pub transaction_id: String,
    pub digest: String,
    pub server_name: String,
    pub room_id: String,
    pub sender_mxid: String,
    pub device_id: String,
    pub event_id: String,
    pub encrypted: bool,
}
impl ReplyDeliveryObservation {
    pub fn validate(&self) -> Result<(), InvalidInput> {
        identifier(&self.transaction_id, 128)?;
        if self.digest.len() != 64 || !self.digest.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(InvalidInput("invalid reply content digest"));
        }
        text(&self.device_id, 255)?;
        matrix_user(&self.sender_mxid, &self.server_name)?;
        matrix_room(&self.room_id, &self.server_name)?;
        matrix_event(&self.event_id)
    }
}
#[derive(Clone, Serialize)]
pub enum ReplyReconciliation {
    Delivered(ReplyDeliveryObservation),
    /// A host adapter proved no send was accepted; timeout alone is insufficient.
    NotSent {
        evidence: String,
    },
}

pub fn generation(value: u64) -> Result<(), InvalidInput> {
    if value == 0 || value > JSON_SAFE_MAX {
        return Err(InvalidInput("invalid Matrix route generation"));
    }
    Ok(())
}

pub fn matrix_user(id: &str, server: &str) -> Result<(), InvalidInput> {
    if id.len() > 255
        || !ruma_common::UserId::parse(id).is_ok_and(|id| id.server_name().as_str() == server)
    {
        return Err(InvalidInput("invalid scoped Matrix user"));
    }
    Ok(())
}
pub fn matrix_room(id: &str, server: &str) -> Result<(), InvalidInput> {
    if id.len() > 255
        || ruma_common::RoomId::parse(id).is_err()
        || id
            .split_once(':')
            .is_none_or(|(_, suffix)| suffix != server)
    {
        return Err(InvalidInput("invalid scoped Matrix room"));
    }
    Ok(())
}
pub fn matrix_event(id: &str) -> Result<(), InvalidInput> {
    if ruma_common::EventId::parse(id).is_err() || id.len() > 255 {
        return Err(InvalidInput("invalid Matrix event"));
    }
    Ok(())
}
