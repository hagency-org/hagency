//! Private journal data and opaque host references. No network authorization.
use crate::{Error, HostConfig, MediaId, UploadResponse};
use hagency_core::{replies::ReplyRoute, uploads::StageCommitment};
use hagency_store::UploadSend;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    sync::{Arc, OnceLock},
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

pub(super) const KEY: &[u8] = b"hagency.observer.uploads.v1";
pub(super) const MAX_RECORDS: usize = 64;
pub(super) const MAX_BODY: usize = 4096;
const MAX_IDENTITY: usize = 4096;
pub(super) const MAX_RECORD: usize = 32 * 1024;
// Vec<u8> JSON <=4*4096, MXC JSON <=6*517, hashes/receipt/field names <1024.
const RESPONSE_RESERVE: usize = 24 * 1024;
pub(super) const MAX_PLAINTEXT: usize = MAX_RECORDS * (MAX_RECORD + 80) + 1024;
// StoreCipher 0.18: decimal-array ciphertext (plaintext +16-byte tag),
// 24-byte nonce and version/field syntax. 256 exceeds nonce+syntax maximum.
pub(super) const MAX_ENVELOPE: usize = 4 * (MAX_PLAINTEXT + 16) + 256;

pub(super) fn memory() -> &'static Arc<Semaphore> {
    static MEMORY: OnceLock<Arc<Semaphore>> = OnceLock::new();
    MEMORY.get_or_init(|| Arc::new(Semaphore::new(MAX_RECORDS)))
}
pub(super) fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn encoded<T: Serialize>(value: &T) -> Result<Vec<u8>, Error> {
    serde_json::to_vec(value).map_err(|_| Error::Storage)
}

#[derive(Clone)]
pub(super) struct Context {
    pub sdk: String,
    pub binding: String,
    https: bool,
    server: String,
    user: String,
    device: String,
    engagement: String,
    registration_generation: u64,
    rooms: Vec<String>,
}
impl Context {
    pub(super) fn new(config: &HostConfig) -> Result<Self, Error> {
        Ok(Self {
            sdk: String::new(),
            binding: config.binding()?,
            https: config.endpoint.scheme() == "https",
            server: config.identity.server_name.clone(),
            user: config.identity.transport.sender_mxid.clone(),
            device: config.identity.transport.device_id.clone(),
            engagement: config.identity.transport.engagement_id.clone(),
            registration_generation: config.identity.transport.registration_generation,
            rooms: config
                .rooms
                .iter()
                .map(|room| room.room_id.clone())
                .collect(),
        })
    }
    pub(super) fn reference(&self, send: &UploadSend) -> Result<Reference, Error> {
        let identity = Identity {
            version: 1,
            upload_id: send.identity().id().into(),
            fence: send.fence(),
            stage: send.stage().clone(),
            route: send.route().clone(),
            sdk: self.sdk.clone(),
            binding: self.binding.clone(),
        };
        identity.validate(self)?;
        Ok(Reference(Arc::new(identity)))
    }
}
#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct Marker {
    version: u8,
    sdk: String,
    binding: String,
}
impl Marker {
    pub(super) fn new(context: &Context) -> Self {
        Self {
            version: 1,
            sdk: context.sdk.clone(),
            binding: context.binding.clone(),
        }
    }
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct Identity {
    version: u8,
    pub upload_id: String,
    pub fence: u64,
    pub stage: StageCommitment,
    pub route: ReplyRoute,
    sdk: String,
    binding: String,
}
impl Identity {
    fn validate(&self, context: &Context) -> Result<(), Error> {
        if !context.https
            || self.version != 1
            || self.fence == 0
            || self.fence > hagency_core::JSON_SAFE_MAX
            || self.sdk != context.sdk
            || self.binding != context.binding
            || self.sdk.is_empty()
            || self.upload_id.len() != 39
            || !self.upload_id.starts_with("upload_")
            || !self.upload_id[7..]
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            || self.route.server_name != context.server
            || self.route.sender_mxid != context.user
            || self.route.device_id != context.device
            || self.route.engagement_id != context.engagement
            || self.route.registration_generation != context.registration_generation
            || !context.rooms.contains(&self.route.room_id)
            || !self.route.encrypted
        {
            return Err(Error::Identity);
        }
        use hagency_core::{
            project::identifier,
            replies::{RoomPrivacy, generation, matrix_room, matrix_user},
        };
        for id in [
            &self.route.session_id,
            &self.route.engagement_id,
            &self.route.fleet_id,
            &self.route.project_id,
            &self.route.device_id,
        ] {
            identifier(id, 512).map_err(|_| Error::Storage)?;
        }
        for value in [
            self.route.session_generation,
            self.route.registration_generation,
            self.route.transport_generation,
            self.route.room_generation,
        ] {
            generation(value).map_err(|_| Error::Storage)?;
        }
        matrix_user(&self.route.sender_mxid, &self.route.server_name)
            .map_err(|_| Error::Storage)?;
        matrix_room(&self.route.room_id, &self.route.server_name).map_err(|_| Error::Storage)?;
        ruma::UserId::parse(&self.route.owner_mxid).map_err(|_| Error::Storage)?;
        if let Some(root) = &self.route.thread_root {
            // Match actual SessionBinding issuance; the complete identity has
            // its own finite cap rather than narrowing opaque event IDs to255.
            ruma::EventId::parse(root).map_err(|_| Error::Storage)?;
        }
        if let RoomPrivacy::Direct { human_mxid } = &self.route.privacy {
            ruma::UserId::parse(human_mxid).map_err(|_| Error::Storage)?;
            if human_mxid == &self.route.sender_mxid {
                return Err(Error::Storage);
            }
        }
        self.stage.validate().map_err(|_| Error::Storage)?;
        if encoded(self)?.len() > MAX_IDENTITY {
            return Err(Error::Capacity);
        }
        Ok(())
    }
    pub(super) fn digest(&self) -> Result<String, Error> {
        Ok(hash(&encoded(self)?))
    }
}
/// Historical exact address only. Clones share bounded identity, never raw bodies.
#[derive(Clone)]
pub(crate) struct Reference(pub(super) Arc<Identity>);
impl Reference {
    pub(crate) fn id(&self) -> &str {
        &self.0.upload_id
    }
    pub(crate) fn fence(&self) -> u64 {
        self.0.fence
    }
    pub(crate) fn stage(&self) -> &StageCommitment {
        &self.0.stage
    }
    pub(crate) fn route(&self) -> &ReplyRoute {
        &self.0.route
    }
}
/// Issued once after reservation, tied to one ephemeral SDK owner. No Clone.
pub(crate) struct LivePermit {
    pub(super) reference: Reference,
    pub(super) owner: Arc<()>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Phase {
    Reserved,
    WritePossible,
    Accepted,
}
/// Bounded safe commitment. No route, SDK identity, body or MXC projection.
pub(crate) struct Inspection {
    phase: Phase,
    receipt: Option<hagency_core::uploads::UploadAcceptance>,
}
impl Inspection {
    pub(crate) fn phase(&self) -> Phase {
        self.phase
    }
    pub(crate) fn receipt(&self) -> Option<&hagency_core::uploads::UploadAcceptance> {
        self.receipt.as_ref()
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Response {
    pub body: Vec<u8>,
    pub body_sha256: [u8; 32],
    pub mxc: String,
    pub receipt_id: String,
    pub receipt_digest: String,
    #[serde(skip)]
    pub custody: Option<OwnedSemaphorePermit>,
}
impl Response {
    // Only called after finite queue AND memory custody have been reserved.
    pub(super) fn copy(
        reference: &Reference,
        sealed: &UploadResponse,
        permit: OwnedSemaphorePermit,
    ) -> Result<Self, Error> {
        if sealed.body().len() > MAX_BODY {
            return Err(Error::Capacity);
        }
        let digest = reference.0.digest()?;
        let mut response = Self {
            body: sealed.body().to_vec(),
            body_sha256: *sealed.body_sha256(),
            mxc: sealed.media_id().to_mxc(),
            receipt_id: format!("upload_receipt_{digest}"),
            receipt_digest: String::new(),
            custody: Some(permit),
        };
        response.receipt_digest = response.digest(&digest);
        response.validate(&digest)?;
        Ok(response)
    }
    fn digest(&self, record: &str) -> String {
        let mut hash = Sha256::new();
        hash.update(b"hagency.upload.accepted.v1\0");
        hash.update(record.as_bytes());
        hash.update(200_u16.to_be_bytes());
        hash.update((self.body.len() as u64).to_be_bytes());
        hash.update(&self.body);
        format!("{:x}", hash.finalize())
    }
    fn validate(&self, record: &str) -> Result<(), Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Reply {
            content_uri: String,
        }
        if self.body.len() > MAX_BODY || self.mxc.len() > 517 {
            return Err(Error::Storage);
        }
        let reply: Reply = serde_json::from_slice(&self.body).map_err(|_| Error::Storage)?;
        if reply.content_uri != self.mxc
            || MediaId::new(&self.mxc)
                .map_err(|_| Error::Storage)?
                .to_mxc()
                != self.mxc
            || self.body_sha256 != <[u8; 32]>::from(Sha256::digest(&self.body))
            || self.receipt_id != format!("upload_receipt_{record}")
            || self.receipt_digest != self.digest(record)
        {
            return Err(Error::Storage);
        }
        Ok(())
    }
    pub(super) fn same(&self, other: &Self) -> bool {
        self.body == other.body
            && self.body_sha256 == other.body_sha256
            && self.mxc == other.mxc
            && self.receipt_id == other.receipt_id
            && self.receipt_digest == other.receipt_digest
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Record {
    pub identity: Identity,
    pub digest: String,
    pub phase: Phase,
    pub response: Option<Response>,
}
impl Record {
    pub(super) fn reserved(reference: &Reference) -> Result<Self, Error> {
        let record = Self {
            identity: (*reference.0).clone(),
            digest: reference.0.digest()?,
            phase: Phase::Reserved,
            response: None,
        };
        if encoded(&record)?.len() + RESPONSE_RESERVE > MAX_RECORD {
            return Err(Error::Capacity);
        }
        Ok(record)
    }
    pub(super) fn inspection(&self) -> Inspection {
        Inspection {
            phase: self.phase,
            receipt: self
                .response
                .as_ref()
                .map(|r| hagency_core::uploads::UploadAcceptance {
                    receipt_id: r.receipt_id.clone(),
                    receipt_digest: r.receipt_digest.clone(),
                }),
        }
    }
    fn validate(&self, context: &Context) -> Result<(), Error> {
        self.identity.validate(context)?;
        if self.digest != self.identity.digest()?
            || encoded(self)?.len() > MAX_RECORD
            || (self.phase == Phase::Accepted) != self.response.is_some()
        {
            return Err(Error::Storage);
        }
        if let Some(response) = &self.response {
            response.validate(&self.digest)?;
        }
        Ok(())
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Ledger {
    marker: Marker,
    pub records: BTreeMap<String, Record>,
}
impl Ledger {
    pub(super) fn new(context: &Context) -> Self {
        Self {
            marker: Marker::new(context),
            records: BTreeMap::new(),
        }
    }
    pub(super) fn validate(&self, context: &Context) -> Result<(), Error> {
        if self.marker != Marker::new(context)
            || self.records.len() > MAX_RECORDS
            || encoded(self)?.len() > MAX_PLAINTEXT
        {
            return Err(Error::Storage);
        }
        for (id, record) in &self.records {
            if id != &record.identity.upload_id {
                return Err(Error::Storage);
            }
            record.validate(context)?;
        }
        Ok(())
    }
    pub(super) fn restore(&self, id: &str, context: &Context) -> Result<Reference, Error> {
        let record = self.records.get(id).ok_or(Error::OutcomeUnknown)?;
        record.validate(context)?;
        Ok(Reference(Arc::new(record.identity.clone())))
    }
    pub(super) fn exact(&self, reference: &Reference) -> Result<&Record, Error> {
        let record = self
            .records
            .get(&reference.0.upload_id)
            .ok_or(Error::OutcomeUnknown)?;
        if record.identity != *reference.0 {
            return Err(Error::Conflict);
        }
        Ok(record)
    }
}
