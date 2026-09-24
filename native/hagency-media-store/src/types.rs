use crate::Error;
use hagency_files::Snapshot;
use hagency_media::{CheckedBytes, Descriptor, Encrypted};
use sha2::{Digest, Sha256};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

pub const MAX_ITEM_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_FILE_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_RECORDS: usize = 128;
pub const MAX_RESULTS: usize = 8;

/// A storage partition chosen by the host, never execution or Matrix authority.
#[derive(Clone)]
pub struct HostNamespace(pub(crate) [u8; 32]);
impl HostNamespace {
    /// Storage partition identity only, never a path or execution authority.
    pub fn digest(&self) -> &[u8; 32] {
        &self.0
    }
    pub fn new(value: &str) -> Result<Self, Error> {
        if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
            return Err(Error::Identity);
        }
        Ok(Self(Sha256::digest(value.as_bytes()).into()))
    }
}
#[derive(Clone)]
pub struct OperationId(pub(crate) String);
impl OperationId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub fn new(value: &str) -> Result<Self, Error> {
        if value.is_empty()
            || value.len() > 128
            || !value
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"_.-".contains(&c))
        {
            return Err(Error::Identity);
        }
        Ok(Self(value.into()))
    }
}
#[derive(Clone, Copy)]
pub struct Limits {
    pub(crate) item_bytes: usize,
    pub(crate) file_bytes: u64,
    pub(crate) records: usize,
    pub(crate) results: usize,
}
impl Limits {
    pub fn new(
        item_bytes: usize,
        file_bytes: u64,
        records: usize,
        results: usize,
    ) -> Result<Self, Error> {
        if item_bytes == 0
            || item_bytes > MAX_ITEM_BYTES
            || !(1024..=MAX_FILE_BYTES).contains(&file_bytes)
            || records == 0
            || records > MAX_RECORDS
            || results == 0
            || results > MAX_RESULTS
        {
            return Err(Error::Limit);
        }
        Ok(Self {
            item_bytes,
            file_bytes,
            records,
            results,
        })
    }
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            item_bytes: 4 * 1024 * 1024,
            file_bytes: 64 * 1024 * 1024,
            records: 64,
            results: 4,
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    Snapshot,
    Encrypted,
    Checked,
}
impl Kind {
    pub(crate) fn tag(self) -> u8 {
        match self {
            Self::Snapshot => 1,
            Self::Encrypted => 2,
            Self::Checked => 3,
        }
    }
    pub(crate) fn from_tag(tag: u8) -> Result<Self, Error> {
        match tag {
            1 => Ok(Self::Snapshot),
            2 => Ok(Self::Encrypted),
            3 => Ok(Self::Checked),
            _ => Err(Error::Corrupt),
        }
    }
}
/// Consumes real private upstream custody. No raw byte/path constructor.
pub enum Media {
    Snapshot(Snapshot),
    Encrypted(Encrypted),
    Checked(CheckedBytes),
}
impl Media {
    pub(crate) fn kind(&self) -> Kind {
        match self {
            Self::Snapshot(_) => Kind::Snapshot,
            Self::Encrypted(_) => Kind::Encrypted,
            Self::Checked(_) => Kind::Checked,
        }
    }
    pub(crate) fn bytes(&self) -> &[u8] {
        match self {
            Self::Snapshot(v) => v.bytes(),
            Self::Encrypted(v) => v.ciphertext(),
            Self::Checked(v) => v.bytes(),
        }
    }
    pub(crate) fn descriptor(&self) -> &[u8] {
        match self {
            Self::Encrypted(v) => v.descriptor().private_event_json(),
            _ => &[],
        }
    }
}
/// Failed staging preserves ownership rather than silently discarding keys.
/// Unadmitted input is returned; once IO begins, the original Store owns it.
pub struct StageFailure {
    pub(crate) error: Error,
    pub(crate) unadmitted: Option<Box<Media>>,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FailureCustody {
    ReturnedUnadmitted,
    RetainedByStore,
}
impl StageFailure {
    pub fn error(&self) -> Error {
        self.error
    }
    pub fn custody(&self) -> FailureCustody {
        if self.unadmitted.is_some() {
            FailureCustody::ReturnedUnadmitted
        } else {
            FailureCustody::RetainedByStore
        }
    }
    pub fn into_unadmitted(self) -> Option<Media> {
        self.unadmitted.map(|value| *value)
    }
    pub(crate) fn returned(error: Error, media: Media) -> Self {
        Self {
            error,
            unadmitted: Some(Box::new(media)),
        }
    }
    pub(crate) fn retained(error: Error) -> Self {
        Self {
            error,
            unadmitted: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Recovery {
    Clean,
    IncompleteTail,
    WriteOutcomeUnknown,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyncEvidence {
    /// OS acknowledged both retained file and directory descriptor sync calls.
    /// This is not qualified hardware power-loss durability.
    FileAndDirectorySynced,
    /// Current readability only. Never ordinary durable admission or upload authority.
    FileSyncedDirectoryUnconfirmed,
}
/// Host-local staging observation, never an upload or delivery receipt.
#[derive(Clone)]
pub struct Receipt {
    pub(crate) operation: OperationId,
    pub(crate) kind: Kind,
    pub(crate) len: usize,
    pub(crate) digest: [u8; 32],
    pub(crate) sync: SyncEvidence,
    pub(crate) replayed: bool,
}
impl Receipt {
    pub fn kind(&self) -> Kind {
        self.kind
    }
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    pub fn digest(&self) -> &[u8; 32] {
        &self.digest
    }
    pub fn sync_evidence(&self) -> SyncEvidence {
        self.sync
    }
    pub fn replayed(&self) -> bool {
        self.replayed
    }
    pub fn operation(&self) -> &OperationId {
        &self.operation
    }
}
pub(crate) struct Pool {
    pub(crate) held: AtomicUsize,
    pub(crate) limit: usize,
}
pub(crate) struct Permit(pub(crate) Arc<Pool>);
impl Permit {
    pub(crate) fn acquire(pool: &Arc<Pool>) -> Result<Self, Error> {
        pool.held
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < pool.limit).then_some(n + 1)
            })
            .map_err(|_| Error::Capacity)?;
        Ok(Self(pool.clone()))
    }
}
impl Drop for Permit {
    fn drop(&mut self) {
        self.0.held.fetch_sub(1, Ordering::AcqRel);
    }
}
/// Validated copied storage bytes. This does not reconstruct original source
/// handles, authenticate a descriptor, or provide any Matrix destination.
pub struct StagedMedia {
    pub(crate) bytes: Vec<u8>,
    pub(crate) descriptor: Option<Descriptor>,
    pub(crate) receipt: Receipt,
    pub(crate) _permit: Permit,
}

/// Original codec material plus its stable storage commitment before IO.
/// The host must persist this identity independently before staging. No capture,
/// execution, upload or journal-space reservation is granted by this object.
pub struct PreparedEncrypted {
    pub(crate) media: Media,
    pub(crate) operation: OperationId,
    pub(crate) namespace: HostNamespace,
    pub(crate) digest: [u8; 32],
    pub(crate) _permit: Permit,
}
impl PreparedEncrypted {
    pub fn operation(&self) -> &OperationId {
        &self.operation
    }
    pub fn namespace(&self) -> &HostNamespace {
        &self.namespace
    }
    pub fn digest(&self) -> &[u8; 32] {
        &self.digest
    }
    pub fn len(&self) -> usize {
        self.media.bytes().len()
    }
    pub fn is_empty(&self) -> bool {
        self.media.bytes().is_empty()
    }
}
impl StagedMedia {
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn descriptor(&self) -> Option<&Descriptor> {
        self.descriptor.as_ref()
    }
    pub fn receipt(&self) -> &Receipt {
        &self.receipt
    }
}

/// Exact committed storage custody, never the original codec Encrypted/source
/// Snapshot and never an upload retry authority. No constructor/Clone/serde.
pub struct RestoredEncrypted {
    pub(crate) bytes: Vec<u8>,
    pub(crate) descriptor: Descriptor,
    pub(crate) receipt: Receipt,
    pub(crate) namespace: HostNamespace,
    pub(crate) _permit: Permit,
}
impl RestoredEncrypted {
    /// Original storage partition commitment, never execution authority.
    pub fn namespace_digest(&self) -> &[u8; 32] {
        self.namespace.digest()
    }
    pub fn ciphertext(&self) -> &[u8] {
        &self.bytes
    }
    pub fn descriptor(&self) -> &Descriptor {
        &self.descriptor
    }
    pub fn receipt(&self) -> &Receipt {
        &self.receipt
    }
    /// A storage partition comparison only; not workspace or room authority.
    pub fn matches_namespace(&self, namespace: &HostNamespace) -> bool {
        self.namespace.0 == namespace.0
    }
}
