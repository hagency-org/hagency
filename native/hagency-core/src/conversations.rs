//! Route data never authenticates its creator. The domain writer checks the
//! current runner and allocation before admitting an internal conversation.
use crate::{
    InvalidInput,
    project::identifier,
    tasks::{SessionBinding, text},
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InternalKind {
    Internal,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InternalSessionBinding {
    pub kind: InternalKind,
    pub id: String,
    pub engagement_id: String,
    pub conversation_id: String,
}
impl InternalSessionBinding {
    pub fn validate(&self) -> Result<(), InvalidInput> {
        identifier(&self.id, 128)?;
        identifier(&self.engagement_id, 128)?;
        identifier(&self.conversation_id, 128)
    }
}
/// Existing native Matrix records are untagged. Both wire variants deny unknown
/// fields so mixed or unknown routes fail instead of acquiring another scope.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StoredSession {
    Matrix(SessionBinding),
    Internal(InternalSessionBinding),
}
impl StoredSession {
    pub fn validate(&self) -> Result<(), InvalidInput> {
        match self {
            Self::Matrix(b) => b.validate(),
            Self::Internal(b) => b.validate(),
        }
    }
    pub fn id(&self) -> &str {
        match self {
            Self::Matrix(b) => &b.id,
            Self::Internal(b) => &b.id,
        }
    }
    pub fn engagement_id(&self) -> &str {
        match self {
            Self::Matrix(b) => &b.engagement_id,
            Self::Internal(b) => &b.engagement_id,
        }
    }
    pub fn matrix(&self) -> Option<&SessionBinding> {
        if let Self::Matrix(b) = self {
            Some(b)
        } else {
            None
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationRequest {
    pub call_id: String,
    pub label: String,
    pub participant_engagements: Vec<String>,
}
impl ConversationRequest {
    pub fn validate(&self) -> Result<(), InvalidInput> {
        identifier(&self.call_id, 512)?;
        text(&self.label, 255)?;
        if self.participant_engagements.is_empty() || self.participant_engagements.len() > 64 {
            return Err(InvalidInput("conversation requires 1..64 participants"));
        }
        let mut seen = std::collections::BTreeSet::new();
        for id in &self.participant_engagements {
            identifier(id, 128)?;
            if !seen.insert(id) {
                return Err(InvalidInput("duplicate conversation participant"));
            }
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    pub id: String,
    pub label: String,
    pub creator_session_id: String,
    pub participants: Vec<InternalSessionBinding>,
    pub state: String,
    #[serde(default)]
    pub revision: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationResult {
    pub conversation: Conversation,
    pub replayed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationChange {
    pub call_id: String,
    pub expected_revision: u64,
    pub action: ConversationAction,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ConversationAction {
    Members {
        participant_engagements: Vec<String>,
    },
    Close {},
}
impl ConversationChange {
    pub fn validate(&self) -> Result<(), InvalidInput> {
        identifier(&self.call_id, 512)?;
        if self.expected_revision >= crate::JSON_SAFE_MAX {
            return Err(InvalidInput("conversation revision is out of range"));
        }
        if let ConversationAction::Members {
            participant_engagements,
        } = &self.action
        {
            ConversationRequest {
                call_id: self.call_id.clone(),
                label: "members".into(),
                participant_engagements: participant_engagements.clone(),
            }
            .validate()?;
        }
        Ok(())
    }
}
