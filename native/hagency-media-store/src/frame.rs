use crate::{Error, HostNamespace, Kind, Limits, Media, OperationId};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
};

pub(crate) const FILE_HEADER: u64 = 72;
pub(crate) const INTENT: usize = 192;
pub(crate) const TRAILER: u64 = 40;
const FILE_MAGIC: &[u8; 8] = b"HGMEDIA1";
const BEGIN: &[u8; 8] = b"HGMBEGIN";
pub(crate) const COMMIT: &[u8; 8] = b"HGMCOMIT";
#[derive(Clone)]
pub(crate) struct Entry {
    pub(crate) operation: OperationId,
    pub(crate) kind: Kind,
    pub(crate) offset: u64,
    pub(crate) length: u64,
    pub(crate) bytes: usize,
    pub(crate) digest: [u8; 32],
    pub(crate) chain: [u8; 32],
    pub(crate) previous: [u8; 32],
}
pub(crate) struct Prepared {
    pub(crate) intent: [u8; INTENT],
    pub(crate) entry: Entry,
}
pub(crate) enum Scan {
    Incomplete,
    Complete(Entry, Vec<u8>, Vec<u8>),
}
fn sum(parts: &[&[u8]]) -> [u8; 32] {
    let mut hash = Sha256::new();
    for part in parts {
        hash.update(part);
    }
    hash.finalize().into()
}
pub(crate) fn header(namespace: &HostNamespace) -> [u8; FILE_HEADER as usize] {
    let mut result = [0; FILE_HEADER as usize];
    result[..8].copy_from_slice(FILE_MAGIC);
    result[8..40].copy_from_slice(&namespace.0);
    let hash = sum(&[&result[..40]]);
    result[40..].copy_from_slice(&hash);
    result
}
pub(crate) fn validate_header(
    file: &mut File,
    namespace: &HostNamespace,
) -> Result<[u8; 32], Error> {
    file.seek(SeekFrom::Start(0)).map_err(|_| Error::Io)?;
    let mut actual = [0; FILE_HEADER as usize];
    file.read_exact(&mut actual).map_err(|_| Error::Corrupt)?;
    if actual != header(namespace) {
        return Err(Error::Identity);
    }
    Ok(sum(&[&actual]))
}
fn identity(namespace: &[u8; 32], operation: &OperationId, fields: &[u8]) -> [u8; 32] {
    sum(&[namespace, operation.0.as_bytes(), fields])
}
fn identity_fields(
    operation: &OperationId,
    kind: Kind,
    bytes: usize,
    descriptor: &[u8],
    content_hash: &[u8; 32],
) -> [u8; 80] {
    let mut fields = [0; 80];
    fields[..8].copy_from_slice(&(bytes as u64).to_le_bytes());
    fields[8..12].copy_from_slice(&(descriptor.len() as u32).to_le_bytes());
    fields[12..14].copy_from_slice(&(operation.0.len() as u16).to_le_bytes());
    fields[14] = kind.tag();
    fields[16..48].copy_from_slice(content_hash);
    fields[48..].copy_from_slice(&sum(&[descriptor]));
    fields
}
/// Pure association with supplied digest data, without ciphertext IO or proof of
/// source custody, SDK acceptance, durability, namespace ownership or authority.
pub fn encrypted_receipt_matches(
    namespace_digest: &[u8; 32],
    operation: &OperationId,
    ciphertext_len: usize,
    descriptor: &hagency_media::Descriptor,
    receipt_digest: &[u8; 32],
) -> bool {
    if ciphertext_len > crate::MAX_ITEM_BYTES {
        return false;
    }
    let fields = identity_fields(
        operation,
        Kind::Encrypted,
        ciphertext_len,
        descriptor.private_event_json(),
        descriptor.ciphertext_sha256(),
    );
    identity(namespace_digest, operation, &fields) == *receipt_digest
}
pub(crate) fn prepare(
    namespace: &HostNamespace,
    operation: &OperationId,
    media: &Media,
    offset: u64,
    previous: [u8; 32],
    limits: Limits,
) -> Result<Prepared, Error> {
    let data = media.bytes();
    let descriptor = media.descriptor();
    if data.len() > limits.item_bytes || descriptor.len() > 1024 {
        return Err(Error::Capacity);
    }
    let length = INTENT as u64
        + operation.0.len() as u64
        + data.len() as u64
        + descriptor.len() as u64
        + TRAILER;
    let mut intent = [0; INTENT];
    intent[..8].copy_from_slice(BEGIN);
    intent[8..16].copy_from_slice(&length.to_le_bytes());
    intent[16..96].copy_from_slice(&identity_fields(
        operation,
        media.kind(),
        data.len(),
        descriptor,
        &sum(&[data]),
    ));
    intent[96..128].copy_from_slice(&previous);
    let digest = identity(&namespace.0, operation, &intent[16..96]);
    intent[128..160].copy_from_slice(&digest);
    let check = sum(&[&namespace.0, &intent[..160], operation.0.as_bytes()]);
    intent[160..].copy_from_slice(&check);
    let chain = sum(&[&intent, operation.0.as_bytes(), data, descriptor]);
    Ok(Prepared {
        intent,
        entry: Entry {
            operation: operation.clone(),
            kind: media.kind(),
            offset,
            length,
            bytes: data.len(),
            digest,
            chain,
            previous,
        },
    })
}
fn fixed<const N: usize>(bytes: &[u8]) -> Result<[u8; N], Error> {
    bytes.try_into().map_err(|_| Error::Corrupt)
}
pub(crate) fn scan(
    file: &mut File,
    namespace: &HostNamespace,
    offset: u64,
    remaining: u64,
    limits: Limits,
    previous: [u8; 32],
    capture: bool,
) -> Result<Scan, Error> {
    if remaining < INTENT as u64 {
        return Ok(Scan::Incomplete);
    }
    file.seek(SeekFrom::Start(offset)).map_err(|_| Error::Io)?;
    let mut intent = [0; INTENT];
    file.read_exact(&mut intent).map_err(|_| Error::Io)?;
    if &intent[..8] != BEGIN || intent[31] != 0 {
        return Err(Error::Corrupt);
    }
    let length = u64::from_le_bytes(fixed(&intent[8..16])?);
    let bytes = u64::from_le_bytes(fixed(&intent[16..24])?);
    let descriptors = u32::from_le_bytes(fixed(&intent[24..28])?) as u64;
    let names = u16::from_le_bytes(fixed(&intent[28..30])?) as u64;
    let kind = Kind::from_tag(intent[30])?;
    if bytes > limits.item_bytes as u64
        || descriptors > 1024
        || !(1..=128).contains(&names)
        || length > limits.file_bytes
    {
        return Err(Error::Capacity);
    }
    if length != INTENT as u64 + names + bytes + descriptors + TRAILER
        || (kind == Kind::Encrypted) != (descriptors > 0)
    {
        return Err(Error::Corrupt);
    }
    if remaining < INTENT as u64 + names {
        return Ok(Scan::Incomplete);
    }
    let mut name = [0; 128];
    file.read_exact(&mut name[..names as usize])
        .map_err(|_| Error::Io)?;
    let operation =
        OperationId::new(std::str::from_utf8(&name[..names as usize]).map_err(|_| Error::Corrupt)?)
            .map_err(|_| Error::Corrupt)?;
    let digest = identity(&namespace.0, &operation, &intent[16..96]);
    if intent[96..128] != previous
        || intent[128..160] != digest
        || intent[160..] != sum(&[&namespace.0, &intent[..160], operation.0.as_bytes()])
    {
        return Err(Error::Corrupt);
    }
    if remaining < length {
        return Ok(Scan::Incomplete);
    }
    let mut chain = Sha256::new();
    chain.update(intent);
    chain.update(operation.0.as_bytes());
    let mut content_hash = Sha256::new();
    let mut content = Vec::new();
    if capture {
        content
            .try_reserve_exact(bytes as usize)
            .map_err(|_| Error::Capacity)?;
    }
    let mut buffer = [0; 64 * 1024];
    let mut left = bytes;
    while left > 0 {
        let n = left.min(buffer.len() as u64) as usize;
        file.read_exact(&mut buffer[..n]).map_err(|_| Error::Io)?;
        chain.update(&buffer[..n]);
        content_hash.update(&buffer[..n]);
        if capture {
            content.extend_from_slice(&buffer[..n]);
        }
        left -= n as u64;
    }
    let mut descriptor = vec![0; descriptors as usize];
    file.read_exact(&mut descriptor).map_err(|_| Error::Io)?;
    if kind == Kind::Encrypted {
        hagency_media::Descriptor::from_private_event_json(&descriptor)
            .map_err(|_| Error::Corrupt)?;
    }
    chain.update(&descriptor);
    let chain: [u8; 32] = chain.finalize().into();
    let mut trailer = [0; TRAILER as usize];
    file.read_exact(&mut trailer).map_err(|_| Error::Io)?;
    if &trailer[..8] != COMMIT
        || trailer[8..] != chain
        || intent[32..64] != content_hash.finalize()[..]
        || intent[64..96] != sum(&[&descriptor])
    {
        return Err(Error::Corrupt);
    }
    Ok(Scan::Complete(
        Entry {
            operation,
            kind,
            offset,
            length,
            bytes: bytes as usize,
            digest,
            chain,
            previous,
        },
        content,
        descriptor,
    ))
}
