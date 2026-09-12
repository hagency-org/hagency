//! Private delivery metadata, never a restored current send authority.
use crate::{
    Error,
    approval_batch::Target,
    outgoing::state::{self, Write},
};
use hagency_core::{
    JSON_SAFE_MAX, canonical,
    project::identifier,
    replies::{matrix_event, matrix_room, matrix_user},
};
use hagency_store::PrivateApprovalCard;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeSet;
pub(crate) const MAX_RECEIPTS: usize = 64;
pub(crate) const MAX_START: usize = 64 * 1024;
pub(crate) const MAX_CARD: usize = 48 * 1024;
#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Debug)]
pub(crate) enum Phase {
    Prepared,
    QueryPrepared,
    CryptoApplying,
    Ready,
    WritePossible,
    ResponseStored,
    Complete,
    Quarantined,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Frozen {
    pub target: Target,
    pub cutoff: u64,
    pub content: Value,
    pub content_digest: String,
    pub digest: String,
}
impl Frozen {
    pub fn new(card: &PrivateApprovalCard) -> Result<Self, Error> {
        let mut value = Self {
            target: Target(card.target().clone()),
            cutoff: card.owner_expires_at(),
            content: card.content().clone(),
            content_digest: state::hash(state::encode(card.content(), MAX_CARD)?.as_bytes()),
            digest: String::new(),
        };
        value.digest = value.hash()?;
        value.validate()?;
        size(&value, MAX_START)?;
        Ok(value)
    }
    fn hash(&self) -> Result<String, Error> {
        hash(&json!([
            "approval-card-v1",
            self.target,
            self.cutoff,
            self.content
        ]))
    }
    pub fn validate(&self) -> Result<(), Error> {
        let t = &self.target.0;
        let a = &t.authority;
        for id in [&a.engagement_id, &a.fleet_id, &a.project_id, &t.device_id] {
            identifier(id, 512).map_err(|_| Error::Storage)?;
        }
        matrix_room(&a.room_id, &a.server_name).map_err(|_| Error::Storage)?;
        matrix_room(&a.project_room_id, &a.server_name).map_err(|_| Error::Storage)?;
        matrix_user(&a.bot_mxid, &a.server_name).map_err(|_| Error::Storage)?;
        let owner: ruma::OwnedUserId = a
            .owner_mxid
            .as_str()
            .try_into()
            .map_err(|_| Error::Storage)?;
        matrix_user(&a.owner_mxid, owner.server_name().as_str()).map_err(|_| Error::Storage)?;
        if a.room_id == a.project_room_id
            || a.bot_mxid == a.owner_mxid
            || [
                a.registration_generation,
                t.room_generation,
                t.binding_generation,
                t.expires_at,
                self.cutoff,
            ]
            .iter()
            .any(|v| *v == 0 || *v > JSON_SAFE_MAX)
            || self.cutoff > t.expires_at
            || !request_id(&t.request_id)
            || !state::digest(&t.request_digest)
            || self.content_digest
                != state::hash(state::encode(&self.content, MAX_CARD)?.as_bytes())
            || self.hash()? != self.digest
        {
            return Err(Error::Storage);
        }
        state::encode(&self.content, MAX_CARD)?;
        let d = &self.content["com.agentchat.approval"];
        let mut expected = vec!["approve_once"];
        if t.reusable_scope {
            expected.extend(["approve_task", "approve_always"]);
        }
        expected.push("deny");
        let actions = d["actions"].as_array().ok_or(Error::Storage)?;
        if self.content.as_object().is_none_or(|m| m.len() != 3)
            || self.content["msgtype"] != "com.agentchat.approval.request.v1"
            || self.content["body"].as_str().is_none_or(str::is_empty)
            || d["version"] != 1
            || d["kind"] != "request"
            || d["runtime"] != "codex"
            || d["agent"] != a.engagement_id
            || d["project"] != a.project_id
            || d["project_room_id"] != a.project_room_id
            || d["request_id"] != t.request_id
            || d["input_digest"] != t.request_digest
            || d["expires_at"] != self.cutoff
            || d["upstream_request_id"].as_str().is_none_or(str::is_empty)
            || d["tool_name"].as_str().is_none_or(str::is_empty)
            || !d["input_preview"].is_string()
            || t.reusable_scope != d.get("reusable_scope").is_some()
            || actions.len() != expected.len()
            || actions.iter().zip(expected).any(|(v, id)| v["id"] != id)
        {
            return Err(Error::Storage);
        }
        Ok(())
    }
    pub fn users(&self) -> Vec<String> {
        BTreeSet::from([
            self.target.0.authority.bot_mxid.clone(),
            self.target.0.authority.owner_mxid.clone(),
        ])
        .into_iter()
        .collect()
    }
    pub fn transaction(&self) -> String {
        format!("approval_{}", self.digest)
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Attempt {
    pub card: Frozen,
    pub identity: String,
    pub phase: Phase,
    pub query_id: Option<String>,
    pub query_body: Option<String>,
    pub query_response: Option<Value>,
    pub keys_digest: Option<String>,
    pub writes: Vec<Write>,
    pub index: usize,
}
impl Attempt {
    pub fn new(card: Frozen, identity: String) -> Self {
        Self {
            card,
            identity,
            phase: Phase::Prepared,
            query_id: None,
            query_body: None,
            query_response: None,
            keys_digest: None,
            writes: vec![],
            index: 0,
        }
    }
    pub fn query(&self) -> Value {
        json!({"device_keys": self.card.users().into_iter().map(|u|(u,json!([]))).collect::<serde_json::Map<_,_>>()})
    }
    pub fn validate(&self, identity: &str, user: &str, device: &str) -> Result<(), Error> {
        self.card.validate()?;
        if self.identity != identity
            || self.card.target.0.authority.bot_mxid != user
            || self.card.target.0.device_id != device
            || self.writes.len() > state::MAX_WRITES
            || self.index > self.writes.len()
        {
            return Err(Error::Storage);
        }
        let queried = self
            .query_id
            .as_ref()
            .is_some_and(|s| !s.is_empty() && s.len() <= 255)
            && self.query_body.as_deref()
                == Some(state::encode(&self.query(), state::MAX_QUERY)?.as_str());
        let no_query = self.query_id.is_none()
            && self.query_body.is_none()
            && self.query_response.is_none()
            && self.keys_digest.is_none();
        if !queried && !no_query {
            return Err(Error::Storage);
        }
        if let Some(v) = &self.query_response {
            state::encode(v, state::MAX_QUERY)?;
        }
        if let Some(d) = &self.keys_digest
            && self
                .query_response
                .as_ref()
                .map(|v| state::encode(v, state::MAX_QUERY).map(|s| state::hash(s.as_bytes())))
                .transpose()?
                .as_ref()
                != Some(d)
        {
            return Err(Error::Storage);
        }
        let empty = self.writes.is_empty() && self.index == 0;
        let prepared = !self.writes.is_empty()
            && self.index < self.writes.len()
            && queried
            && self.keys_digest.is_some();
        let valid = match self.phase {
            Phase::Prepared => empty && no_query,
            Phase::QueryPrepared => {
                empty && queried && self.query_response.is_none() && self.keys_digest.is_none()
            }
            Phase::CryptoApplying | Phase::Quarantined => {
                empty && queried && self.query_response.is_some() && self.keys_digest.is_none()
            }
            Phase::Ready | Phase::WritePossible => prepared,
            Phase::ResponseStored => prepared && !self.writes[self.index].room,
            Phase::Complete => {
                prepared && self.index + 1 == self.writes.len() && self.writes[self.index].room
            }
        };
        if !valid {
            return Err(Error::Storage);
        }
        let mut expected_recipients = BTreeSet::new();
        if prepared {
            let response = self.query_response.as_ref().ok_or(Error::Storage)?;
            let devices = response["device_keys"].as_object().ok_or(Error::Storage)?;
            if devices.keys().cloned().collect::<BTreeSet<_>>()
                != self.card.users().into_iter().collect()
            {
                return Err(Error::Storage);
            }
            let mut count = 0;
            for (u, map) in devices {
                let map = map.as_object().ok_or(Error::Storage)?;
                if map.is_empty() {
                    return Err(Error::Storage);
                }
                for d in map.keys() {
                    count += 1;
                    if count > 64 {
                        return Err(Error::Storage);
                    }
                    if u != user || d != device {
                        expected_recipients.insert((u.clone(), d.clone()));
                    }
                }
            }
            if !devices
                .get(user)
                .and_then(|m| m.get(device))
                .is_some_and(Value::is_object)
            {
                return Err(Error::Storage);
            }
        }
        let mut actual_recipients = BTreeSet::new();
        let mut transactions = BTreeSet::new();
        for (index, w) in self.writes.iter().enumerate() {
            let accepted = index < self.index
                || (index == self.index
                    && matches!(self.phase, Phase::ResponseStored | Phase::Complete));
            if w.response.is_some() != accepted
                || w.room != (index + 1 == self.writes.len())
                || w.event_type != "m.room.encrypted"
                || w.transaction_id.is_empty()
                || w.transaction_id.len() > 255
                || !transactions.insert(&w.transaction_id)
                || w.digest != state::hash(w.body.as_bytes())
                || (w.room && w.transaction_id != self.card.transaction())
                || w.body.len()
                    > if w.room {
                        state::MAX_EVENT
                    } else {
                        state::MAX_QUERY
                    }
            {
                return Err(Error::Storage);
            }
            let body: Value = serde_json::from_str(&w.body).map_err(|_| Error::Storage)?;
            if w.room {
                if body["algorithm"] != "m.megolm.v1.aes-sha2" {
                    return Err(Error::Storage);
                }
            } else {
                let messages = body["messages"].as_object().ok_or(Error::Storage)?;
                if messages.is_empty()
                    || messages.keys().any(|u| !self.card.users().contains(u))
                    || messages
                        .values()
                        .any(|v| v.as_object().is_none_or(|m| m.is_empty() || m.len() > 64))
                {
                    return Err(Error::Storage);
                }
                for (u, map) in messages {
                    for (d, body) in map.as_object().ok_or(Error::Storage)? {
                        if body["algorithm"] != "m.olm.v1.curve25519-aes-sha2"
                            || !actual_recipients.insert((u.clone(), d.clone()))
                        {
                            return Err(Error::Storage);
                        }
                    }
                }
            }
            if let Some(response) = &w.response {
                validate_response(w.room, response)?;
            }
        }
        if prepared && actual_recipients != expected_recipients {
            return Err(Error::Storage);
        }
        size(self, state::MAX_ATTEMPT)
    }
    pub fn receipt(&self) -> Result<Receipt, Error> {
        if self.phase != Phase::Complete {
            return Err(Error::OutcomeUnknown);
        }
        let event = self
            .writes
            .last()
            .and_then(|w| w.response.as_ref())
            .and_then(|v| v["event_id"].as_str())
            .ok_or(Error::Storage)?;
        Ok(Receipt {
            request_id: self.card.target.0.request_id.clone(),
            card_digest: self.card.digest.clone(),
            attempt_digest: hash(&serde_json::to_value(self).map_err(|_| Error::Storage)?)?,
            event_id: event.into(),
        })
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Receipt {
    pub request_id: String,
    pub card_digest: String,
    pub attempt_digest: String,
    pub event_id: String,
}
impl Receipt {
    pub fn validate(&self) -> Result<(), Error> {
        if !request_id(&self.request_id)
            || !state::digest(&self.card_digest)
            || !state::digest(&self.attempt_digest)
        {
            return Err(Error::Storage);
        }
        matrix_event(&self.event_id).map_err(|_| Error::Storage)
    }
}
pub(crate) struct View {
    pub attempt: Option<Attempt>,
    pub receipts: Vec<Receipt>,
}
pub(crate) fn validate_response(room: bool, value: &Value) -> Result<(), Error> {
    state::encode(value, 4096)?;
    let map = value.as_object().ok_or(Error::Wire)?;
    if room {
        if map.len() != 1 {
            return Err(Error::Wire);
        }
        matrix_event(value["event_id"].as_str().ok_or(Error::Wire)?).map_err(|_| Error::Wire)
    } else if map.is_empty() {
        Ok(())
    } else {
        Err(Error::Wire)
    }
}
pub(crate) fn hash(value: &Value) -> Result<String, Error> {
    canonical::transport_digest(value).map_err(|_| Error::Storage)
}
pub(crate) fn size(value: &impl Serialize, max: usize) -> Result<(), Error> {
    state::encode(
        &serde_json::to_value(value).map_err(|_| Error::Storage)?,
        max,
    )
    .map(|_| ())
}
pub(crate) fn request_id(id: &str) -> bool {
    id.strip_prefix("approval_").is_some_and(|s| {
        s.len() == 40
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}
