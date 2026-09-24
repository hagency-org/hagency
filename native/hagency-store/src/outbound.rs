//! Host-only outbound custody commands. No Deserialize implementation or HTTP
//! route can mint a managed scope from an operator fixture envelope.
use crate::Error;
use hagency_core::{
    InvalidInput, JSON_SAFE_MAX, canonical,
    custody::{Kind, Lane, Receipt},
};
use serde::Serialize;
use serde_json::Value;

pub(crate) mod repository;
#[cfg(test)]
mod tests;

#[derive(Clone, Serialize)]
pub struct RegistrationIdentity {
    pub binding: String,
    pub registration_generation: u64,
    pub side_id: String,
    pub fleet_id: String,
    /// SHA-256 of host-owned registration identity; never an AS secret.
    pub registration_fingerprint: String,
}
#[derive(Clone)]
pub struct Activation {
    pub registration: RegistrationIdentity,
    pub machine_generation: u64,
    /// Hash of the configured credential identity, not the machine bearer token.
    pub credential_fingerprint: String,
}
#[derive(Clone)]
pub struct TransportScope {
    binding: String,
    generation: u64,
    key: String,
    consumer: String,
}
impl TransportScope {
    pub fn consumer(&self) -> &str {
        &self.consumer
    }
    pub fn machine_generation(&self) -> u64 {
        self.generation
    }
}
#[derive(Clone)]
pub struct PollTicket {
    scope: TransportScope,
    lane: Lane,
    key: String,
}
impl PollTicket {
    pub fn consumer(&self) -> &str {
        self.scope.consumer()
    }
    pub fn lane(&self) -> Lane {
        self.lane
    }
}
#[derive(Clone)]
pub struct LeasedDelivery {
    pub machine_generation: u64,
    pub id: String,
    pub lane: Lane,
    pub kind: Kind,
    pub payload: Value,
    pub token: String,
    pub expires_at_ms: u64,
}
#[derive(Clone)]
pub struct AckTicket {
    scope: TransportScope,
    lane: Lane,
    id: String,
    token: String,
}
impl AckTicket {
    /// Private adapter request body. This type has no Debug or Serialize projection.
    pub fn wire_body(&self) -> Value {
        serde_json::json!({"id":self.id,"lane":self.lane,"token":self.token})
    }
}
#[derive(Clone, Copy)]
pub enum AckResponse {
    Accepted,
    Unknown,
    StaleLease,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AckResolution {
    Accepted,
    Unknown,
    Reclaim,
    Replaced,
}
#[derive(Clone)]
pub struct ClaimTicket {
    binding: String,
    id: String,
    key: String,
}
impl ClaimTicket {
    pub fn id(&self) -> &str {
        &self.id
    }
}
pub struct StartedWork {
    pub ticket: ClaimTicket,
    pub delivery: Receipt,
    pub kind: Kind,
    pub origin_machine_generation: u64,
    pub payload: Value,
}
#[derive(Clone)]
pub enum Inspection {
    Completed(Value),
    Retry,
}
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DeliveryView {
    pub receipt: Receipt,
    pub origin_machine_generation: u64,
    pub lease_state: String,
    pub processing_state: String,
    pub attempt_id: Option<String>,
    pub result_digest: Option<String>,
}
#[derive(Clone)]
pub struct PublicationTicket {
    scope: TransportScope,
    sequence: u64,
    digest: String,
    body: String,
}
impl PublicationTicket {
    pub fn sequence(&self) -> u64 {
        self.sequence
    }
    pub fn wire_body(&self) -> &str {
        &self.body
    }
}
#[derive(Clone, Copy)]
pub enum PublicationResponse {
    Accepted,
    Unknown,
    Rejected,
}
/// Inputs are created by the future authenticated host adapter, never decoded
/// from the fixture intake endpoint. Timestamps are the host clock.
#[derive(Clone)]
pub enum Command {
    Activate(Activation),
    BeginPoll {
        scope: TransportScope,
        lane: Lane,
    },
    Receive {
        poll: PollTicket,
        delivery: LeasedDelivery,
    },
    BeginAck {
        scope: TransportScope,
        lane: Lane,
        id: String,
    },
    Ack {
        ticket: AckTicket,
        response: AckResponse,
    },
    Claim {
        scope: TransportScope,
        lane: Lane,
        id: String,
        lease_ms: u64,
    },
    Start(ClaimTicket),
    /// An adapter timeout or lost response is durable uncertainty, not failure
    /// permission to repeat an external effect.
    ProcessingUnknown(ClaimTicket),
    Complete {
        ticket: ClaimTicket,
        result: Value,
    },
    Inspect {
        scope: TransportScope,
        attempt_id: String,
        outcome: Inspection,
    },
    View {
        scope: TransportScope,
        lane: Lane,
        id: String,
    },
    Head {
        scope: TransportScope,
        lane: Lane,
    },
    /// Resume a current unconfirmed lease even when its processing tombstone
    /// is already done; a completed redelivery still needs its remote ACK.
    AckHead {
        scope: TransportScope,
        lane: Lane,
    },
    /// Freeze a complete host-produced v2 update. Generation, sequence and v are
    /// host generated here; statuses keep their original observedAt verbatim.
    FreezePublication {
        scope: TransportScope,
        body: Value,
    },
    PendingPublication(TransportScope),
    BeginPublication(PublicationTicket),
    Publication {
        ticket: PublicationTicket,
        response: PublicationResponse,
    },
}
pub enum Reply {
    Scope(TransportScope),
    Poll(PollTicket),
    Received(Receipt),
    AckTicket(AckTicket),
    Ack(AckResolution),
    Claim(Option<ClaimTicket>),
    Started(StartedWork),
    Finished { digest: String },
    Inspected,
    ProcessingRecorded,
    View(DeliveryView),
    Head(Option<DeliveryView>),
    Publication(Option<PublicationTicket>),
    PublicationRecorded,
}
fn text(value: &str, max: usize) -> Result<(), Error> {
    if value.is_empty() || value.len() > max || value.chars().any(char::is_control) {
        return Err(InvalidInput("invalid outbound identity").into());
    }
    Ok(())
}
fn positive(value: u64) -> Result<(), Error> {
    if value == 0 || value > JSON_SAFE_MAX {
        return Err(InvalidInput("invalid outbound generation or timestamp").into());
    }
    Ok(())
}
fn fingerprint(value: &str) -> Result<(), Error> {
    if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(InvalidInput("outbound fingerprint must be SHA-256").into());
    }
    Ok(())
}
fn data(value: &Value, max: usize) -> Result<usize, Error> {
    let encoded = canonical::encode_transport(value)?;
    let bytes = encoded.len().max(serde_json::to_vec(value)?.len());
    if !value.is_object() || bytes > max {
        return Err(InvalidInput("outbound data exceeds object or byte bound").into());
    }
    Ok(bytes)
}
impl Command {
    pub(crate) fn input_bytes(&self) -> Result<usize, Error> {
        let bytes = match self {
            Self::Activate(a) => {
                text(&a.registration.binding, 128)?;
                text(&a.registration.side_id, 512)?;
                text(&a.registration.fleet_id, 128)?;
                positive(a.registration.registration_generation)?;
                positive(a.machine_generation)?;
                fingerprint(&a.registration.registration_fingerprint)?;
                fingerprint(&a.credential_fingerprint)?;
                2048
            }
            Self::Receive { delivery, .. } => {
                positive(delivery.machine_generation)?;
                positive(delivery.expires_at_ms)?;
                text(&delivery.id, 512)?;
                text(&delivery.token, 4096)?;
                if (delivery.lane == Lane::Matrix) != (delivery.kind == Kind::Transaction) {
                    return Err(InvalidInput("invalid outbound lane").into());
                }
                // 16 KiB covers doubled JSON escaping of the bounded lease token,
                // delivery ID, scope and all envelope metadata in addition to data.
                data(&delivery.payload, 4 * 1024 * 1024)? + 16384
            }
            Self::BeginAck { id, .. }
            | Self::View { id, .. }
            | Self::Inspect { attempt_id: id, .. }
            | Self::Claim { id, .. } => {
                text(id, 512)?;
                if let Self::Claim { lease_ms, .. } = self
                    && !(1..=120_000).contains(lease_ms)
                {
                    return Err(InvalidInput("invalid custody claim deadline").into());
                }
                if let Self::Inspect {
                    outcome: Inspection::Completed(value),
                    ..
                } = self
                {
                    data(value, 64 * 1024)? + 2048
                } else {
                    2048
                }
            }
            Self::Complete { result, .. } => data(result, 64 * 1024)? + 2048,
            Self::FreezePublication { body, .. } => {
                let bytes = data(body, 1024 * 1024)?;
                if body.get("v").is_some()
                    || body.get("sequence").is_some()
                    || body.get("generation").is_some()
                    || body.get("heartbeat") != Some(&Value::Bool(true))
                {
                    return Err(InvalidInput("publication metadata is host-owned").into());
                }
                for (key, max) in [("statuses", 200), ("probeReceipts", 10)] {
                    if let Some(value) = body.get(key)
                        && value.as_array().is_none_or(|a| a.len() > max)
                    {
                        return Err(InvalidInput("publication batch exceeds bound").into());
                    }
                }
                bytes + 2048
            }
            Self::BeginPublication(ticket) | Self::Publication { ticket, .. } => {
                ticket.body.len() + 2048
            }
            // Opaque ACK tickets may retain 4096 token bytes; 12 KiB also covers
            // worst-case escaping plus bounded scope/consumer/ID metadata.
            Self::Ack { .. } => 12288,
            _ => 2048,
        };
        Ok(bytes)
    }
}
