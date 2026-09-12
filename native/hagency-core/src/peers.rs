//! Runtime peer requests are data. Actor identity and admissible destination
//! sessions are derived by the domain writer from current conversation authority.
use crate::{InvalidInput, canonical, project::identifier, tasks::text};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PeerKind {
    Request,
    Response,
    Notification,
}
impl PeerKind {
    pub fn wakes(self) -> bool {
        self != Self::Notification
    }
}
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PeerPriority {
    #[default]
    Normal,
    High,
    Urgent,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PeerSend {
    pub call_id: String,
    pub conversation_id: String,
    pub recipient_session_ids: Vec<String>,
    pub kind: PeerKind,
    #[serde(default)]
    pub priority: PeerPriority,
    pub summary: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub data: Value,
}
impl PeerSend {
    pub fn validate(&self) -> Result<(), InvalidInput> {
        identifier(&self.call_id, 512)?;
        identifier(&self.conversation_id, 128)?;
        text(&self.summary, 1024)?;
        if self.body.len() > 32 * 1024 || self.body.contains('\0') {
            return Err(InvalidInput("invalid peer body"));
        }
        if self.recipient_session_ids.is_empty() || self.recipient_session_ids.len() > 64 {
            return Err(InvalidInput("peer message requires 1..64 recipients"));
        }
        let mut seen = std::collections::BTreeSet::new();
        for id in &self.recipient_session_ids {
            identifier(id, 128)?;
            if !seen.insert(id) {
                return Err(InvalidInput("duplicate peer recipient"));
            }
        }
        if canonical::encode_payload(&self.data)?.len() > 32 * 1024 {
            return Err(InvalidInput("peer data exceeds 32 KiB"));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerMessage {
    pub sequence: u64,
    pub id: String,
    pub conversation_id: String,
    pub source_session_id: String,
    pub source_engagement_id: String,
    pub source_dispatch_id: String,
    pub source_task_id: Option<String>,
    pub recipient_session_ids: Vec<String>,
    pub kind: PeerKind,
    pub priority: PeerPriority,
    pub summary: String,
    pub body: String,
    pub data: Value,
    pub received_at: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerReceipt {
    pub id: String,
    pub sequence: u64,
    pub replayed: bool,
    pub recipients: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerInboxItem {
    pub message: PeerMessage,
    pub wake: bool,
}
