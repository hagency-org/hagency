//! Source data is not proof of authentication. Only the authenticated transport
//! adapter constructs InboundMessage; it deliberately cannot deserialize HTTP input.
use crate::{
    InvalidInput, canonical,
    tasks::{clock, text},
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
pub struct InboundMessage {
    pub server_name: String,
    pub room_id: String,
    pub event_id: String,
    pub sender_mxid: String,
    pub thread_root: Option<String>,
    pub body: String,
    pub kind: String,
    pub origin_ts: u64,
}
impl InboundMessage {
    pub fn validate(&self) -> Result<(), InvalidInput> {
        ruma_common::ServerName::parse(&self.server_name)
            .map_err(|_| InvalidInput("invalid message server"))?;
        ruma_common::RoomId::parse(&self.room_id)
            .map_err(|_| InvalidInput("invalid message room"))?;
        ruma_common::EventId::parse(&self.event_id)
            .map_err(|_| InvalidInput("invalid source event"))?;
        ruma_common::UserId::parse(&self.sender_mxid)
            .map_err(|_| InvalidInput("invalid source sender"))?;
        if let Some(root) = &self.thread_root {
            ruma_common::EventId::parse(root).map_err(|_| InvalidInput("invalid source thread"))?;
        }
        text(&self.body, 32 * 1024)?;
        text(&self.kind, 64)?;
        clock(self.origin_ts)?;
        Ok(())
    }
    pub fn source_key(&self) -> Result<String, InvalidInput> {
        canonical::digest(&serde_json::json!([
            self.server_name,
            self.room_id,
            self.event_id
        ]))
    }
}
#[derive(Debug, Clone, Serialize)]
pub struct MessageTarget {
    pub session_id: String,
    pub wake: bool,
}
/// Read projection. Deserializing this object conveys no ingress authority.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub sequence: u64,
    pub source_key: String,
    pub server_name: String,
    pub room_id: String,
    pub event_id: String,
    pub sender_mxid: String,
    pub thread_root: Option<String>,
    pub body: String,
    pub kind: String,
    pub origin_ts: u64,
    pub received_at: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboxItem {
    pub message: Message,
    pub wake: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageReceipt {
    pub sequence: u64,
    pub created: bool,
    pub projected: usize,
}
