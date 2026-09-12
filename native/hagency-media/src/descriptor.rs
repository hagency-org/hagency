use crate::Error;
use base64::{
    Engine,
    engine::general_purpose::{STANDARD_NO_PAD, URL_SAFE_NO_PAD},
};
use matrix_sdk_crypto::MediaEncryptionInfo;
use serde::Deserialize;

const MAX_DESCRIPTOR: usize = 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireInfo {
    v: String,
    key: WireKey,
    iv: String,
    hashes: WireHashes,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireKey {
    kty: String,
    alg: String,
    ext: bool,
    k: String,
    key_ops: Vec<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireHashes {
    sha256: String,
}

/// Private encryption metadata only, without URL or room authority. No Debug,
/// Serialize or generic Deserialize: callers must use the bounded parser.
pub struct Descriptor {
    json: Vec<u8>,
    pub(crate) hash: [u8; 32],
}
impl Descriptor {
    /// The caller must separately authenticate the encrypted event that supplied
    /// this data. A valid descriptor is not sender identity or decryption proof.
    pub fn from_private_event_json(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() > MAX_DESCRIPTOR {
            return Err(Error::Descriptor);
        }
        // Struct deserialization rejects duplicate fields before any Value can
        // discard them. Fixed shape and the byte cap bound all allocations.
        let wire: WireInfo = serde_json::from_slice(bytes).map_err(|_| Error::Descriptor)?;
        if wire.v != "v2"
            || wire.key.kty != "oct"
            || wire.key.alg != "A256CTR"
            || !wire.key.ext
            || wire.key.key_ops.len() != 2
            || !wire.key.key_ops.iter().any(|op| op == "encrypt")
            || !wire.key.key_ops.iter().any(|op| op == "decrypt")
        {
            return Err(Error::Descriptor);
        }
        let key = URL_SAFE_NO_PAD
            .decode(&wire.key.k)
            .map_err(|_| Error::Descriptor)?;
        let iv = STANDARD_NO_PAD
            .decode(&wire.iv)
            .map_err(|_| Error::Descriptor)?;
        let hash = STANDARD_NO_PAD
            .decode(&wire.hashes.sha256)
            .map_err(|_| Error::Descriptor)?;
        if key.len() != 32
            || iv.len() != 16
            || hash.len() != 32
            || iv[8..].iter().any(|byte| *byte != 0)
        {
            return Err(Error::Descriptor);
        }
        let hash = hash.try_into().map_err(|_| Error::Descriptor)?;
        // Also require the pinned SDK to accept exactly the validated metadata.
        let _: MediaEncryptionInfo =
            serde_json::from_slice(bytes).map_err(|_| Error::Descriptor)?;
        Ok(Self {
            json: bytes.to_vec(),
            hash,
        })
    }

    /// Secret-bearing JSON for a trusted encrypted event/staging adapter only.
    /// Never log it, project it to MCP/HTTP or send it in a plaintext room event.
    pub fn private_event_json(&self) -> &[u8] {
        &self.json
    }

    /// Declared ciphertext digest data only. This does not read ciphertext or
    /// create a receipt, source custody, durability evidence or current authority.
    pub fn ciphertext_sha256(&self) -> &[u8; 32] {
        &self.hash
    }

    pub(crate) fn sdk(&self) -> Result<MediaEncryptionInfo, Error> {
        serde_json::from_slice(&self.json).map_err(|_| Error::Descriptor)
    }
}
