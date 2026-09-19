//! The redacted public status notice (ADR-137): a content-free status word
//! in the project room. Its own validator — a sibling of `Frozen`, never a
//! widening of it — and never a channel for request material.
use crate::Error;
use hagency_core::{
    approvals::ApprovalRoomAuthority,
    project::identifier,
    replies::{matrix_room, matrix_user},
};
use hagency_store::PrivateApprovalCard;
use serde_json::{Value, json};

pub(crate) const MAX_NOTICE_BODY: usize = 512;
pub(crate) const NOTICE_MSGTYPE: &str = "com.agentchat.approval.status.v1";
pub(crate) const STATUS_KEY: &str = "com.agentchat.approval.status";

/// The exact status packet: three top-level keys, nothing else. The status
/// object carries only the status word and the agent/project identity pair
/// the destination re-derivation already binds — no request id, no digest,
/// no tool name, no preview, no scope key in any byte.
#[derive(Clone)]
pub(crate) struct PublicFrozen {
    agent: String,
    project: String,
    server_name: String,
    project_room_id: String,
    private_room_id: String,
    bot_mxid: String,
    body: String,
    digest: String,
}
impl PublicFrozen {
    pub fn new(card: &PrivateApprovalCard) -> Result<Self, Error> {
        let t = card.target();
        let a = &t.authority;
        let value = Self {
            agent: a.engagement_id.clone(),
            project: a.project_id.clone(),
            server_name: a.server_name.clone(),
            project_room_id: a.project_room_id.clone(),
            private_room_id: a.room_id.clone(),
            bot_mxid: a.bot_mxid.clone(),
            body: "An approval request is waiting for its owner.".to_owned(),
            digest: String::new(),
        };
        let mut value = value;
        value.digest = value.hash()?;
        value.validate()?;
        Ok(value)
    }
    fn hash(&self) -> Result<String, Error> {
        crate::approval_delivery::state::hash(&json!([
            "approval-status-v1",
            self.agent,
            self.project,
            self.project_room_id,
            self.body
        ]))
    }
    pub fn msgtype(&self) -> &str {
        NOTICE_MSGTYPE
    }
    /// Deterministic transaction id: one identity per notice content; the
    /// send is never re-attempted, so the id is never reused by this host.
    pub fn transaction(&self) -> String {
        format!("approval_status_{}", self.digest)
    }
    pub fn content(&self) -> Result<String, Error> {
        let value = json!({
            "msgtype": NOTICE_MSGTYPE,
            "body": self.body,
            STATUS_KEY: {
                "kind": "status",
                "version": 1,
                "state": "waiting_for_owner",
                "agent": self.agent,
                "project": self.project
            }
        });
        self.validate_packet(&value)?;
        serde_json::to_string(&value).map_err(|_| Error::Storage)
    }
    /// Destination agreement with the live rows: the re-derived authority
    /// must address this notice exactly (stale or caller-influenced state is
    /// refused here, never repaired).
    pub fn matches(&self, authority: &ApprovalRoomAuthority) -> bool {
        authority.server_name == self.server_name
            && authority.project_room_id == self.project_room_id
            && authority.bot_mxid == self.bot_mxid
            && authority.engagement_id == self.agent
            && authority.project_id == self.project
    }
    fn validate(&self) -> Result<(), Error> {
        for id in [&self.agent, &self.project] {
            identifier(id, 512).map_err(|_| Error::Storage)?;
        }
        matrix_room(&self.project_room_id, &self.server_name).map_err(|_| Error::Storage)?;
        matrix_room(&self.private_room_id, &self.server_name).map_err(|_| Error::Storage)?;
        matrix_user(&self.bot_mxid, &self.server_name).map_err(|_| Error::Storage)?;
        // Room distinctness: the status word never lands in the private
        // approval room, and the two rooms are never the same room.
        if self.project_room_id == self.private_room_id
            || self.body.is_empty()
            || self.body.len() > MAX_NOTICE_BODY
            || self.hash()? != self.digest
        {
            return Err(Error::Storage);
        }
        let value = json!({
            "msgtype": NOTICE_MSGTYPE,
            "body": self.body,
            STATUS_KEY: {
                "kind": "status",
                "version": 1,
                "state": "waiting_for_owner",
                "agent": self.agent,
                "project": self.project
            }
        });
        self.validate_packet(&value)
    }
    /// The packet contract as an exact shape: three top-level keys, the
    /// status word's five keys, and the bound identity pair — absence of
    /// request material is structural (no such key can appear), not a filter.
    fn validate_packet(&self, value: &Value) -> Result<(), Error> {
        let object = value.as_object().ok_or(Error::Storage)?;
        let status = value[STATUS_KEY].as_object().ok_or(Error::Storage)?;
        let keys: Vec<&str> = object.keys().map(String::as_str).collect();
        let status_keys: Vec<&str> = status.keys().map(String::as_str).collect();
        if keys.len() != 3
            || !keys.contains(&"msgtype")
            || !keys.contains(&"body")
            || !keys.contains(&STATUS_KEY)
            || status_keys.len() != 5
            || value["msgtype"] != NOTICE_MSGTYPE
            || value["body"].as_str() != Some(self.body.as_str())
            || value[STATUS_KEY]["kind"] != "status"
            || value[STATUS_KEY]["version"] != 1
            || value[STATUS_KEY]["state"] != "waiting_for_owner"
            || value[STATUS_KEY]["agent"] != self.agent
            || value[STATUS_KEY]["project"] != self.project
        {
            return Err(Error::Storage);
        }
        Ok(())
    }
}
