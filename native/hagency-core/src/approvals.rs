//! Host-only approval observations. Runtime data cannot construct authority.
use crate::{InvalidInput, JSON_SAFE_MAX, project::identifier, tasks::text};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

#[derive(Clone, Serialize)]
pub struct ApprovalRoomObservation {
    pub engagement_id: String,
    pub registration_generation: u64,
    pub generation: u64,
    pub room_id: String,
    pub device_id: String,
    pub joined: BTreeSet<String>,
    pub invite_only: bool,
    pub encrypted: bool,
    /// Missing/unknown state is negative evidence, never a cached safe room.
    pub available: bool,
}
#[derive(Clone, Serialize)]
pub struct HostApprovalContext {
    pub id: String,
    pub connection_id: String,
    pub thread_id: String,
    pub turn_id: String,
    pub workspace_resource: String,
    pub workspace: String,
    pub windows_paths: bool,
    pub environment_id: Option<String>,
    pub may_write: bool,
    pub yolo: bool,
}
impl HostApprovalContext {
    pub fn validate(&self) -> Result<(), InvalidInput> {
        for id in [
            &self.id,
            &self.connection_id,
            &self.thread_id,
            &self.turn_id,
            &self.workspace_resource,
        ] {
            identifier(id, 256)?;
        }
        text(&self.workspace, 8192)?;
        if let Some(id) = &self.environment_id {
            text(id, 8192)?;
        }
        Ok(())
    }
}
/// Exact upstream JSON-RPC ID data. Numeric and string IDs remain distinct.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ApprovalRpcId {
    Number(u64),
    String(String),
}
impl ApprovalRpcId {
    pub fn validate(&self) -> Result<(), InvalidInput> {
        match self {
            Self::Number(n) if *n <= JSON_SAFE_MAX => Ok(()),
            Self::String(s) => text(s, 255),
            _ => Err(InvalidInput("invalid approval upstream ID")),
        }
    }
}
#[derive(Clone, Serialize)]
pub struct HostApprovalRequest {
    pub context_id: String,
    pub upstream_id: ApprovalRpcId,
    pub item_id: String,
    pub method: String,
    pub params: Value,
    pub expires_at: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalChoice {
    Once,
    Task,
    Always,
    Deny,
}
#[derive(Clone, Serialize)]
pub struct OwnerVerdictObservation {
    pub request_id: String,
    pub request_digest: String,
    pub binding_generation: u64,
    pub server_name: String,
    pub room_id: String,
    pub sender_mxid: String,
    pub event_id: String,
    pub encrypted: bool,
    pub choice: ApprovalChoice,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalSummary {
    pub id: String,
    pub state: String,
    pub reusable_scope: bool,
    pub choice: Option<ApprovalChoice>,
}
/// Private owner card metadata; never a console or public room projection.
#[derive(Clone, Serialize)]
pub struct PrivateApproval {
    pub summary: ApprovalSummary,
    pub digest: String,
    pub room_id: String,
    pub owner_mxid: String,
    pub binding_generation: u64,
    pub method: String,
    pub params: Value,
    pub description: Option<String>,
    pub expires_at: u64,
}
/// Returned exactly once after persisting Applying. No actual native wire
/// response is generated here; the runtime adapter must support the decision.
#[derive(Clone, Serialize)]
pub struct ApprovalApplication {
    pub id: String,
    pub digest: String,
    pub connection_id: String,
    pub upstream_id: ApprovalRpcId,
    pub thread_id: String,
    pub turn_id: String,
    pub item_id: String,
    pub allow: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplicationOutcome {
    Applied,
    NotApplied,
    Unknown,
}
#[derive(Clone, Serialize)]
pub struct ApprovalApplicationObservation {
    pub application: ApprovalApplication,
    pub outcome: ApplicationOutcome,
    pub evidence: String,
}
#[derive(Clone, Debug, Serialize)]
pub struct GrantSummary {
    pub id: String,
    pub kind: String,
    pub mode: ApprovalChoice,
    pub revoked: bool,
}

impl ApprovalApplicationObservation {
    pub fn validate(&self) -> Result<(), InvalidInput> {
        identifier(&self.application.id, 128)?;
        text(&self.application.digest, 64)?;
        for id in [
            &self.application.connection_id,
            &self.application.thread_id,
            &self.application.turn_id,
            &self.application.item_id,
        ] {
            identifier(id, 256)?;
        }
        self.application.upstream_id.validate()?;
        text(&self.evidence, 4000)
    }
}

/// Current registration/project truth for the separate approval-bot transport.
/// This does not identify or replace an Agent's Matrix transport.
#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct ApprovalRoomAuthority {
    pub engagement_id: String,
    pub fleet_id: String,
    pub project_id: String,
    pub registration_generation: u64,
    pub server_name: String,
    pub room_id: String,
    pub project_room_id: String,
    pub owner_mxid: String,
    pub bot_mxid: String,
}
/// Exact shared snapshot captured before host I/O; its digest is a CAS token,
/// not evidence that a caller supplied Matrix document was authenticated.
#[derive(Clone, Serialize)]
pub struct ApprovalRoomCapture {
    pub available: bool,
    pub device_id: String,
    pub generation: u64,
    pub server_name: String,
    pub room_id: String,
    pub digest: String,
}
#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct ApprovalIntakeTarget {
    pub authority: ApprovalRoomAuthority,
    pub device_id: String,
    pub room_generation: u64,
    pub binding_generation: u64,
    pub request_id: String,
    pub request_digest: String,
    pub expires_at: u64,
    pub reusable_scope: bool,
}
/// Authenticated host boundary only. SDK proof remains private to its owner;
/// no deserializable or runtime-facing JSON can construct this observation.
#[derive(Clone, Serialize)]
pub struct ApprovalVerdictInput {
    pub target: ApprovalIntakeTarget,
    pub source_digest: String,
    pub verdict: OwnerVerdictObservation,
}
