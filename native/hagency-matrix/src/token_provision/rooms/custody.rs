//! Separate fixed encrypted room-write custody; partial records never rearm.
use crate::Error;
use hagency_store::private;
use matrix_sdk_store_encryption::StoreCipher;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
};

pub(super) const STAGES: &[&str] = &[
    "dm-possible",
    "dm-response",
    "invite-possible",
    "invite-response",
    "join-possible",
    "join-response",
    "complete",
];
const PLAIN: usize = 32 * 1024;
const ENVELOPE: usize = 256 * 1024;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    binding: String,
    stage: String,
    value: Value,
}
pub(super) struct Custody {
    root: PathBuf,
    binding: String,
    cipher: StoreCipher,
    _lock: File,
}
impl Custody {
    pub fn open(root: PathBuf, binding: String, key: [u8; 32]) -> Result<Self, Error> {
        private::directory(root.parent().ok_or(Error::Storage)?).map_err(|_| Error::Storage)?;
        private::directory(&root).map_err(|_| Error::Storage)?;
        let path = root.join("rooms.lock");
        let lock = private::open(&path, !path.try_exists().map_err(|_| Error::Storage)?)
            .map_err(|_| Error::Storage)?;
        lock.try_lock().map_err(|_| Error::Busy)?;
        let mut count = 0;
        for entry in fs::read_dir(&root).map_err(|_| Error::Storage)? {
            count += 1;
            let entry = entry.map_err(|_| Error::Storage)?;
            let name = entry.file_name();
            let name = name.to_str().ok_or(Error::Storage)?;
            if count > STAGES.len() + 3
                || (!STAGES.contains(&name)
                    && !["rooms.lock", "binding", "cipher.key"].contains(&name))
            {
                return Err(Error::Storage);
            }
            private::open(&entry.path(), false).map_err(|_| Error::Storage)?;
        }
        let path = root.join("binding");
        let cipher = if path.try_exists().map_err(|_| Error::Storage)? {
            if read(&path, 64)? != binding.as_bytes() {
                return Err(Error::Conflict);
            }
            StoreCipher::import_with_key(&key, &read(&root.join("cipher.key"), 1024)?)
                .map_err(|_| Error::Storage)?
        } else {
            if count != 1 {
                return Err(Error::Storage);
            }
            private::write_new(&path, binding.as_bytes()).map_err(|_| Error::Storage)?;
            let cipher = StoreCipher::new().map_err(|_| Error::Storage)?;
            private::write_new(
                &root.join("cipher.key"),
                &cipher.export_with_key(&key).map_err(|_| Error::Storage)?,
            )
            .map_err(|_| Error::Storage)?;
            cipher
        };
        Ok(Self {
            root,
            binding,
            cipher,
            _lock: lock,
        })
    }
    pub fn write(&self, stage: &'static str, value: Value) -> Result<(), Error> {
        if !STAGES.contains(&stage) {
            return Err(Error::Storage);
        }
        let record = Record {
            binding: self.binding.clone(),
            stage: stage.into(),
            value,
        };
        if serde_json::to_vec(&record)
            .map_err(|_| Error::Storage)?
            .len()
            > PLAIN
        {
            return Err(Error::BodyTooLarge);
        }
        let bytes = self
            .cipher
            .encrypt_value(&record)
            .map_err(|_| Error::Storage)?;
        if bytes.len() > ENVELOPE {
            return Err(Error::BodyTooLarge);
        }
        private::write_new(&self.root.join(stage), &bytes).map_err(|_| Error::Storage)
    }
    pub fn values(&self) -> Result<Vec<Option<Value>>, Error> {
        STAGES
            .iter()
            .map(|stage| {
                let path = self.root.join(stage);
                if !path.try_exists().map_err(|_| Error::Storage)? {
                    return Ok(None);
                }
                let record: Record = self
                    .cipher
                    .decrypt_value(&read(&path, ENVELOPE)?)
                    .map_err(|_| Error::Storage)?;
                if record.binding != self.binding
                    || record.stage != *stage
                    || serde_json::to_vec(&record)
                        .map_err(|_| Error::Storage)?
                        .len()
                        > PLAIN
                {
                    return Err(Error::Storage);
                }
                Ok(Some(record.value))
            })
            .collect()
    }
}
fn read(path: &Path, cap: usize) -> Result<Vec<u8>, Error> {
    let file = private::open(path, false).map_err(|_| Error::Storage)?;
    if file.metadata().map_err(|_| Error::Storage)?.len() > cap as u64 {
        return Err(Error::BodyTooLarge);
    }
    let mut bytes = vec![];
    file.take(cap as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::Storage)?;
    if bytes.len() > cap {
        return Err(Error::BodyTooLarge);
    }
    Ok(bytes)
}
