use super::*;

fn stage_encrypted(f: &Fixture, store: &mut Store, id: &str, data: &[u8]) -> Receipt {
    store
        .stage(
            &op(id),
            Media::Encrypted(codec().encrypt(f.snapshot(data)).unwrap()),
        )
        .map_err(|failure| failure.error())
        .unwrap()
}

#[test]
fn native_media_restore_exact_ciphertext() {
    let f = Fixture::new();
    let data = b"original encrypted content\0\xff";
    let encrypted = codec().encrypt(f.snapshot(data)).unwrap();
    let ciphertext = encrypted.ciphertext().to_vec();
    let descriptor = encrypted.descriptor().private_event_json().to_vec();
    let mut store = f.create(Limits::default());
    let original = store
        .stage(&op("original"), Media::Encrypted(encrypted))
        .map_err(|failure| failure.error())
        .unwrap();
    let journal = journal_bytes(&mut store);
    drop(store);
    fs::write(f.root.path().join("work/input.bin"), b"source replaced").unwrap();
    let mut store = f.reopen(Limits::default()).unwrap();
    assert_sync(store.sync);
    let restored = store.restore_encrypted(&op("original"), original.digest());
    match store.sync {
        SyncEvidence::FileAndDirectorySynced => {
            let restored = restored.unwrap();
            // Held custody remains valid after the original journal owner closes.
            drop(store);
            assert_eq!(restored.ciphertext(), ciphertext);
            assert_eq!(restored.descriptor().private_event_json(), descriptor);
            assert_eq!(restored.receipt().digest(), original.digest());
            assert_eq!(restored.receipt().operation().0, "original");
            assert_eq!(restored.receipt().kind(), Kind::Encrypted);
            assert_eq!(restored.receipt().len(), data.len());
            assert!(restored.receipt().replayed());
            assert_eq!(restored.namespace_digest(), namespace().digest());
            assert_ne!(
                restored.namespace_digest(),
                HostNamespace::new("other").unwrap().digest()
            );
            assert!(restored.matches_namespace(&namespace()));
            assert!(!restored.matches_namespace(&HostNamespace::new("other").unwrap()));
            assert_eq!(
                codec()
                    .decrypt(restored.descriptor(), restored.ciphertext())
                    .unwrap()
                    .bytes(),
                data
            );
        }
        SyncEvidence::FileSyncedDirectoryUnconfirmed => {
            // Actual platform refusal, not a qualified restoration round trip.
            assert!(matches!(restored, Err(Error::Durability)));
            assert_eq!(store.read(&op("original")).unwrap().bytes(), ciphertext);
            drop(store);
        }
    }
    assert_eq!(fs::read(f.journal()).unwrap(), journal);
}

#[test]
fn native_media_restore_identity_refusal() {
    let f = Fixture::new();
    let mut store = f.create(Limits::default());
    let original = stage_encrypted(&f, &mut store, "original", b"first");
    let other = stage_encrypted(&f, &mut store, "other", b"second");
    let plain = store
        .stage(&op("plain"), f.plain(b"not encrypted custody"))
        .map_err(|failure| failure.error())
        .unwrap();
    let encrypted = codec().encrypt(f.snapshot(b"checked plaintext")).unwrap();
    let checked = codec()
        .decrypt(encrypted.descriptor(), encrypted.ciphertext())
        .unwrap();
    let checked = store
        .stage(&op("checked"), Media::Checked(checked))
        .map_err(|failure| failure.error())
        .unwrap();
    let before = journal_bytes(&mut store);
    assert!(matches!(
        store.restore_encrypted(&op("missing"), original.digest()),
        Err(Error::NotFound)
    ));
    for (id, digest) in [
        ("original", other.digest()),
        ("other", original.digest()),
        ("original", &[0; 32]),
        ("plain", plain.digest()),
        ("checked", checked.digest()),
    ] {
        assert!(matches!(
            store.restore_encrypted(&op(id), digest),
            Err(Error::Conflict)
        ));
    }
    assert_eq!(store.committed_records(), 4);
    drop(store);
    assert!(matches!(
        Store::open(
            f.directory(),
            HostNamespace::new("wrong partition").unwrap(),
            Limits::default()
        ),
        Err(Error::Identity)
    ));
    let mut reopened = f.reopen(Limits::default()).unwrap();
    assert_eq!(
        reopened.read(&op("original")).unwrap().receipt().digest(),
        original.digest()
    );
    assert_eq!(journal_bytes(&mut reopened), before);
}

#[test]
fn native_media_restore_capacity_and_durability() {
    let f = Fixture::new();
    let mut store = f.create(Limits::new(1024, 16384, 4, 1).unwrap());
    let receipt = stage_encrypted(&f, &mut store, "original", b"one held result");
    assert_sync(store.sync);
    let generic = store.read(&op("original")).unwrap();
    assert!(matches!(store.read(&op("original")), Err(Error::Capacity)));
    if store.sync == SyncEvidence::FileAndDirectorySynced {
        assert!(matches!(
            store.restore_encrypted(&op("original"), receipt.digest()),
            Err(Error::Capacity)
        ));
        drop(generic);
        let restored = store
            .restore_encrypted(&op("original"), receipt.digest())
            .unwrap();
        assert!(matches!(store.read(&op("original")), Err(Error::Capacity)));
        assert!(matches!(
            store.restore_encrypted(&op("original"), receipt.digest()),
            Err(Error::Capacity)
        ));
        drop(restored);
        assert_eq!(
            store.read(&op("original")).unwrap().receipt().digest(),
            receipt.digest()
        );
    } else {
        assert!(matches!(
            store.restore_encrypted(&op("original"), receipt.digest()),
            Err(Error::Durability)
        ));
        drop(generic);
        assert!(store.read(&op("original")).is_ok());
    }
    // Negative-only evidence injection; never synthesize successful OS sync.
    store.sync = SyncEvidence::FileSyncedDirectoryUnconfirmed;
    for _ in 0..3 {
        assert!(matches!(
            store.restore_encrypted(&op("original"), receipt.digest()),
            Err(Error::Durability)
        ));
    }
    assert!(store.read(&op("original")).is_ok());
}

#[test]
fn native_media_restore_corruption_and_pending() {
    for boundary in [
        Checkpoint::IntentSynced,
        Checkpoint::PayloadChunk,
        Checkpoint::CommitWritten,
        Checkpoint::Synced,
    ] {
        let f = Fixture::new();
        let mut store = f.create(Limits::default());
        let original = stage_encrypted(&f, &mut store, "original", b"committed prefix");
        let result = store.stage_inner(&op("later"), f.plain(&vec![1; 100000]), |phase| {
            if phase == boundary {
                Err(Error::OutcomeUnknown)
            } else {
                Ok(())
            }
        });
        assert!(matches!(
            result.map_err(|failure| failure.error()),
            Err(Error::OutcomeUnknown)
        ));
        assert!(matches!(
            store.restore_encrypted(&op("original"), original.digest()),
            Err(Error::OutcomeUnknown)
        ));
        let journal = journal_bytes(&mut store);
        drop(store);
        let mut reopened = f.reopen(Limits::default()).unwrap();
        let complete = matches!(boundary, Checkpoint::CommitWritten | Checkpoint::Synced);
        if complete {
            assert_eq!(reopened.recovery(), Recovery::Clean);
            let restored = reopened.restore_encrypted(&op("original"), original.digest());
            match reopened.sync {
                SyncEvidence::FileAndDirectorySynced => {
                    assert_eq!(restored.unwrap().receipt().digest(), original.digest());
                }
                SyncEvidence::FileSyncedDirectoryUnconfirmed => {
                    assert!(matches!(restored, Err(Error::Durability)));
                }
            }
        } else {
            assert_eq!(reopened.recovery(), Recovery::IncompleteTail);
            assert!(matches!(
                reopened.restore_encrypted(&op("original"), original.digest()),
                Err(Error::OutcomeUnknown)
            ));
        }
        assert_eq!(
            reopened.read(&op("original")).unwrap().receipt().digest(),
            original.digest()
        );
        assert_eq!(reopened.committed_records(), if complete { 2 } else { 1 });
        assert_eq!(journal_bytes(&mut reopened), journal);
    }
    let f = Fixture::new();
    let mut store = f.create(Limits::default());
    let original = stage_encrypted(&f, &mut store, "original", b"committed encrypted content");
    let offset = store.entries["original"].offset + frame::INTENT as u64;
    let mut journal = journal_bytes(&mut store);
    journal[offset as usize] ^= 1;
    store.file.seek(SeekFrom::Start(offset)).unwrap();
    store
        .file
        .write_all(&journal[offset as usize..offset as usize + 1])
        .unwrap();
    let expected = if store.sync == SyncEvidence::FileAndDirectorySynced {
        Error::Corrupt
    } else {
        Error::Durability
    };
    assert_eq!(
        store
            .restore_encrypted(&op("original"), original.digest())
            .err(),
        Some(expected)
    );
    assert!(matches!(store.read(&op("original")), Err(Error::Corrupt)));
    assert!(matches!(
        store.restore_encrypted(&op("original"), original.digest()),
        Err(Error::OutcomeUnknown)
    ));
    drop(store);
    assert!(matches!(f.reopen(Limits::default()), Err(Error::Corrupt)));
    assert_eq!(fs::read(f.journal()).unwrap(), journal);
}
