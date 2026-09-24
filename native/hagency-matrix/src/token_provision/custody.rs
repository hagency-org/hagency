//! Fixed create-only encrypted stage records. A partial bootstrap is retained,
//! never repaired with another cipher/account. No canonical domain writes.
use super::{Context, SavedResponse};
use crate::Error;
use hagency_core::canonical;
use hagency_store::private;
use matrix_sdk_store_encryption::StoreCipher;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
};

const STAGES: &[&str] = &["possible", "initial", "auth-possible", "auth", "complete"];
const AS_STAGES: &[&str] = &["possible", "initial", "login-possible", "login", "complete"];
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
    stages: &'static [&'static str],
    root: PathBuf,
    binding: String,
    cipher: StoreCipher,
    _lock: File,
}
pub(super) struct Responses {
    pub possible: bool,
    pub initial: Option<SavedResponse>,
    pub auth_possible: bool,
    pub auth: Option<SavedResponse>,
    pub complete: Option<Value>,
}
impl Custody {
    pub fn open(root: PathBuf, context: &Context, key: [u8; 32]) -> Result<Self, Error> {
        let stages = if context.application_service.is_some() {
            AS_STAGES
        } else {
            STAGES
        };
        private::directory(root.parent().ok_or(Error::Storage)?).map_err(|_| Error::Storage)?;
        private::directory(&root).map_err(|_| Error::Storage)?;
        let path = root.join("registration.lock");
        let lock = private::open(&path, !path.try_exists().map_err(|_| Error::Storage)?)
            .map_err(|_| Error::Storage)?;
        lock.try_lock().map_err(|_| Error::Busy)?;
        let entries = fs::read_dir(&root).map_err(|_| Error::Storage)?;
        let mut count = 0;
        for entry in entries {
            count += 1;
            let entry = entry.map_err(|_| Error::Storage)?;
            let name = entry.file_name();
            let name = name.to_str().ok_or(Error::Storage)?;
            if count > 9
                || (!stages.contains(&name)
                    && !["registration.lock", "binding", "cipher.key", "sdk"].contains(&name))
            {
                return Err(Error::Storage);
            }
            if name == "sdk" {
                private::directory(&entry.path()).map_err(|_| Error::Storage)?;
            } else {
                private::open(&entry.path(), false).map_err(|_| Error::Storage)?;
            }
        }
        let binding = canonical::transport_digest(
            &serde_json::to_value(context).map_err(|_| Error::Storage)?,
        )
        .map_err(|_| Error::Storage)?;
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
            stages,
            root,
            binding,
            cipher,
            _lock: lock,
        })
    }
    pub fn write(&self, stage: &'static str, value: Value) -> Result<(), Error> {
        if !self.stages.contains(&stage) {
            return Err(Error::Storage);
        }
        let record = Record {
            binding: self.binding.clone(),
            stage: stage.to_owned(),
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
    fn value(&self, stage: &'static str) -> Result<Option<Value>, Error> {
        let path = self.root.join(stage);
        if !path.try_exists().map_err(|_| Error::Storage)? {
            return Ok(None);
        }
        let record: Record = self
            .cipher
            .decrypt_value(&read(&path, ENVELOPE)?)
            .map_err(|_| Error::Storage)?;
        if record.binding != self.binding
            || record.stage != stage
            || serde_json::to_vec(&record)
                .map_err(|_| Error::Storage)?
                .len()
                > PLAIN
        {
            return Err(Error::Storage);
        }
        Ok(Some(record.value))
    }
    pub fn responses(&self) -> Result<Responses, Error> {
        let possible = self.value("possible")?;
        let initial = self.value("initial")?;
        let auth_possible = self.value(self.stages[2])?;
        let auth = self.value(self.stages[3])?;
        let complete = self.value("complete")?;
        if possible.as_ref().is_some_and(|v| !v.is_null())
            || auth_possible.as_ref().is_some_and(|v| !v.is_null())
            || (initial.is_some() && possible.is_none())
            || (auth_possible.is_some() && initial.is_none())
            || (auth.is_some() && auth_possible.is_none())
            || (complete.is_some() && initial.is_none())
        {
            return Err(Error::Storage);
        }
        Ok(Responses {
            possible: possible.is_some(),
            auth_possible: auth_possible.is_some(),
            complete,
            initial: initial
                .map(serde_json::from_value)
                .transpose()
                .map_err(|_| Error::Storage)?,
            auth: auth
                .map(serde_json::from_value)
                .transpose()
                .map_err(|_| Error::Storage)?,
        })
    }
}
fn read(path: &Path, cap: usize) -> Result<Vec<u8>, Error> {
    let file = private::open(path, false).map_err(|_| Error::Storage)?;
    if file.metadata().map_err(|_| Error::Storage)?.len() > cap as u64 {
        return Err(Error::BodyTooLarge);
    }
    let mut value = Vec::new();
    file.take(cap as u64 + 1)
        .read_to_end(&mut value)
        .map_err(|_| Error::Storage)?;
    if value.len() > cap {
        return Err(Error::BodyTooLarge);
    }
    Ok(value)
}
