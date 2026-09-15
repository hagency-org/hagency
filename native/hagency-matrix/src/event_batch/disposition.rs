//! Private terminal source receipts. They confer no event/room/crypto authority.
use super::{Error, MAX_TIMELINE, Receipt};
use hagency_core::canonical;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Source {
    room: String,
    event: Option<String>,
    raw: String,
    immutable: String,
}
fn digest(value: &Value) -> Result<String, Error> {
    canonical::transport_digest(value).map_err(|_| Error::Wire)
}
fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}
impl Source {
    pub(super) fn new(room: &str, raw: &Value) -> Result<Self, Error> {
        let mut immutable = raw.clone();
        // unsigned age/bundled presentation can change between sync responses.
        // The complete original raw fingerprint still binds this batch's coverage.
        if let Some(object) = immutable.as_object_mut() {
            object.remove("unsigned");
        }
        let event = raw
            .get("event_id")
            .and_then(Value::as_str)
            .filter(|id| ruma::EventId::parse(id).is_ok())
            .map(|id| digest(&json!([room, id])))
            .transpose()?;
        Ok(Self {
            room: digest(&json!(room))?,
            event,
            raw: digest(raw)?,
            immutable: digest(&immutable)?,
        })
    }
    fn matches_key(&self, other: &Self) -> bool {
        self.room == other.room
            && match (&self.event, &other.event) {
                (Some(a), Some(b)) => a == b,
                (None, None) => self.raw == other.raw,
                _ => false,
            }
    }
    fn validate(&self) -> Result<(), Error> {
        if !valid_digest(&self.room)
            || !valid_digest(&self.raw)
            || !valid_digest(&self.immutable)
            || self.event.as_ref().is_some_and(|id| !valid_digest(id))
        {
            return Err(Error::Storage);
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Rejection {
    Malformed,
    Unsupported,
    CryptoIneligible,
    PlaintextEncrypted,
    SourceConflict,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Decision {
    Candidate { index: usize },
    Rejected { reason: Rejection },
    NotTarget,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Disposition {
    source: Source,
    sdk_observation: String,
    pub decision: Decision,
}
impl Disposition {
    pub(super) fn new(source: Source, proof: Value, decision: Decision) -> Result<Self, Error> {
        Ok(Self {
            source,
            sdk_observation: digest(&proof)?,
            decision,
        })
    }
    pub(super) fn prior(source: &Source, history: &[Receipt]) -> Option<Decision> {
        history
            .iter()
            .flat_map(|r| r.dispositions.iter().flatten())
            .find(|r| {
                r.source.matches_key(source) && !matches!(r.decision, Decision::Candidate { .. })
            })
            .map(|r| {
                if source.immutable != r.source.immutable {
                    Decision::Rejected {
                        reason: Rejection::SourceConflict,
                    }
                } else {
                    r.decision.clone()
                }
            })
    }
    pub(super) fn matches(&self, room: &str, raw: &Value) -> Result<bool, Error> {
        let source = Source::new(room, raw)?;
        Ok(self.source.room == source.room
            && self.source.event == source.event
            && self.source.raw == source.raw
            && self.source.immutable == source.immutable)
    }
}
pub(super) fn validate(
    values: &[Disposition],
    candidates: usize,
    filtered: usize,
) -> Result<(), Error> {
    if values.len() > MAX_TIMELINE {
        return Err(Error::Storage);
    }
    let mut next = 0;
    let mut ignored = 0;
    for value in values {
        value.source.validate()?;
        if !valid_digest(&value.sdk_observation) {
            return Err(Error::Storage);
        }
        match value.decision {
            Decision::Candidate { index } if index == next => next += 1,
            Decision::Candidate { .. } => return Err(Error::Storage),
            Decision::NotTarget => ignored += 1,
            Decision::Rejected { .. } => {}
        }
    }
    if next != candidates || ignored != filtered {
        return Err(Error::Storage);
    }
    Ok(())
}
pub(super) fn rejected(values: Option<&[Disposition]>) -> usize {
    values
        .unwrap_or_default()
        .iter()
        .filter(|v| matches!(v.decision, Decision::Rejected { .. }))
        .count()
}

/// Every raw joined timeline entry must be represented exactly once. Room/order
/// comes from the authenticated sync; no malformed event needs a fabricated ID.
pub(super) fn raw_events(raw: &Value) -> Result<Vec<(String, Value)>, Error> {
    let mut all = vec![];
    if let Some(joined) = raw.pointer("/rooms/join") {
        let mut rooms = joined
            .as_object()
            .ok_or(Error::Wire)?
            .iter()
            .collect::<Vec<_>>();
        rooms.sort_by_key(|(room, _)| *room);
        for (room, update) in rooms {
            if update
                .pointer("/timeline/limited")
                .is_some_and(|v| v != &Value::Bool(false))
            {
                return Err(Error::Unsupported);
            }
            if let Some(events) = update.pointer("/timeline/events") {
                for event in events.as_array().ok_or(Error::Wire)? {
                    if all.len() >= MAX_TIMELINE {
                        return Err(Error::Capacity);
                    }
                    all.push((room.clone(), event.clone()));
                }
            }
        }
    }
    Ok(all)
}
