use super::*;

fn encrypted(f: &Fixture, data: &[u8]) -> hagency_media::Encrypted {
    codec().encrypt(f.snapshot(data)).unwrap()
}
fn returned(failure: StageFailure, expected: Error, data: &[u8]) {
    assert_eq!(failure.error(), expected);
    assert_eq!(failure.custody(), FailureCustody::ReturnedUnadmitted);
    let Some(Media::Encrypted(original)) = failure.into_unadmitted() else {
        panic!("original encrypted custody was not returned");
    };
    assert_eq!(
        codec()
            .decrypt(original.descriptor(), original.ciphertext())
            .unwrap()
            .bytes(),
        data
    );
}
fn plan(f: &Fixture, store: &mut Store, id: &str, data: &[u8]) -> Option<PreparedEncrypted> {
    let before = journal_bytes(store);
    assert_sync(store.sync);
    let result = store.prepare_encrypted(&op(id), encrypted(f, data));
    let result = match store.sync {
        SyncEvidence::FileAndDirectorySynced => Some(result.map_err(|e| e.error()).unwrap()),
        SyncEvidence::FileSyncedDirectoryUnconfirmed => {
            returned(result.err().unwrap(), Error::Durability, data);
            None
        }
    };
    assert_eq!(journal_bytes(store), before);
    result
}

#[test]
fn native_media_prepare_exact_identity() {
    let f = Fixture::new();
    let mut store = f.create(Limits::default());
    let data = b"original encrypted source";
    let encrypted = encrypted(&f, data);
    let ciphertext = encrypted.ciphertext().to_vec();
    let descriptor = encrypted.descriptor().private_event_json().to_vec();
    let before = journal_bytes(&mut store);
    let result = store.prepare_encrypted(&op("original"), encrypted);
    assert_eq!(journal_bytes(&mut store), before);
    assert_sync(store.sync);
    match store.sync {
        SyncEvidence::FileAndDirectorySynced => {
            let prepared = result.map_err(|e| e.error()).unwrap();
            let digest = *prepared.digest();
            assert_eq!(prepared.namespace().digest(), namespace().digest());
            assert_eq!(prepared.operation().as_str(), "original");
            assert_eq!(prepared.len(), data.len());
            assert!(!prepared.is_empty());
            store
                .stage(&op("intervening"), f.plain(b"other append"))
                .map_err(|e| e.error())
                .unwrap();
            fs::write(f.root.path().join("work/input.bin"), b"changed source").unwrap();
            let receipt = store
                .stage_prepared(prepared)
                .map_err(|e| e.error())
                .unwrap();
            assert_eq!(receipt.digest(), &digest);
            assert_eq!(store.committed_records(), 2);
            let stored = store.read(&op("original")).unwrap();
            assert_eq!(stored.bytes(), ciphertext);
            assert_eq!(
                stored.descriptor().unwrap().private_event_json(),
                descriptor
            );
            drop(stored);
            drop(store);
            let mut reopened = f.reopen(Limits::default()).unwrap();
            let restored = reopened.restore_encrypted(&op("original"), &digest);
            match reopened.sync {
                SyncEvidence::FileAndDirectorySynced => {
                    let restored = restored.unwrap();
                    assert_eq!(restored.ciphertext(), ciphertext);
                    assert_eq!(restored.descriptor().private_event_json(), descriptor);
                }
                SyncEvidence::FileSyncedDirectoryUnconfirmed => {
                    assert!(matches!(restored, Err(Error::Durability)));
                }
            }
        }
        SyncEvidence::FileSyncedDirectoryUnconfirmed => {
            returned(result.err().unwrap(), Error::Durability, data);
            assert_eq!(store.committed_records(), 0);
        }
    }
}

#[test]
fn native_media_prepare_capacity() {
    let f = Fixture::new();
    let mut store = f.create(Limits::new(1024, 16384, 2, 1).unwrap());
    store
        .stage(&op("plain"), f.plain(b"generic read"))
        .map_err(|e| e.error())
        .unwrap();
    let held = store.read(&op("plain")).unwrap();
    returned(
        store
            .prepare_encrypted(&op("blocked"), encrypted(&f, b"first"))
            .err()
            .unwrap(),
        Error::Capacity,
        b"first",
    );
    drop(held);
    if let Some(prepared) = plan(&f, &mut store, "original", b"exact material") {
        assert!(matches!(store.read(&op("plain")), Err(Error::Capacity)));
        returned(
            store
                .prepare_encrypted(&op("excess"), encrypted(&f, b"second"))
                .err()
                .unwrap(),
            Error::Capacity,
            b"second",
        );
        // A held plan reserves memory, not journal space. Another accepted
        // append can fill storage before the prepared write is attempted.
        store
            .stage(&op("filled"), f.plain(b"last record"))
            .map_err(|e| e.error())
            .unwrap();
        let before = journal_bytes(&mut store);
        returned(
            store.stage_prepared(prepared).err().unwrap(),
            Error::Capacity,
            b"exact material",
        );
        assert_eq!(journal_bytes(&mut store), before);
    }
    assert_eq!(store.read(&op("plain")).unwrap().bytes(), b"generic read");
    // Negative-only qualification test; never create positive OS evidence.
    store.sync = SyncEvidence::FileSyncedDirectoryUnconfirmed;
    returned(
        store
            .prepare_encrypted(&op("unqualified"), encrypted(&f, b"no qualification"))
            .err()
            .unwrap(),
        Error::Durability,
        b"no qualification",
    );
}

#[test]
fn native_media_prepare_owner_identity() {
    let f = Fixture::new();
    let other = Fixture::new();
    let mut store = f.create(Limits::default());
    let mut second = other.create(Limits::default());
    if let Some(prepared) = plan(&f, &mut store, "original", b"same namespace") {
        let before = journal_bytes(&mut second);
        returned(
            second.stage_prepared(prepared).err().unwrap(),
            Error::Identity,
            b"same namespace",
        );
        assert_eq!(journal_bytes(&mut second), before);
        assert_eq!(store.committed_records(), 0);
    }
    if let Some(prepared) = plan(&f, &mut store, "retained", b"old owner") {
        drop(store);
        let mut reopened = f.reopen(Limits::default()).unwrap();
        let before = journal_bytes(&mut reopened);
        returned(
            reopened.stage_prepared(prepared).err().unwrap(),
            Error::Identity,
            b"old owner",
        );
        assert_eq!(journal_bytes(&mut reopened), before);
        assert_eq!(reopened.committed_records(), 0);
    }
}

#[test]
fn native_media_prepare_failure_custody() {
    let f = Fixture::new();
    let mut store = f.create(Limits::default());
    if let Some(prepared) = plan(&f, &mut store, "original", b"before pending write") {
        let failure = store
            .stage_inner(&op("interrupted"), f.plain(b"tail"), |phase| {
                if phase == Checkpoint::IntentSynced {
                    Err(Error::OutcomeUnknown)
                } else {
                    Ok(())
                }
            })
            .err()
            .unwrap();
        assert_eq!(failure.custody(), FailureCustody::RetainedByStore);
        let before = journal_bytes(&mut store);
        returned(
            store.stage_prepared(prepared).err().unwrap(),
            Error::OutcomeUnknown,
            b"before pending write",
        );
        assert_eq!(journal_bytes(&mut store), before);
        assert!(store.pending.is_some());
        assert_eq!(store.recovery(), Recovery::WriteOutcomeUnknown);
    }
    let f = Fixture::new();
    let mut store = f.create(Limits::default());
    if let Some(prepared) = plan(&f, &mut store, "write_refused", b"retained original") {
        let original = std::mem::replace(&mut store.file, File::open(f.journal()).unwrap());
        let failure = store.stage_prepared(prepared).err().unwrap();
        assert_eq!(failure.error(), Error::OutcomeUnknown);
        assert_eq!(failure.custody(), FailureCustody::RetainedByStore);
        assert!(failure.into_unadmitted().is_none());
        assert_eq!(store.recovery(), Recovery::WriteOutcomeUnknown);
        let Some(Media::Encrypted(media)) = store.pending.as_ref() else {
            panic!("possible write lost its original encrypted material");
        };
        assert_eq!(
            codec()
                .decrypt(media.descriptor(), media.ciphertext())
                .unwrap()
                .bytes(),
            b"retained original"
        );
        let read_only = std::mem::replace(&mut store.file, original);
        drop(read_only);
        assert_eq!(store.committed_records(), 0);
    }
}
