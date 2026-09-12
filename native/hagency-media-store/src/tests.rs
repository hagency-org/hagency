use super::*;
mod association;
mod preparation;
mod restoration;
use hagency_files::{RelativeFile, Workspace};
use hagency_media::Codec;
use std::{
    fs,
    io::{Read, Seek, SeekFrom, Write},
};

struct Fixture {
    root: tempfile::TempDir,
    workspace: Workspace,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        private::directory(&root.path().join("stage")).unwrap();
        private::directory(&root.path().join("work")).unwrap();
        let workspace = Workspace::from_directory(
            Dir::open_ambient_dir(root.path().join("work"), cap_std::ambient_authority()).unwrap(),
            hagency_files::Limits::new(1024 * 1024, 8).unwrap(),
        )
        .unwrap();
        Self { root, workspace }
    }
    fn directory(&self) -> Dir {
        Dir::open_ambient_dir(self.root.path().join("stage"), cap_std::ambient_authority()).unwrap()
    }
    fn create(&self, limits: Limits) -> Store {
        Store::create(self.directory(), namespace(), limits).unwrap()
    }
    fn reopen(&self, limits: Limits) -> Result<Store, Error> {
        Store::open(self.directory(), namespace(), limits)
    }
    fn snapshot(&self, bytes: &[u8]) -> hagency_files::Snapshot {
        fs::write(self.root.path().join("work/input.bin"), bytes).unwrap();
        self.workspace
            .snapshot(&RelativeFile::new("input.bin").unwrap())
            .unwrap()
    }
    fn plain(&self, bytes: &[u8]) -> Media {
        Media::Snapshot(self.snapshot(bytes))
    }
    fn journal(&self) -> std::path::PathBuf {
        self.root.path().join("stage/media.journal")
    }
}
fn namespace() -> HostNamespace {
    HostNamespace::new("private-host-partition").unwrap()
}
fn op(value: &str) -> OperationId {
    OperationId::new(value).unwrap()
}
fn codec() -> Codec {
    Codec::new(hagency_media::Limits::new(1024 * 1024, 8).unwrap())
}
fn assert_sync(evidence: SyncEvidence) {
    #[cfg(unix)]
    assert_eq!(evidence, SyncEvidence::FileAndDirectorySynced);
    #[cfg(windows)]
    assert!(matches!(
        evidence,
        SyncEvidence::FileAndDirectorySynced | SyncEvidence::FileSyncedDirectoryUnconfirmed
    ));
}

// Windows locks also reject a second handle opened by the same process. Inspect
// the actual owner, keep its lock and restore the cursor for the next operation.
fn journal_bytes(store: &mut Store) -> Vec<u8> {
    let cursor = store.file.stream_position().unwrap();
    store.file.seek(SeekFrom::Start(0)).unwrap();
    let mut bytes = Vec::new();
    Read::by_ref(&mut store.file)
        .take(store.limits.file_bytes + 1)
        .read_to_end(&mut bytes)
        .unwrap();
    store.file.seek(SeekFrom::Start(cursor)).unwrap();
    assert!(bytes.len() as u64 <= store.limits.file_bytes);
    bytes
}

#[test]
fn native_media_stage_directory_sync_handle() {
    let f = Fixture::new();
    let directory = f.directory();
    #[cfg(target_os = "linux")]
    {
        let original = directory.try_clone().unwrap().into_std_file();
        // Actual pinned cap-std O_PATH handle; Linux fsync reports EBADF.
        assert_eq!(original.sync_all().unwrap_err().raw_os_error(), Some(9));
    }
    #[cfg(unix)]
    {
        fs::rename(f.root.path().join("stage"), f.root.path().join("retained")).unwrap();
        private::directory(&f.root.path().join("stage")).unwrap();
    }
    #[allow(unused_mut)]
    let mut store = Store::create(directory, namespace(), Limits::default()).unwrap();
    assert_sync(store.sync);
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let retained = store
            ._directory
            .try_clone()
            .unwrap()
            .into_std_file()
            .metadata()
            .unwrap();
        let synced = store.directory_file.metadata().unwrap();
        assert_eq!(retained.dev(), synced.dev());
        assert_eq!(retained.ino(), synced.ino());
        store.directory_file.sync_all().unwrap();
        assert!(f.root.path().join("retained/media.journal").is_file());
        assert!(!f.journal().exists());
    }
    #[cfg(windows)]
    {
        // Removing the sealed owner is test-only negative evidence. A matching
        // original directory or successful journal flush must not replace it.
        store.windows_directory.take();
        assert_eq!(
            store.sync_storage().unwrap(),
            SyncEvidence::FileSyncedDirectoryUnconfirmed
        );
        let encrypted = codec()
            .encrypt(f.snapshot(b"unconfirmed original"))
            .unwrap();
        let receipt = store
            .stage(&op("unconfirmed"), Media::Encrypted(encrypted))
            .unwrap_or_else(|_| panic!("actual unconfirmed staging must retain its receipt"));
        assert_eq!(
            receipt.sync_evidence(),
            SyncEvidence::FileSyncedDirectoryUnconfirmed
        );
        assert!(matches!(
            store.restore_encrypted(&op("unconfirmed"), receipt.digest()),
            Err(Error::Durability)
        ));
        assert!(store.read(&op("unconfirmed")).is_ok());
    }
}

#[test]
fn native_media_stage_roundtrip() {
    let f = Fixture::new();
    let data = b"immutable private bytes\0\xff";
    let encrypted = codec().encrypt(f.snapshot(data)).unwrap();
    let ciphertext = encrypted.ciphertext().to_vec();
    let descriptor = encrypted.descriptor().private_event_json().to_vec();
    let checked = codec()
        .decrypt(encrypted.descriptor(), &ciphertext)
        .unwrap();
    let mut store = f.create(Limits::default());
    for (id, media) in [
        ("plain", f.plain(data)),
        ("encrypted", Media::Encrypted(encrypted)),
        ("checked", Media::Checked(checked)),
    ] {
        let receipt = store
            .stage(&op(id), media)
            .map_err(|failure| failure.error())
            .unwrap();
        assert!(!receipt.replayed());
        assert_sync(receipt.sync_evidence());
        assert_eq!(receipt.len(), data.len());
    }
    fs::write(
        f.root.path().join("work/input.bin"),
        b"later source contents",
    )
    .unwrap();
    drop(store);
    let mut store = f.reopen(Limits::default()).unwrap();
    assert_eq!(store.recovery(), Recovery::Clean);
    for id in ["plain", "checked"] {
        let stored = store.read(&op(id)).unwrap();
        assert_eq!(stored.bytes(), data);
        assert!(stored.descriptor().is_none());
    }
    let stored = store.read(&op("encrypted")).unwrap();
    assert_eq!(stored.bytes(), ciphertext);
    assert_eq!(
        stored.descriptor().unwrap().private_event_json(),
        descriptor
    );
    assert_eq!(
        codec()
            .decrypt(stored.descriptor().unwrap(), stored.bytes())
            .unwrap()
            .bytes(),
        data
    );
}

#[test]
fn native_media_stage_replay() {
    let f = Fixture::new();
    let mut store = f.create(Limits::default());
    let first = store
        .stage(&op("stable"), f.plain(b"first"))
        .map_err(|failure| failure.error())
        .unwrap();
    let size = store.occupied_bytes();
    assert!(
        store
            .stage(&op("stable"), f.plain(b"first"))
            .map_err(|failure| failure.error())
            .unwrap()
            .replayed()
    );
    assert!(matches!(
        store
            .stage(&op("stable"), f.plain(b"different"))
            .map_err(|failure| failure.error()),
        Err(Error::Conflict)
    ));
    assert!(matches!(
        store
            .stage(
                &op("stable"),
                Media::Encrypted(codec().encrypt(f.snapshot(b"first")).unwrap())
            )
            .map_err(|failure| failure.error()),
        Err(Error::Conflict)
    ));
    assert_eq!(store.occupied_bytes(), size);
    drop(store);
    assert!(matches!(
        Store::open(
            f.directory(),
            HostNamespace::new("other").unwrap(),
            Limits::default()
        ),
        Err(Error::Identity)
    ));
    let mut store = f.reopen(Limits::default()).unwrap();
    let replay = store
        .stage(&op("stable"), f.plain(b"first"))
        .map_err(|failure| failure.error())
        .unwrap();
    assert!(replay.replayed());
    assert_eq!(replay.digest(), first.digest());
    assert_eq!(store.committed_records(), 1);
    for invalid in ["", "../path", "has space", "bad\0id"] {
        assert!(OperationId::new(invalid).is_err());
    }
    assert!(OperationId::new(&"a".repeat(129)).is_err());
    assert!(HostNamespace::new(&"n".repeat(257)).is_err());
    assert!(HostNamespace::new("").is_err());
    // A missing existing entry never silently creates a replacement journal.
    drop(store);
    fs::rename(f.journal(), f.root.path().join("stage/retained-original")).unwrap();
    assert!(f.reopen(Limits::default()).is_err());
    assert!(!f.journal().exists());
}

#[test]
fn native_media_stage_interruptions() {
    for boundary in [
        Checkpoint::IntentSynced,
        Checkpoint::PayloadChunk,
        Checkpoint::PayloadWritten,
        Checkpoint::CommitWritten,
        Checkpoint::Synced,
    ] {
        let f = Fixture::new();
        let mut store = f.create(Limits::default());
        store
            .stage(&op("earlier"), f.plain(b"known prefix"))
            .map_err(|failure| failure.error())
            .unwrap();
        let encrypted = codec().encrypt(f.snapshot(&vec![7; 100000])).unwrap();
        let ciphertext = encrypted.ciphertext().to_vec();
        let descriptor = encrypted.descriptor().private_event_json().to_vec();
        assert!(matches!(
            store
                .stage_inner(&op("interrupted"), Media::Encrypted(encrypted), |phase| {
                    if phase == boundary {
                        Err(Error::OutcomeUnknown)
                    } else {
                        Ok(())
                    }
                })
                .map_err(|failure| failure.error()),
            Err(Error::OutcomeUnknown)
        ));
        assert_eq!(store.recovery(), Recovery::WriteOutcomeUnknown);
        assert!(store.pending.is_some());
        assert!(matches!(
            store
                .stage(&op("new_id"), f.plain(b"no resend"))
                .map_err(|failure| failure.error()),
            Err(Error::OutcomeUnknown)
        ));
        assert_eq!(store.read(&op("earlier")).unwrap().bytes(), b"known prefix");
        let size = fs::metadata(f.journal()).unwrap().len();
        drop(store);
        let mut reopened = f.reopen(Limits::default()).unwrap();
        assert_eq!(fs::metadata(f.journal()).unwrap().len(), size);
        assert_eq!(
            reopened.read(&op("earlier")).unwrap().bytes(),
            b"known prefix"
        );
        let complete = matches!(boundary, Checkpoint::CommitWritten | Checkpoint::Synced);
        if complete {
            assert_eq!(reopened.recovery(), Recovery::Clean);
            let stored = reopened.read(&op("interrupted")).unwrap();
            assert_eq!(stored.bytes(), ciphertext);
            assert_eq!(
                stored.descriptor().unwrap().private_event_json(),
                descriptor
            );
            assert_eq!(reopened.committed_records(), 2);
        } else {
            assert_eq!(reopened.recovery(), Recovery::IncompleteTail);
            assert!(matches!(
                reopened.read(&op("interrupted")),
                Err(Error::NotFound)
            ));
            assert!(matches!(
                reopened
                    .stage(&op("new_id"), f.plain(b"not retried"))
                    .map_err(|failure| failure.error()),
                Err(Error::OutcomeUnknown)
            ));
        }
    }
    // Actual interrupted writes at every fixed framing boundary, not just hooks.
    let f = Fixture::new();
    let mut store = f.create(Limits::default());
    store
        .stage(&op("earlier"), f.plain(b"known"))
        .map_err(|failure| failure.error())
        .unwrap();
    let prefix = store.occupied_bytes();
    store
        .stage(&op("next"), f.plain(b"bounded payload"))
        .map_err(|failure| failure.error())
        .unwrap();
    drop(store);
    let bytes = fs::read(f.journal()).unwrap();
    for end in [
        prefix + 1,
        prefix + 191,
        prefix + 192,
        prefix + 194,
        bytes.len() as u64 - 1,
    ] {
        let mut file = private::open(&f.journal(), false).unwrap();
        file.set_len(0).unwrap();
        file.write_all(&bytes[..end as usize]).unwrap();
        file.sync_all().unwrap();
        drop(file);
        let mut reopened = f.reopen(Limits::default()).unwrap();
        assert_eq!(reopened.recovery(), Recovery::IncompleteTail);
        assert_eq!(reopened.read(&op("earlier")).unwrap().bytes(), b"known");
        assert_eq!(reopened.occupied_bytes(), end);
    }
}

#[test]
fn native_media_stage_bounds() {
    let f = Fixture::new();
    let limits = Limits::new(512, 1024, 2, 1).unwrap();
    let mut store = f.create(limits);
    assert!(matches!(
        store
            .stage(&op("too_big"), f.plain(&[0; 513]))
            .map_err(|failure| failure.error()),
        Err(Error::Capacity)
    ));
    store
        .stage(&op("a"), f.plain(&[1; 300]))
        .map_err(|failure| failure.error())
        .unwrap();
    assert!(matches!(
        store
            .stage(&op("b"), f.plain(&[2; 300]))
            .map_err(|failure| failure.error()),
        Err(Error::Capacity)
    ));
    store
        .stage(&op("b"), f.plain(b"small"))
        .map_err(|failure| failure.error())
        .unwrap();
    assert!(matches!(
        store
            .stage(&op("c"), f.plain(b"small"))
            .map_err(|failure| failure.error()),
        Err(Error::Capacity)
    ));
    assert!(
        store
            .stage(&op("a"), f.plain(&[1; 300]))
            .map_err(|failure| failure.error())
            .unwrap()
            .replayed()
    );
    let held = store.read(&op("a")).unwrap();
    assert!(matches!(store.read(&op("b")), Err(Error::Capacity)));
    drop(held);
    assert_eq!(store.read(&op("b")).unwrap().bytes(), b"small");
    // Once corruption is actually observed, repairing bytes cannot silently
    // reopen admission in the same owner. Recovery requires validated reopen.
    let location = store.entries.get("a").unwrap().offset + frame::INTENT as u64 + 1;
    store.file.seek(SeekFrom::Start(location)).unwrap();
    store.file.write_all(&[9]).unwrap();
    assert!(matches!(store.read(&op("a")), Err(Error::Corrupt)));
    store.file.seek(SeekFrom::Start(location)).unwrap();
    store.file.write_all(&[1]).unwrap();
    assert!(matches!(
        store
            .stage(&op("after_corruption"), f.plain(b"refused"))
            .map_err(|failure| failure.error()),
        Err(Error::OutcomeUnknown)
    ));
    drop(store);
    assert!(matches!(
        f.reopen(Limits::new(512, 1024, 1, 1).unwrap()),
        Err(Error::Capacity)
    ));
    let bytes = fs::read(f.journal()).unwrap();
    for offset in [
        FILE_HEADER as usize,
        FILE_HEADER as usize + 160,
        FILE_HEADER as usize + 200,
        bytes.len() - 1,
    ] {
        let mut corrupted = bytes.clone();
        corrupted[offset] ^= 1;
        fs::write(f.journal(), &corrupted).unwrap();
        assert!(matches!(f.reopen(limits), Err(Error::Corrupt)));
    }
    // Oversized claimed lengths are rejected before payload allocation/read.
    let mut corrupted = bytes.clone();
    corrupted[FILE_HEADER as usize + 16..FILE_HEADER as usize + 24]
        .copy_from_slice(&u64::MAX.to_le_bytes());
    fs::write(f.journal(), &corrupted).unwrap();
    assert!(matches!(f.reopen(limits), Err(Error::Capacity)));
    fs::write(f.journal(), &bytes).unwrap();
    let file = private::open(&f.journal(), false).unwrap();
    file.set_len(1025).unwrap();
    drop(file);
    assert!(matches!(f.reopen(limits), Err(Error::Capacity)));
    assert!(Limits::new(1, MAX_FILE_BYTES + 1, 1, 1).is_err());
    assert!(Limits::new(MAX_ITEM_BYTES + 1, 1024, 1, 1).is_err());
    assert!(Limits::new(1, 1024, MAX_RECORDS + 1, 1).is_err());
    assert!(Limits::new(1, 1024, 1, MAX_RESULTS + 1).is_err());
}

#[test]
fn native_media_stage_disk_failure() {
    let f = Fixture::new();
    let mut store = f.create(Limits::default());
    store
        .stage(&op("known"), f.plain(b"retained"))
        .map_err(|failure| failure.error())
        .unwrap();
    // Keep the actual original lock owner alive. A second read-only descriptor
    // performs the real refused OS write; no fake successful persistence is used.
    let read_only = File::open(f.journal()).unwrap();
    let original = std::mem::replace(&mut store.file, read_only);
    assert!(matches!(
        store
            .stage(&op("write_refused"), f.plain(b"new data"))
            .map_err(|failure| failure.error()),
        Err(Error::OutcomeUnknown)
    ));
    let failed = std::mem::replace(&mut store.file, original);
    drop(failed);
    assert_eq!(store.recovery(), Recovery::WriteOutcomeUnknown);
    assert!(store.pending.is_some());
    assert_eq!(store.read(&op("known")).unwrap().bytes(), b"retained");
    assert!(matches!(
        store
            .stage(&op("other"), f.plain(b"no new operation"))
            .map_err(|failure| failure.error()),
        Err(Error::OutcomeUnknown)
    ));
    drop(store);
    let mut reopened = f.reopen(Limits::default()).unwrap();
    assert_eq!(reopened.read(&op("known")).unwrap().bytes(), b"retained");
    assert!(matches!(
        reopened.read(&op("write_refused")),
        Err(Error::NotFound)
    ));
}

#[test]
fn native_media_stage_platform() {
    let f = Fixture::new();
    let mut store = f.create(Limits::default());
    assert!(matches!(f.reopen(Limits::default()), Err(Error::Locked)));
    assert_sync(store.sync_evidence());
    let retained = f.root.path().join("retained-stage");
    #[cfg(unix)]
    {
        fs::rename(f.root.path().join("stage"), &retained).unwrap();
        private::directory(&f.root.path().join("stage")).unwrap();
        store
            .stage(&op("original-directory"), f.plain(b"held directory"))
            .map_err(|failure| failure.error())
            .unwrap();
        assert!(!f.journal().exists());
        assert!(fs::metadata(retained.join("media.journal")).unwrap().len() > FILE_HEADER);
        assert_eq!(
            store.read(&op("original-directory")).unwrap().bytes(),
            b"held directory"
        );
    }
    #[cfg(windows)]
    {
        assert_eq!(
            fs::rename(f.root.path().join("stage"), &retained)
                .unwrap_err()
                .raw_os_error(),
            Some(32)
        );
        store
            .stage(&op("held-directory"), f.plain(b"held directory"))
            .map_err(|failure| failure.error())
            .unwrap();
    }
    drop(store);
    #[cfg(windows)]
    fs::rename(f.root.path().join("stage"), &retained).unwrap();
    let f = Fixture::new();
    let mut store = f.create(Limits::default());
    store
        .stage(&op("entry"), f.plain(b"original"))
        .map_err(|failure| failure.error())
        .unwrap();
    drop(store);
    fs::hard_link(f.journal(), f.root.path().join("stage/other-link")).unwrap();
    assert!(matches!(f.reopen(Limits::default()), Err(Error::Private)));
    fs::remove_file(f.root.path().join("stage/other-link")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::{PermissionsExt, symlink};
        fs::set_permissions(f.journal(), fs::Permissions::from_mode(0o644)).unwrap();
        assert!(matches!(f.reopen(Limits::default()), Err(Error::Private)));
        fs::set_permissions(f.journal(), fs::Permissions::from_mode(0o600)).unwrap();
        fs::rename(f.journal(), f.root.path().join("stage/saved")).unwrap();
        symlink("saved", f.journal()).unwrap();
        assert!(f.reopen(Limits::default()).is_err());
        fs::remove_file(f.journal()).unwrap();
        fs::rename(f.root.path().join("stage/saved"), f.journal()).unwrap();
        fs::set_permissions(
            f.root.path().join("stage"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        assert!(matches!(f.reopen(Limits::default()), Err(Error::Private)));
    }
    #[cfg(windows)]
    {
        grant_world_read(&f.journal());
        assert!(matches!(f.reopen(Limits::default()), Err(Error::Private)));
        // Existing even-empty public files must never enter creation sealing.
        let existing = fs::OpenOptions::new()
            .write(true)
            .open(f.journal())
            .unwrap();
        existing.set_len(0).unwrap();
        drop(existing);
        assert!(Store::create(f.directory(), namespace(), Limits::default()).is_err());
        assert!(matches!(
            private::check_handle(&File::open(f.journal()).unwrap()),
            Err(hagency_store::Error::Private)
        ));
        assert_eq!(fs::metadata(f.journal()).unwrap().len(), 0);
        let directory_fixture = Fixture::new();
        grant_world_read(&directory_fixture.root.path().join("stage"));
        assert!(matches!(
            Store::create(
                directory_fixture.directory(),
                namespace(),
                Limits::default()
            ),
            Err(Error::Private)
        ));
    }
    #[cfg(windows)]
    {
        // A separate private fixture keeps the reparse refusal independent of
        // the deliberately public ACL above.
        let f = Fixture::new();
        drop(f.create(Limits::default()));
        let saved = f.root.path().join("stage/saved");
        fs::rename(f.journal(), &saved).unwrap();
        std::os::windows::fs::symlink_file(&saved, f.journal()).unwrap();
        assert!(f.reopen(Limits::default()).is_err());
    }
}

#[cfg(windows)]
fn grant_world_read(path: &std::path::Path) {
    use std::{
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    // One fixed administrative OS command against this test-owned file only.
    // Discard potentially path-bearing output and retain the child until exit.
    let mut child = Command::new("icacls.exe")
        .arg(path)
        .arg("/grant")
        .arg("*S-1-1-0:(R)")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("private ACL fixture exceeded its deadline");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn native_media_stage_replay_custody() {
    fn encrypted(f: &Fixture) -> (Media, Vec<u8>, Vec<u8>) {
        let encrypted = codec()
            .encrypt(f.snapshot(b"retain original keys and handles"))
            .unwrap();
        let bytes = encrypted.ciphertext().to_vec();
        let descriptor = encrypted.descriptor().private_event_json().to_vec();
        (Media::Encrypted(encrypted), bytes, descriptor)
    }
    fn returned(failure: StageFailure, expected: Error, bytes: &[u8], descriptor: &[u8]) {
        assert_eq!(failure.error(), expected);
        assert_eq!(failure.custody(), FailureCustody::ReturnedUnadmitted);
        let original = failure.into_unadmitted().unwrap();
        assert_eq!(original.bytes(), bytes);
        assert_eq!(original.descriptor(), descriptor);
    }
    let f = Fixture::new();
    let mut store = f.create(Limits::new(1024, 4096, 1, 1).unwrap());
    store
        .stage(&op("full"), f.plain(b"committed"))
        .map_err(|failure| failure.error())
        .unwrap();
    for (id, expected) in [("next", Error::Capacity), ("full", Error::Conflict)] {
        let (media, bytes, descriptor) = encrypted(&f);
        returned(
            store.stage(&op(id), media).err().unwrap(),
            expected,
            &bytes,
            &descriptor,
        );
        assert!(store.pending.is_none());
    }
    drop(store);
    let f = Fixture::new();
    let mut store = f.create(Limits::default());
    let (media, bytes, descriptor) = encrypted(&f);
    let failed = store
        .stage_inner(&op("original"), media, |_| Err(Error::OutcomeUnknown))
        .err()
        .unwrap();
    assert_eq!(failed.custody(), FailureCustody::RetainedByStore);
    assert!(failed.into_unadmitted().is_none());
    assert_eq!(store.pending.as_ref().unwrap().bytes(), bytes);
    assert_eq!(store.pending.as_ref().unwrap().descriptor(), descriptor);
    let (new, new_bytes, new_descriptor) = encrypted(&f);
    returned(
        store.stage(&op("second"), new).err().unwrap(),
        Error::OutcomeUnknown,
        &new_bytes,
        &new_descriptor,
    );
    assert_eq!(store.pending.as_ref().unwrap().descriptor(), descriptor);
    drop(store);
    let f = Fixture::new();
    let mut store = f.create(Limits::default());
    // Actual external hardlink creation invalidates the retained file check.
    fs::hard_link(f.journal(), f.root.path().join("stage/extra-link")).unwrap();
    let (media, bytes, descriptor) = encrypted(&f);
    returned(
        store
            .stage(&op("private_check_failed"), media)
            .err()
            .unwrap(),
        Error::Private,
        &bytes,
        &descriptor,
    );
    assert!(store.pending.is_none());
    assert_eq!(store.recovery(), Recovery::WriteOutcomeUnknown);
    drop(store);
    // The original upstream RAII permit is held too, not only copied key text.
    let f = Fixture::new();
    let constrained = Workspace::from_directory(
        Dir::open_ambient_dir(f.root.path().join("work"), cap_std::ambient_authority()).unwrap(),
        hagency_files::Limits::new(1024, 1).unwrap(),
    )
    .unwrap();
    fs::write(f.root.path().join("work/input.bin"), b"retained source").unwrap();
    let selection = RelativeFile::new("input.bin").unwrap();
    let mut store = f.create(Limits::new(1024, 4096, 1, 1).unwrap());
    store
        .stage(&op("full"), f.plain(b"already staged"))
        .map_err(|f| f.error())
        .unwrap();
    let media = Media::Encrypted(
        codec()
            .encrypt(constrained.snapshot(&selection).unwrap())
            .unwrap(),
    );
    let failure = store.stage(&op("not_admitted"), media).err().unwrap();
    assert!(matches!(
        constrained.snapshot(&selection),
        Err(hagency_files::Error::Capacity)
    ));
    let returned = failure.into_unadmitted().unwrap();
    assert!(matches!(
        constrained.snapshot(&selection),
        Err(hagency_files::Error::Capacity)
    ));
    drop(returned);
    drop(constrained.snapshot(&selection).unwrap());
    drop(store);
    // A different file owner gives the write-phase case its own fresh journal.
    let g = Fixture::new();
    let mut store = g.create(Limits::default());
    let media = Media::Encrypted(
        codec()
            .encrypt(constrained.snapshot(&selection).unwrap())
            .unwrap(),
    );
    let failure = store
        .stage_inner(&op("pending"), media, |_| Err(Error::OutcomeUnknown))
        .err()
        .unwrap();
    assert_eq!(failure.custody(), FailureCustody::RetainedByStore);
    drop(failure);
    assert!(matches!(
        constrained.snapshot(&selection),
        Err(hagency_files::Error::Capacity)
    ));
    drop(store);
    drop(constrained.snapshot(&selection).unwrap());
}
