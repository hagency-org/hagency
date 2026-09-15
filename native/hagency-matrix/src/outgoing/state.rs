//! Private, encrypted journal data. These types are never external proof inputs.
use crate::Error;
use hagency_core::{
    canonical,
    replies::{ReplyDeliveryObservation, ReplyRoute, matrix_event},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub(crate) const MAX_RECEIPTS: usize = 64;
pub(crate) const MAX_EVENT: usize = 60 * 1024;
pub(crate) const MAX_QUERY: usize = 256 * 1024;
pub(crate) const MAX_WRITES: usize = 17;
pub(crate) const MAX_ATTEMPT: usize = 1024 * 1024;
#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) enum Kind {
    Final,
    Notice,
    File,
}
#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) enum Phase {
    BeforeBegin,
    Ready,
    QueryPrepared,
    CryptoApplying,
    WritePossible,
    ResponseStored,
    Complete,
    Quarantined,
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Write {
    pub event_type: String,
    pub transaction_id: String,
    pub body: String,
    pub digest: String,
    pub response: Option<Value>,
    pub room: bool,
}
impl Write {
    pub fn new(
        event_type: String,
        transaction_id: String,
        body: Value,
        room: bool,
    ) -> Result<Self, Error> {
        let body = encode(&body, if room { MAX_EVENT } else { MAX_QUERY })?;
        let digest = hash(body.as_bytes());
        Ok(Self {
            event_type,
            transaction_id,
            body,
            digest,
            response: None,
            room,
        })
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Attempt {
    pub kind: Kind,
    pub id: String,
    pub fence: u64,
    pub domain_digest: String,
    pub route: ReplyRoute,
    pub transaction_id: String,
    pub content: Value,
    pub content_digest: String,
    pub identity: String,
    pub joined: BTreeSet<String>,
    pub phase: Phase,
    pub query_id: Option<String>,
    pub query_body: Option<String>,
    pub query_response: Option<Value>,
    pub keys_digest: Option<String>,
    pub writes: Vec<Write>,
    pub index: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<crate::sdk::file_publication::Binding>,
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Receipt {
    pub kind: Kind,
    pub id: String,
    pub fence: u64,
    pub attempt_digest: String,
}
impl Attempt {
    pub fn validate(&self, identity: &str, user: &str, device: &str) -> Result<(), Error> {
        if self.identity != identity
            || self.route.sender_mxid != user
            || self.route.device_id != device
            || self.id.is_empty()
            || self.id.len() > 128
            || self.fence == 0
            || !digest(&self.domain_digest)
            || self.transaction_id.is_empty()
            || self.transaction_id.len() > 128
            || self.joined.len() > 16
            || !self.joined.contains(user)
            || self.content_digest != hash(encode(&self.content, MAX_EVENT)?.as_bytes())
            || self.writes.len() > MAX_WRITES
            || self.index > self.writes.len()
            || self
                .query_body
                .as_ref()
                .is_some_and(|b| b.len() > MAX_QUERY)
            || self
                .query_id
                .as_ref()
                .is_some_and(|v| v.is_empty() || v.len() > 255)
        {
            return Err(Error::Storage);
        }
        self.validate_route()?;
        let expected_relation = self.route.thread_root.as_ref().map(|root|
            serde_json::json!({"rel_type":"m.thread","event_id":root,"is_falling_back":true,"m.in_reply_to":{"event_id":root}}));
        if self.content.get("m.relates_to") != expected_relation.as_ref()
            || self.content["msgtype"]
                != match self.kind {
                    Kind::Notice => "m.notice",
                    Kind::Final => "m.text",
                    Kind::File => "m.file",
                }
            || (self.kind == Kind::File) != self.file.is_some()
            || (self.kind == Kind::File && !self.route.encrypted)
        {
            return Err(Error::Storage);
        }
        if let Some(v) = &self.query_response {
            encode(v, MAX_QUERY)?;
        }
        self.validate_phase()?;
        for (index, w) in self.writes.iter().enumerate() {
            if w.body.len() > if w.room { MAX_EVENT } else { MAX_QUERY }
                || w.digest != hash(w.body.as_bytes())
                || w.transaction_id.is_empty()
                || w.transaction_id.len() > 255
                || w.room != (index + 1 == self.writes.len())
                || (w.room
                    && (w.transaction_id != self.transaction_id
                        || w.event_type
                            != if self.route.encrypted {
                                "m.room.encrypted"
                            } else {
                                "m.room.message"
                            }))
                || (!w.room && (!self.route.encrypted || w.event_type != "m.room.encrypted"))
                || (!self.route.encrypted && w.digest != self.content_digest)
            {
                return Err(Error::Storage);
            }
            if let Some(response) = &w.response {
                encode(response, 4096)?;
                if w.room {
                    let event = response
                        .get("event_id")
                        .and_then(Value::as_str)
                        .ok_or(Error::Storage)?;
                    matrix_event(event).map_err(|_| Error::Storage)?;
                    if response.as_object().is_none_or(|o| o.len() != 1) {
                        return Err(Error::Storage);
                    }
                } else if response.as_object().is_none_or(|o| !o.is_empty()) {
                    return Err(Error::Storage);
                }
            }
        }
        encode(
            &serde_json::to_value(self).map_err(|_| Error::Storage)?,
            MAX_ATTEMPT,
        )?;
        Ok(())
    }
    fn validate_route(&self) -> Result<(), Error> {
        use hagency_core::{
            JSON_SAFE_MAX,
            project::identifier,
            replies::{RoomPrivacy, matrix_room, matrix_user},
        };
        let r = &self.route;
        for id in [
            &r.session_id,
            &r.engagement_id,
            &r.fleet_id,
            &r.project_id,
            &r.device_id,
        ] {
            identifier(id, 512).map_err(|_| Error::Storage)?;
        }
        if [
            r.session_generation,
            r.registration_generation,
            r.transport_generation,
            r.room_generation,
        ]
        .into_iter()
        .any(|g| g == 0 || g > JSON_SAFE_MAX)
        {
            return Err(Error::Storage);
        }
        matrix_user(&r.sender_mxid, &r.server_name).map_err(|_| Error::Storage)?;
        matrix_room(&r.room_id, &r.server_name).map_err(|_| Error::Storage)?;
        ruma::UserId::parse(&r.owner_mxid).map_err(|_| Error::Storage)?;
        for user in &self.joined {
            ruma::UserId::parse(user).map_err(|_| Error::Storage)?;
        }
        if let Some(root) = &r.thread_root {
            matrix_event(root).map_err(|_| Error::Storage)?;
        }
        if let RoomPrivacy::Direct { human_mxid } = &r.privacy
            && (!r.encrypted
                || human_mxid == &r.sender_mxid
                || !self.joined.contains(human_mxid)
                || self.joined.len() != 2)
        {
            return Err(Error::Storage);
        }
        Ok(())
    }
    fn validate_phase(&self) -> Result<(), Error> {
        let has_query = self.query_id.is_some() && self.query_body.is_some();
        let no_query = self.query_id.is_none()
            && self.query_body.is_none()
            && self.query_response.is_none()
            && self.keys_digest.is_none();
        if has_query {
            let query = serde_json::json!({"device_keys": self.joined.iter().map(|u|(u.clone(),serde_json::json!([]))).collect::<serde_json::Map<_,_>>()});
            if !self.route.encrypted
                || self.query_body.as_deref() != Some(encode(&query, MAX_QUERY)?.as_str())
            {
                return Err(Error::Storage);
            }
        } else if !no_query {
            return Err(Error::Storage);
        }
        if let Some(keys) = &self.keys_digest
            && self
                .query_response
                .as_ref()
                .map(|v| encode(v, MAX_QUERY).map(|s| hash(s.as_bytes())))
                .transpose()?
                .as_ref()
                != Some(keys)
        {
            return Err(Error::Storage);
        }
        let empty = self.writes.is_empty() && self.index == 0;
        let prepared = !self.writes.is_empty()
            && self.index < self.writes.len()
            && if self.route.encrypted {
                has_query && self.keys_digest.is_some()
            } else {
                no_query && self.writes.len() == 1
            };
        let valid = match self.phase {
            Phase::BeforeBegin => empty && no_query,
            Phase::Ready => prepared || (empty && no_query && self.route.encrypted),
            Phase::QueryPrepared => {
                empty && has_query && self.query_response.is_none() && self.keys_digest.is_none()
            }
            Phase::CryptoApplying | Phase::Quarantined => {
                empty && has_query && self.query_response.is_some() && self.keys_digest.is_none()
            }
            Phase::WritePossible => prepared,
            Phase::ResponseStored => prepared && !self.writes[self.index].room,
            Phase::Complete => {
                prepared && self.index + 1 == self.writes.len() && self.writes[self.index].room
            }
        };
        if !valid {
            return Err(Error::Storage);
        }
        for (index, write) in self.writes.iter().enumerate() {
            let accepted = index < self.index
                || (index == self.index
                    && matches!(self.phase, Phase::ResponseStored | Phase::Complete));
            if write.response.is_some() != accepted {
                return Err(Error::Storage);
            }
        }
        Ok(())
    }
    pub fn observation(&self) -> Result<ReplyDeliveryObservation, Error> {
        if self.phase != Phase::Complete {
            return Err(Error::OutcomeUnknown);
        }
        let write = self
            .writes
            .last()
            .filter(|w| w.room)
            .ok_or(Error::Storage)?;
        let event = write
            .response
            .as_ref()
            .and_then(|v| v.get("event_id"))
            .and_then(Value::as_str)
            .ok_or(Error::Storage)?;
        matrix_event(event).map_err(|_| Error::Wire)?;
        Ok(ReplyDeliveryObservation {
            transaction_id: self.transaction_id.clone(),
            digest: self.domain_digest.clone(),
            server_name: self.route.server_name.clone(),
            room_id: self.route.room_id.clone(),
            sender_mxid: self.route.sender_mxid.clone(),
            device_id: self.route.device_id.clone(),
            event_id: event.into(),
            encrypted: write.event_type == "m.room.encrypted",
        })
    }
    pub fn receipt(&self) -> Result<Receipt, Error> {
        self.observation()?;
        Ok(Receipt {
            kind: self.kind,
            id: self.id.clone(),
            fence: self.fence,
            attempt_digest: canonical::transport_digest(
                &serde_json::to_value(self).map_err(|_| Error::Storage)?,
            )
            .map_err(|_| Error::Storage)?,
        })
    }
}
pub(crate) fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
pub(crate) fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
pub(crate) fn encode(value: &Value, max: usize) -> Result<String, Error> {
    let value = canonical::encode_transport(value).map_err(|_| Error::Wire)?;
    if value.len() > max {
        return Err(Error::Capacity);
    }
    Ok(value)
}
pub(crate) enum Command {
    Read,
    Start(Box<Attempt>),
    StartFile(Box<crate::sdk::file_publication::Start>),
    Begun,
    Query,
    Encrypt(Value),
    Possible(usize),
    Accept(usize, Value),
    Settle,
}
pub(crate) struct View {
    pub attempt: Option<Attempt>,
    pub receipts: Vec<Receipt>,
}
