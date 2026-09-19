use crate::{InvalidInput, JSON_SAFE_MAX, canonical};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const MAX_DELIVERY_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Lane {
    Matrix,
    Work,
}
impl Lane {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Matrix => "matrix",
            Self::Work => "work",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Transaction,
    Probe,
    Request,
}

/// An operator-injected fixture envelope for the native development boundary.
/// Production transport supplies authenticated provenance in a later phase.
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Delivery {
    pub binding: String,
    pub generation: u64,
    pub id: String,
    pub lane: Lane,
    pub kind: Kind,
    pub payload: Value,
}

impl Delivery {
    pub fn validate(&self) -> Result<(), InvalidInput> {
        for (value, max) in [(&self.binding, 128), (&self.id, 512)] {
            if value.is_empty() || value.len() > max || value.chars().any(char::is_control) {
                return Err(InvalidInput("invalid delivery identity"));
            }
        }
        if self.generation == 0 || self.generation > JSON_SAFE_MAX {
            return Err(InvalidInput("invalid registration generation"));
        }
        if (self.lane == Lane::Matrix) != (self.kind == Kind::Transaction)
            || !self.payload.is_object()
        {
            return Err(InvalidInput("invalid delivery lane or payload"));
        }
        if serde_json::to_vec(self)
            .map_err(|_| InvalidInput("invalid delivery"))?
            .len()
            > MAX_DELIVERY_BYTES
        {
            return Err(InvalidInput("delivery exceeds byte limit"));
        }
        canonical::encode(&self.payload)?;
        Ok(())
    }

    pub fn content_digest(&self) -> Result<String, InvalidInput> {
        canonical::digest(
            &serde_json::json!({"lane": self.lane, "kind": self.kind, "payload": self.payload}),
        )
    }
}

/// No payload, private owner room, credentials or processing authority in a receipt.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Receipt {
    pub id: String,
    pub lane: Lane,
    pub generation: u64,
    pub digest: String,
    pub received_at_ms: u64,
    pub state: CustodyState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CustodyState {
    Received,
}
