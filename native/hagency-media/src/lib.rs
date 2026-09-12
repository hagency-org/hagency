//! Bounded attachment crypto, not authenticated room or delivery authority.
//! Descriptor provenance must come from an authenticated encrypted Matrix event.
mod descriptor;
pub use descriptor::Descriptor;
use hagency_files::Snapshot;
use matrix_sdk_crypto::{AttachmentDecryptor, AttachmentEncryptor};
use sha2::{Digest, Sha256};
use std::{
    io::{Cursor, Read},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("invalid media codec limits")]
    Limit,
    #[error("media codec capacity exhausted")]
    Capacity,
    #[error("invalid or unsupported attachment descriptor")]
    Descriptor,
    #[error("attachment ciphertext integrity failed")]
    Integrity,
    #[error("attachment crypto failed")]
    Crypto,
}

#[derive(Clone, Copy)]
pub struct Limits {
    max_bytes: usize,
    max_results: usize,
}
impl Limits {
    pub fn new(max_bytes: usize, max_results: usize) -> Result<Self, Error> {
        if max_bytes == 0 || max_bytes > 16 * 1024 * 1024 || max_results == 0 || max_results > 8 {
            return Err(Error::Limit);
        }
        Ok(Self {
            max_bytes,
            max_results,
        })
    }
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_bytes: 4 * 1024 * 1024,
            max_results: 4,
        }
    }
}
struct Pool {
    limits: Limits,
    held: AtomicUsize,
}
struct Permit(Arc<Pool>);
impl Drop for Permit {
    fn drop(&mut self) {
        self.0.held.fetch_sub(1, Ordering::AcqRel);
    }
}
#[derive(Clone)]
pub struct Codec {
    pool: Arc<Pool>,
}

/// Contains secret metadata and the original snapshot's immutable bytes and
/// actual file custody. Reuse this object for retry; re-encryption changes keys.
pub struct Encrypted {
    ciphertext: Vec<u8>,
    descriptor: Descriptor,
    _snapshot: Snapshot,
    _permit: Permit,
}
impl Encrypted {
    pub fn ciphertext(&self) -> &[u8] {
        &self.ciphertext
    }
    pub fn descriptor(&self) -> &Descriptor {
        &self.descriptor
    }
}
/// Plaintext obtained with a checked ciphertext hash. This does not authenticate
/// its key, sender, room or purpose; those require the outer encrypted event.
pub struct CheckedBytes {
    bytes: Vec<u8>,
    digest: [u8; 32],
    _permit: Permit,
}
impl CheckedBytes {
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn digest(&self) -> &[u8; 32] {
        &self.digest
    }
}
impl Codec {
    pub fn new(limits: Limits) -> Self {
        Self {
            pool: Arc::new(Pool {
                limits,
                held: AtomicUsize::new(0),
            }),
        }
    }
    fn admit(&self, len: usize) -> Result<Permit, Error> {
        if len > self.pool.limits.max_bytes {
            return Err(Error::Capacity);
        }
        self.pool
            .held
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < self.pool.limits.max_results).then_some(n + 1)
            })
            .map_err(|_| Error::Capacity)?;
        Ok(Permit(self.pool.clone()))
    }
    pub fn encrypt(&self, snapshot: Snapshot) -> Result<Encrypted, Error> {
        let permit = self.admit(snapshot.len())?;
        let mut source = Cursor::new(snapshot.bytes());
        // Pinned SDK uses fresh randomness. Its documented RNG panic never
        // produces a fallback key or plaintext upload result.
        let mut encryptor = AttachmentEncryptor::new(&mut source);
        let ciphertext = drain(&mut encryptor, snapshot.len())?;
        let json = serde_json::to_vec(&encryptor.finish()).map_err(|_| Error::Crypto)?;
        let descriptor = Descriptor::from_private_event_json(&json)?;
        if Sha256::digest(&ciphertext).as_slice() != descriptor.hash {
            return Err(Error::Crypto);
        }
        Ok(Encrypted {
            ciphertext,
            descriptor,
            _snapshot: snapshot,
            _permit: permit,
        })
    }
    pub fn decrypt(
        &self,
        descriptor: &Descriptor,
        ciphertext: &[u8],
    ) -> Result<CheckedBytes, Error> {
        let permit = self.admit(ciphertext.len())?;
        if Sha256::digest(ciphertext).as_slice() != descriptor.hash {
            return Err(Error::Integrity);
        }
        let mut source = Cursor::new(ciphertext);
        let mut decryptor =
            AttachmentDecryptor::new(&mut source, descriptor.sdk()?).map_err(|_| Error::Crypto)?;
        // Even though the full hash was checked first, consume SDK EOF as well.
        // No streaming accessor exposes plaintext before all checks complete.
        let bytes = drain(&mut decryptor, ciphertext.len())?;
        let digest = Sha256::digest(&bytes).into();
        Ok(CheckedBytes {
            bytes,
            digest,
            _permit: permit,
        })
    }
}

fn drain(reader: &mut impl Read, expected: usize) -> Result<Vec<u8>, Error> {
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(expected)
        .map_err(|_| Error::Capacity)?;
    let mut chunk = [0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut chunk).map_err(|_| Error::Crypto)?;
        if n == 0 {
            break;
        }
        if n > expected.saturating_sub(bytes.len()) {
            return Err(Error::Crypto);
        }
        bytes.extend_from_slice(&chunk[..n]);
    }
    if bytes.len() != expected {
        return Err(Error::Crypto);
    }
    Ok(bytes)
}
