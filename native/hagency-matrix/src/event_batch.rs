//! Private authenticated journal DTOs. Only the owned SDK constructs proofs;
//! deserialization occurs solely after authenticated journal decryption.
use crate::{Error, wire};
use hagency_core::{canonical, ingress::*, messages::InboundMessage, replies::ReplyRoute};
use matrix_sdk_base::sync::SyncResponse;
use matrix_sdk_common::deserialized_responses::{
    AlgorithmInfo, TimelineEventKind, VerificationState,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
mod disposition;
use disposition::{Decision, Disposition, Rejection, Source};

pub(crate) const MAX_TIMELINE: usize = 100;
pub(crate) const MAX_TARGETS: usize = 64;
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Phase {
    Prepared,
    Applying,
    Derived,
    Quarantined,
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Batch {
    pub token: String,
    pub digest: String,
    pub sdk_identity: String,
    pub targets: Vec<ReplyRoute>,
    pub raw: Value,
    pub phase: Phase,
    pub reason: Option<String>,
    pub events: Vec<Event>,
    pub acknowledgements: Vec<Acknowledgement>,
    pub filtered: usize,
    #[serde(default)]
    pub dispositions: Option<Vec<Disposition>>,
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Event {
    pub route: ReplyRoute,
    input: Message,
    pub mentions: BTreeSet<String>,
    proof: Proof,
    #[serde(default)]
    pub(crate) attachment: Option<crate::attachments::Manifest>,
}
#[derive(Clone, Serialize, Deserialize)]
struct Message {
    server_name: String,
    room_id: String,
    event_id: String,
    sender_mxid: String,
    thread_root: Option<String>,
    body: String,
    kind: String,
    origin_ts: u64,
}
impl Message {
    fn observation(&self) -> InboundMessage {
        InboundMessage {
            server_name: self.server_name.clone(),
            room_id: self.room_id.clone(),
            event_id: self.event_id.clone(),
            sender_mxid: self.sender_mxid.clone(),
            thread_root: self.thread_root.clone(),
            body: self.body.clone(),
            kind: self.kind.clone(),
            origin_ts: self.origin_ts,
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Proof {
    Plain,
    Verified {
        sender: String,
        device: String,
        session: String,
    },
}
impl Event {
    pub(crate) fn attachment_observation(
        &self,
    ) -> Result<Option<hagency_core::attachments::MatrixAttachmentObservation>, Error> {
        self.attachment
            .as_ref()
            .map(|m| m.observation(self.observation()))
            .transpose()
    }
    pub(crate) fn observation(&self) -> MatrixEventObservation {
        MatrixEventObservation {
            scope: MatrixIngressScope::from(&self.route),
            event: self.input.observation(),
            mentions: self.mentions.clone(),
            encrypted: matches!(self.proof, Proof::Verified { .. }),
        }
    }
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct Acknowledgement {
    pub sequence: u64,
    pub session_id: String,
    pub wake: bool,
}
impl From<&MatrixIngressReceipt> for Acknowledgement {
    fn from(r: &MatrixIngressReceipt) -> Self {
        Self {
            sequence: r.sequence,
            session_id: r.session_id.clone(),
            wake: r.wake,
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Receipt {
    pub token: String,
    pub digest: String,
    pub target_digest: String,
    pub acknowledgements: Vec<Acknowledgement>,
    pub filtered: usize,
    #[serde(default)]
    pub dispositions: Option<Vec<Disposition>>,
}
impl Batch {
    pub(crate) fn new(
        raw: Value,
        targets: Vec<ReplyRoute>,
        identity: String,
    ) -> Result<Self, Error> {
        let token = raw
            .get("next_batch")
            .and_then(Value::as_str)
            .ok_or(Error::Wire)?
            .to_owned();
        let digest = canonical::transport_digest(&raw).map_err(|_| Error::Wire)?;
        if targets.is_empty() || targets.len() > MAX_TARGETS {
            return Err(Error::Capacity);
        }
        Ok(Self {
            token,
            digest,
            sdk_identity: identity,
            targets,
            raw,
            phase: Phase::Prepared,
            reason: None,
            events: vec![],
            acknowledgements: vec![],
            filtered: 0,
            dispositions: Some(vec![]),
        })
    }
    pub(crate) fn derive(&mut self, sync: SyncResponse, history: &[Receipt]) -> Result<(), Error> {
        if sync
            .rooms
            .left
            .values()
            .any(|room| !room.timeline.events.is_empty())
        {
            return Err(Error::Unsupported);
        }
        let raw = disposition::raw_events(&self.raw)?;
        let mut returned = vec![];
        for (room, update) in sync.rooms.joined {
            if update.timeline.limited {
                return Err(Error::Unsupported);
            }
            for timeline in update.timeline.events {
                returned.push((room.to_string(), timeline));
            }
        }
        if raw.len() != returned.len() {
            return Err(Error::Conflict);
        }
        let mut events = vec![];
        let mut dispositions = vec![];
        let mut filtered = 0;
        let mut seen = BTreeSet::new();
        for ((room, original), (actual_room, timeline)) in raw.iter().zip(returned) {
            if room != &actual_room {
                return Err(Error::Conflict);
            }
            let value = wire::json(timeline.raw().json().get().as_bytes())?;
            match &timeline.kind {
                TimelineEventKind::Decrypted(_)
                    if ["event_id", "sender", "origin_server_ts"]
                        .iter()
                        .any(|field| original.get(field) != value.get(field))
                        || value.get("room_id").and_then(Value::as_str) != Some(room.as_str()) =>
                {
                    return Err(Error::Conflict);
                }
                TimelineEventKind::Decrypted(_) => {}
                _ if canonical::transport_digest(original).map_err(|_| Error::Wire)?
                    != canonical::transport_digest(&value).map_err(|_| Error::Wire)? =>
                {
                    return Err(Error::Conflict);
                }
                _ => {}
            }
            if let Some(id) = original.get("event_id").and_then(Value::as_str)
                && ruma::EventId::parse(id).is_ok()
                && !seen.insert((room.clone(), id.to_owned()))
            {
                return Err(Error::Conflict);
            }
            let source = Source::new(room, original)?;
            let decision = if let Some(prior) = Disposition::prior(&source, history) {
                prior
            } else if timeline.raw().deserialize().is_err() {
                Decision::Rejected {
                    reason: Rejection::Malformed,
                }
            } else {
                match self.event(room, original, &value, &timeline.kind) {
                    Ok(Some(event)) => {
                        let index = events.len();
                        events.push(event);
                        Decision::Candidate { index }
                    }
                    Ok(None) => Decision::NotTarget,
                    Err(reason) => Decision::Rejected { reason },
                }
            };
            if matches!(decision, Decision::NotTarget) {
                filtered += 1;
            }
            dispositions.push(Disposition::new(
                source,
                serde_json::to_value(&timeline.kind).map_err(|_| Error::Storage)?,
                decision,
            )?);
        }
        // Candidate content plus the complete private disposition ledger is bounded.
        if serde_json::to_vec(&(&events, &dispositions))
            .map_err(|_| Error::Storage)?
            .len()
            > 1024 * 1024
        {
            return Err(Error::Capacity);
        }
        self.events = events;
        self.filtered = filtered;
        self.dispositions = Some(dispositions);
        self.phase = Phase::Derived;
        Ok(())
    }
    fn event(
        &self,
        room: &str,
        original: &Value,
        value: &Value,
        kind: &TimelineEventKind,
    ) -> Result<Option<Event>, Rejection> {
        use Rejection::{CryptoIneligible, Malformed, PlaintextEncrypted, Unsupported};
        let string = |field: &str| value.get(field).and_then(Value::as_str).ok_or(Malformed);
        let id = string("event_id")?;
        if value
            .get("room_id")
            .is_some_and(|v| v.as_str() != Some(room))
        {
            return Err(Malformed);
        }
        let proof = match kind {
            TimelineEventKind::UnableToDecrypt { .. } => return Err(CryptoIneligible),
            TimelineEventKind::Decrypted(d) => {
                let info = &d.encryption_info;
                if !matches!(info.verification_state, VerificationState::Verified)
                    || info.sender.as_str() != string("sender")?
                    || info.forwarder.is_some()
                {
                    return Err(CryptoIneligible);
                }
                let device = info
                    .sender_device
                    .as_ref()
                    .ok_or(CryptoIneligible)?
                    .to_string();
                let session = match &info.algorithm_info {
                    AlgorithmInfo::MegolmV1AesSha2 {
                        session_id: Some(id),
                        ..
                    } => id.clone(),
                    _ => return Err(CryptoIneligible),
                };
                Proof::Verified {
                    sender: info.sender.to_string(),
                    device,
                    session,
                }
            }
            TimelineEventKind::PlainText { .. } => Proof::Plain,
        };
        if string("type")? != "m.room.message" {
            return Ok(None);
        }
        let content = value
            .get("content")
            .and_then(Value::as_object)
            .ok_or(Malformed)?;
        let relation = content.get("m.relates_to");
        if relation.is_some_and(|v| !v.is_object()) {
            return Err(Malformed);
        }
        let thread = match relation.and_then(|v| v.get("rel_type")) {
            Some(Value::String(t)) if t == "m.thread" => Some(
                relation
                    .and_then(|v| v.get("event_id"))
                    .and_then(Value::as_str)
                    .ok_or(Malformed)?
                    .to_owned(),
            ),
            Some(Value::String(_)) => return Err(Unsupported),
            Some(_) => return Err(Malformed),
            None => None,
        };
        let mut candidates = self
            .targets
            .iter()
            .filter(|t| t.room_id == room && t.thread_root == thread)
            .collect::<Vec<_>>();
        if candidates.is_empty() && thread.is_none() {
            candidates = self
                .targets
                .iter()
                .filter(|t| t.room_id == room && t.thread_root.as_deref() == Some(id))
                .collect();
        }
        if candidates.len() > 1 {
            return Err(Malformed);
        }
        let Some(target) = candidates.first() else {
            return Ok(None);
        };
        if target.encrypted && matches!(proof, Proof::Plain) {
            return Err(PlaintextEncrypted);
        }
        let kind = content
            .get("msgtype")
            .and_then(Value::as_str)
            .ok_or(Malformed)?;
        if !matches!(
            kind,
            "m.text" | "m.notice" | "m.emote" | "m.file" | "m.image"
        ) {
            return Err(Unsupported);
        }
        let mut mentions = BTreeSet::new();
        if let Some(mentioned) = content.get("m.mentions") {
            let mentioned = mentioned.as_object().ok_or(Malformed)?;
            if let Some(ids) = mentioned.get("user_ids") {
                let ids = ids.as_array().ok_or(Malformed)?;
                if ids.len() > 64 {
                    return Err(Malformed);
                }
                for id in ids {
                    mentions.insert(id.as_str().ok_or(Malformed)?.to_owned());
                }
            }
        }
        let attachment = if matches!(kind, "m.file" | "m.image") {
            let Proof::Verified {
                device, session, ..
            } = &proof
            else {
                return Err(CryptoIneligible);
            };
            if !target.encrypted {
                return Err(Unsupported);
            }
            Some(
                crate::attachments::Manifest::new(
                    &self.sdk_identity,
                    target,
                    original,
                    &value["content"],
                    device,
                    session,
                )
                .map_err(|_| Malformed)?,
            )
        } else {
            None
        };
        let event = Event {
            attachment,
            route: (*target).clone(),
            mentions,
            proof,
            input: Message {
                server_name: target.server_name.clone(),
                room_id: room.into(),
                event_id: id.into(),
                sender_mxid: string("sender")?.into(),
                thread_root: thread,
                body: content
                    .get("body")
                    .and_then(Value::as_str)
                    .ok_or(Malformed)?
                    .into(),
                kind: kind.into(),
                origin_ts: value
                    .get("origin_server_ts")
                    .and_then(Value::as_u64)
                    .ok_or(Malformed)?,
            },
        };
        event.observation().validate().map_err(|_| Malformed)?;
        Ok(Some(event))
    }
    pub(crate) fn rejected(&self) -> usize {
        disposition::rejected(self.dispositions.as_deref())
    }
    pub(crate) fn validate_restored(
        &self,
        identity: &str,
        user: &str,
        device: &str,
    ) -> Result<(), Error> {
        if self.sdk_identity != identity
            || self.targets.is_empty()
            || self.targets.len() > MAX_TARGETS
            || self
                .events
                .len()
                .checked_add(self.filtered)
                .is_none_or(|n| n > MAX_TIMELINE)
            || self.acknowledgements.len() > self.events.len()
            || self.raw.get("next_batch").and_then(Value::as_str) != Some(self.token.as_str())
            || canonical::transport_digest(&self.raw).map_err(|_| Error::Storage)? != self.digest
        {
            return Err(Error::Storage);
        }
        if matches!(self.phase, Phase::Prepared | Phase::Applying)
            && (!self.events.is_empty()
                || !self.acknowledgements.is_empty()
                || self.filtered != 0
                || self.dispositions.as_ref().is_some_and(|v| !v.is_empty()))
        {
            return Err(Error::Storage);
        }
        if let Some(values) = &self.dispositions {
            disposition::validate(values, self.events.len(), self.filtered)?;
            if self.phase == Phase::Derived || !values.is_empty() {
                let raw = disposition::raw_events(&self.raw).map_err(|_| Error::Storage)?;
                if raw.len() != values.len() {
                    return Err(Error::Storage);
                }
                for ((room, event), value) in raw.iter().zip(values) {
                    if !value.matches(room, event).map_err(|_| Error::Storage)? {
                        return Err(Error::Storage);
                    }
                    if let Decision::Candidate { index } = value.decision {
                        let candidate = self.events.get(index).ok_or(Error::Storage)?;
                        if candidate
                            .attachment
                            .as_ref()
                            .is_some_and(|manifest| !manifest.matches_original(event))
                        {
                            return Err(Error::Storage);
                        }
                        if candidate.input.room_id != *room
                            || event.get("event_id").and_then(Value::as_str)
                                != Some(candidate.input.event_id.as_str())
                            || event.get("sender").and_then(Value::as_str)
                                != Some(candidate.input.sender_mxid.as_str())
                            || event.get("origin_server_ts").and_then(Value::as_u64)
                                != Some(candidate.input.origin_ts)
                        {
                            return Err(Error::Storage);
                        }
                        if matches!(candidate.proof, Proof::Plain) {
                            let plain = self
                                .event(
                                    room,
                                    event,
                                    event,
                                    &TimelineEventKind::PlainText {
                                        event: serde_json::from_value(event.clone())
                                            .map_err(|_| Error::Storage)?,
                                    },
                                )
                                .map_err(|_| Error::Storage)?
                                .ok_or(Error::Storage)?;
                            if serde_json::to_value(&plain).map_err(|_| Error::Storage)?
                                != serde_json::to_value(candidate).map_err(|_| Error::Storage)?
                            {
                                return Err(Error::Storage);
                            }
                        }
                    }
                }
            }
        }
        let mut targets = BTreeSet::new();
        for target in &self.targets {
            MatrixIngressScope::from(target)
                .validate()
                .map_err(|_| Error::Storage)?;
            if target.sender_mxid != user
                || target.device_id != device
                || !targets.insert((&target.room_id, &target.thread_root))
            {
                return Err(Error::Storage);
            }
        }
        for event in &self.events {
            event.observation().validate().map_err(|_| Error::Storage)?;
            if matches!(event.input.kind.as_str(), "m.file" | "m.image")
                != event.attachment.is_some()
            {
                return Err(Error::Storage);
            }
            if let Some(manifest) = &event.attachment {
                manifest.validate(identity, user, device)?;
                if manifest.route != event.route {
                    return Err(Error::Storage);
                }
                event.attachment_observation()?;
            }
            if !self.targets.contains(&event.route) || event.input.room_id != event.route.room_id {
                return Err(Error::Storage);
            }
            match &event.proof {
                Proof::Plain if event.route.encrypted => return Err(Error::Storage),
                Proof::Verified {
                    sender,
                    device,
                    session,
                } if sender != &event.input.sender_mxid
                    || device.is_empty()
                    || session.is_empty() =>
                {
                    return Err(Error::Storage);
                }
                _ => {}
            }
        }
        for (event, ack) in self.events.iter().zip(&self.acknowledgements) {
            if ack.sequence == 0 || ack.session_id != event.route.session_id {
                return Err(Error::Storage);
            }
        }
        Ok(())
    }
    pub(crate) fn receipt(&self) -> Result<Receipt, Error> {
        Ok(Receipt {
            token: self.token.clone(),
            digest: self.digest.clone(),
            target_digest: canonical::transport_digest(
                &serde_json::to_value(&self.targets).map_err(|_| Error::Storage)?,
            )
            .map_err(|_| Error::Storage)?,
            acknowledgements: self.acknowledgements.clone(),
            filtered: self.filtered,
            dispositions: self.dispositions.clone(),
        })
    }
}

impl Receipt {
    pub(crate) fn validate_dispositions(&self) -> Result<(), Error> {
        if let Some(values) = &self.dispositions {
            disposition::validate(values, self.acknowledgements.len(), self.filtered)?;
        }
        Ok(())
    }
    pub(crate) fn lacks_filtered_history(&self) -> bool {
        self.filtered > 0 && self.dispositions.is_none()
    }
}
