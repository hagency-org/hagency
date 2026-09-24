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
    /// In a delegated dispatch: this entry was read from the agent's own room
    /// (a follow-up in the task's thread that addresses it), not handed over
    /// from the delegator. Absent everywhere else.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub follow_up: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageReceipt {
    pub sequence: u64,
    pub created: bool,
    pub projected: usize,
}
/// One frozen room message is cut into parts of this many characters, so the
/// window bound and the page bound are measured in content rather than in
/// however much JSON happens to fit (`router/src/conversations.ts`).
pub const DISCUSSION_PART: usize = 1000;
/// The frozen window's own bound: the discussion reaching back from the
/// request, at most this many parts.
pub const DISCUSSION_WINDOW: usize = 200;
/// One `read_conversation` page. Small on purpose: the agent follows `next`
/// until it is null, and reading is what advances its room position.
pub const DISCUSSION_PAGE: usize = 8;
/// Parts one body occupies. An empty body still occupies one part, so a
/// message can never be skipped by the window or page arithmetic.
pub fn discussion_parts(body: &str) -> usize {
    body.chars().count().div_ceil(DISCUSSION_PART).max(1)
}
/// One part of one frozen room message, with the speaker's identity attached.
/// `sender_name` is what the store knows the speaker as — an agent's
/// engagement name, or the project owner — and is absent for a participant it
/// holds no name for; the Matrix ID always identifies the speaker.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiscussionPart {
    pub event_id: String,
    pub sender: String,
    pub sender_name: Option<String>,
    pub timestamp: u64,
    pub thread_root: Option<String>,
    pub part: usize,
    pub parts: usize,
    pub body: String,
}
/// One page of the discussion frozen for one dispatch. `next` is the offset of
/// the following page, or null at the end of the window.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiscussionPage {
    pub messages: Vec<DiscussionPart>,
    pub next: Option<u64>,
    pub total_messages: u64,
    pub total_parts: u64,
}
