#![forbid(unsafe_code)]
//! Bounded private storage only: no runtime, dispatch, Matrix or sender authority.
mod frame;
mod preparation;
mod restoration;
mod types;
use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt, OpenOptionsSyncExt};
use cap_std::fs::{Dir, OpenOptions};
pub use frame::encrypted_receipt_matches;
use frame::{Entry, FILE_HEADER, Scan};
use hagency_store::private;
use std::{
    collections::BTreeMap,
    fs::File,
    io::{Seek, SeekFrom, Write},
    sync::{Arc, atomic::AtomicUsize},
};
pub use types::*;

const JOURNAL: &str = "media.journal";
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum Error {
    #[error("invalid media storage limits")]
    Limit,
    #[error("invalid or mismatched media storage identity")]
    Identity,
    #[error("private media storage capacity exhausted")]
    Capacity,
    #[error("private media storage permission or object check failed")]
    Private,
    #[error("private media storage already has an owner")]
    Locked,
    #[error("private media storage I/O failed")]
    Io,
    #[error("media staging outcome is unknown; new admission is quarantined")]
    OutcomeUnknown,
    #[error("private media storage is corrupt")]
    Corrupt,
    #[error("operation identity conflicts with original media")]
    Conflict,
    #[error("staged media was not found")]
    NotFound,
    #[error("encrypted restoration requires qualified file and directory sync")]
    Durability,
}
/// Owns actual directory/file capabilities. No public path or handle accessor.
/// Blocking local IO has no hard kernel deadline and is not a service worker.
pub struct Store {
    _directory: Dir,
    directory_file: File,
    #[cfg(windows)]
    windows_directory: Option<private::WindowsDirectorySync>,
    file: File,
    namespace: HostNamespace,
    limits: Limits,
    entries: BTreeMap<String, Entry>,
    length: u64,
    chain: [u8; 32],
    recovery: Recovery,
    sync: SyncEvidence,
    pending: Option<Media>,
    pool: Arc<Pool>,
}
impl Store {
    pub fn create(directory: Dir, namespace: HostNamespace, limits: Limits) -> Result<Self, Error> {
        Self::open_inner(directory, namespace, limits, true)
    }
    /// Opens only an existing journal. Missing storage is never silently initialized.
    pub fn open(directory: Dir, namespace: HostNamespace, limits: Limits) -> Result<Self, Error> {
        Self::open_inner(directory, namespace, limits, false)
    }
    fn open_inner(
        directory: Dir,
        namespace: HostNamespace,
        limits: Limits,
        create: bool,
    ) -> Result<Self, Error> {
        let directory_file = directory
            .try_clone()
            .map_err(|_| Error::Io)?
            .into_std_file();
        if !directory_file.metadata().map_err(|_| Error::Io)?.is_dir() {
            return Err(Error::Private);
        }
        private::check_handle(&directory_file).map_err(|_| Error::Private)?;
        #[cfg(target_os = "linux")]
        let directory_file = {
            use std::os::unix::fs::MetadataExt;
            // cap-std directory capabilities use O_PATH on Linux. Duplicating
            // that descriptor cannot make fsync work. Open only the retained
            // directory itself for reading, never its former ambient pathname.
            let readable = directory.open(".").map_err(|_| Error::Io)?.into_std();
            let original = directory_file.metadata().map_err(|_| Error::Io)?;
            let actual = readable.metadata().map_err(|_| Error::Io)?;
            if !actual.is_dir() || original.dev() != actual.dev() || original.ino() != actual.ino()
            {
                return Err(Error::Private);
            }
            private::check_handle(&readable).map_err(|_| Error::Private)?;
            readable
        };
        #[cfg(windows)]
        let windows_directory =
            private::WindowsDirectorySync::open(&directory).map_err(|_| Error::Private)?;
        let mut options = OpenOptions::new();
        options
            .read(true)
            .write(true)
            .follow(FollowSymlinks::No)
            .nonblock(true);
        if create {
            options.create_new(true);
        }
        #[cfg(unix)]
        {
            use cap_std::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        #[cfg(windows)]
        if create {
            use cap_std::fs::OpenOptionsExt;
            use windows_sys::Win32::{
                Foundation::{GENERIC_READ, GENERIC_WRITE},
                Storage::FileSystem::{WRITE_DAC, WRITE_OWNER},
            };
            // cap-primitives4.0.3 passes access_mode to NtCreateFile with the
            // retained RootDirectory for this fixed normal component only.
            options.access_mode(GENERIC_READ | GENERIC_WRITE | WRITE_DAC | WRITE_OWNER);
        }
        let mut file = directory
            .open_with(JOURNAL, &options)
            .map_err(|_| Error::Io)?
            .into_std();
        if !file.metadata().map_err(|_| Error::Io)?.is_file() {
            return Err(Error::Private);
        }
        if !create {
            private::check_handle(&file).map_err(|_| Error::Private)?;
        }
        file.try_lock().map_err(|_| Error::Locked)?;
        if create {
            // Only this successful create_new branch may seal. On refusal the
            // empty entry remains; no header or secret bytes have been written.
            private::seal_created_file_handle(&file).map_err(|_| Error::OutcomeUnknown)?;
            file.write_all(&frame::header(&namespace))
                .map_err(|_| Error::OutcomeUnknown)?;
            file.sync_all().map_err(|_| Error::OutcomeUnknown)?;
        }
        let length = file.metadata().map_err(|_| Error::Io)?.len();
        if length > limits.file_bytes {
            return Err(Error::Capacity);
        }
        let chain = frame::validate_header(&mut file, &namespace)?;
        let mut store = Self {
            _directory: directory,
            directory_file,
            #[cfg(windows)]
            windows_directory,
            file,
            namespace,
            limits,
            entries: BTreeMap::new(),
            length,
            chain,
            recovery: Recovery::Clean,
            sync: SyncEvidence::FileSyncedDirectoryUnconfirmed,
            pending: None,
            pool: Arc::new(Pool {
                held: AtomicUsize::new(0),
                limit: limits.results,
            }),
        };
        let mut offset = FILE_HEADER;
        while offset < length {
            if store.entries.len() >= limits.records {
                return Err(Error::Capacity);
            }
            match frame::scan(
                &mut store.file,
                &store.namespace,
                offset,
                length - offset,
                limits,
                store.chain,
                false,
            )? {
                Scan::Incomplete => {
                    store.recovery = Recovery::IncompleteTail;
                    break;
                }
                Scan::Complete(entry, _, _) => {
                    offset = offset.checked_add(entry.length).ok_or(Error::Capacity)?;
                    store.chain = entry.chain;
                    if store
                        .entries
                        .insert(entry.operation.0.clone(), entry)
                        .is_some()
                    {
                        return Err(Error::Corrupt);
                    }
                }
            }
        }
        // Only after validated recovery. Re-sync establishes current OS evidence,
        // never proof that the previous host's final flush actually completed.
        store.check()?;
        store.sync = store.sync_storage()?;
        store.check()?;
        Ok(store)
    }
    fn check(&self) -> Result<(), Error> {
        if self.length > self.limits.file_bytes {
            return Err(Error::Capacity);
        }
        private::check_handle(&self.directory_file).map_err(|_| Error::Private)?;
        private::check_handle(&self.file).map_err(|_| Error::Private)?;
        if self.file.metadata().map_err(|_| Error::Io)?.len() != self.length {
            return Err(Error::Corrupt);
        }
        Ok(())
    }
    fn sync_storage(&mut self) -> Result<SyncEvidence, Error> {
        self.file.sync_all().map_err(|_| Error::OutcomeUnknown)?;
        #[cfg(windows)]
        {
            // An ordinary read-only directory duplicate is never a fallback for
            // the qualified retained NTFS owner. Unsupported profiles remain
            // inspectable, without preparation/restoration durability authority.
            let acknowledged = self
                .windows_directory
                .as_mut()
                .is_some_and(|directory| directory.sync().is_ok());
            Ok(if acknowledged {
                SyncEvidence::FileAndDirectorySynced
            } else {
                SyncEvidence::FileSyncedDirectoryUnconfirmed
            })
        }
        #[cfg(not(windows))]
        {
            self.directory_file
                .sync_all()
                .map_err(|_| Error::OutcomeUnknown)?;
            Ok(SyncEvidence::FileAndDirectorySynced)
        }
    }
    pub fn recovery(&self) -> Recovery {
        self.recovery
    }
    pub fn sync_evidence(&self) -> SyncEvidence {
        self.sync
    }
    pub fn committed_records(&self) -> usize {
        self.entries.len()
    }
    pub fn occupied_bytes(&self) -> u64 {
        self.length
    }
    pub fn stage(
        &mut self,
        operation: &OperationId,
        media: Media,
    ) -> Result<Receipt, StageFailure> {
        self.stage_inner(operation, media, |_| Ok(()))
    }
    fn stage_inner(
        &mut self,
        operation: &OperationId,
        media: Media,
        mut checkpoint: impl FnMut(Checkpoint) -> Result<(), Error>,
    ) -> Result<Receipt, StageFailure> {
        if let Err(error) = self.check() {
            self.recovery = Recovery::WriteOutcomeUnknown;
            return Err(StageFailure::returned(error, media));
        }
        let prepared = match frame::prepare(
            &self.namespace,
            operation,
            &media,
            self.length,
            self.chain,
            self.limits,
        ) {
            Ok(value) => value,
            Err(error) => return Err(StageFailure::returned(error, media)),
        };
        if let Some(original) = self.entries.get(&operation.0).cloned() {
            if original.digest != prepared.entry.digest {
                return Err(StageFailure::returned(Error::Conflict, media));
            }
            if let Err(error) = self.validate(&original, false) {
                return Err(StageFailure::returned(error, media));
            }
            return Ok(self.receipt(&original, true));
        }
        if self.recovery != Recovery::Clean {
            return Err(StageFailure::returned(Error::OutcomeUnknown, media));
        }
        if self.entries.len() >= self.limits.records
            || self
                .length
                .checked_add(prepared.entry.length)
                .is_none_or(|v| v > self.limits.file_bytes)
        {
            return Err(StageFailure::returned(Error::Capacity, media));
        }
        // Ownership stays here even on error, including source handles and exact
        // ciphertext/keys. A synchronous caller cannot cancel this method mid-poll.
        self.pending = Some(media);
        let result: Result<SyncEvidence, Error> = (|| {
            self.file
                .seek(SeekFrom::Start(self.length))
                .map_err(|_| Error::Io)?;
            self.file
                .write_all(&prepared.intent)
                .map_err(|_| Error::Io)?;
            self.file
                .write_all(operation.0.as_bytes())
                .map_err(|_| Error::Io)?;
            self.file.sync_all().map_err(|_| Error::OutcomeUnknown)?;
            checkpoint(Checkpoint::IntentSynced)?;
            let media = self.pending.as_ref().ok_or(Error::OutcomeUnknown)?;
            for chunk in media.bytes().chunks(64 * 1024) {
                self.file.write_all(chunk).map_err(|_| Error::Io)?;
                checkpoint(Checkpoint::PayloadChunk)?;
            }
            self.file
                .write_all(media.descriptor())
                .map_err(|_| Error::Io)?;
            checkpoint(Checkpoint::PayloadWritten)?;
            self.file.write_all(frame::COMMIT).map_err(|_| Error::Io)?;
            self.file
                .write_all(&prepared.entry.chain)
                .map_err(|_| Error::Io)?;
            checkpoint(Checkpoint::CommitWritten)?;
            let sync = self.sync_storage()?;
            checkpoint(Checkpoint::Synced)?;
            Ok(sync)
        })();
        match result {
            Ok(sync) => {
                self.length += prepared.entry.length;
                self.chain = prepared.entry.chain;
                self.sync = sync;
                let receipt = self.receipt(&prepared.entry, false);
                self.entries.insert(operation.0.clone(), prepared.entry);
                self.pending = None;
                Ok(receipt)
            }
            Err(_) => {
                self.recovery = Recovery::WriteOutcomeUnknown;
                // Exact metadata failure is unknown too; this upper bound still
                // reserves the admitted interrupted bytes without allowing growth.
                self.length = self
                    .file
                    .metadata()
                    .map(|m| m.len())
                    .unwrap_or(self.length + prepared.entry.length);
                Err(StageFailure::retained(Error::OutcomeUnknown))
            }
        }
    }
    fn receipt(&self, entry: &Entry, replayed: bool) -> Receipt {
        Receipt {
            operation: entry.operation.clone(),
            kind: entry.kind,
            len: entry.bytes,
            digest: entry.digest,
            sync: self.sync,
            replayed,
        }
    }
    fn validate(&mut self, entry: &Entry, capture: bool) -> Result<(Vec<u8>, Vec<u8>), Error> {
        let result = (|| {
            self.check()?;
            match frame::scan(
                &mut self.file,
                &self.namespace,
                entry.offset,
                entry.length,
                self.limits,
                entry.previous,
                capture,
            )? {
                Scan::Complete(current, bytes, descriptor)
                    if current.digest == entry.digest && current.chain == entry.chain =>
                {
                    Ok((bytes, descriptor))
                }
                _ => Err(Error::Corrupt),
            }
        })();
        if result.is_err() {
            self.recovery = Recovery::WriteOutcomeUnknown;
        }
        result
    }
    pub fn read(&mut self, operation: &OperationId) -> Result<StagedMedia, Error> {
        let entry = self
            .entries
            .get(&operation.0)
            .cloned()
            .ok_or(Error::NotFound)?;
        let permit = Permit::acquire(&self.pool)?;
        let (bytes, descriptor) = self.validate(&entry, true)?;
        let descriptor = if entry.kind == Kind::Encrypted {
            Some(
                hagency_media::Descriptor::from_private_event_json(&descriptor)
                    .map_err(|_| Error::Corrupt)?,
            )
        } else {
            None
        };
        Ok(StagedMedia {
            bytes,
            descriptor,
            receipt: self.receipt(&entry, true),
            _permit: permit,
        })
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Checkpoint {
    IntentSynced,
    PayloadChunk,
    PayloadWritten,
    CommitWritten,
    Synced,
}

#[cfg(test)]
mod tests;
