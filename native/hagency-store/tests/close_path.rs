mod common;
use common::*;
use hagency_store::DomainRepository;

/// The close path is a bounded resource release (ADR-120): a completed close
/// performs no checkpoint and unlinks neither auxiliary file, and the next
/// open replays the WAL to the same canonical state.
#[test]
fn native_store_close_leaves_wal_for_replay() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    let mut db = DomainRepository::open(&state).unwrap();
    db.register(&registration()).unwrap();
    db.put_resource(&resource("preset", "seat", 0)).unwrap();
    assert_eq!(db.catalog("", 100).unwrap().len(), 1);
    drop(db);
    let wal = std::fs::metadata(state.join("domain.sqlite3-wal"))
        .expect("a completed close must leave the WAL in place");
    assert!(
        wal.len() > 32,
        "the WAL holds no committed frames after close: {} bytes",
        wal.len()
    );
    assert!(
        state.join("domain.sqlite3-shm").exists(),
        "a completed close must leave the SHM in place"
    );
    let db = DomainRepository::open(&state).unwrap();
    assert_eq!(db.catalog("", 100).unwrap().len(), 1);
}
