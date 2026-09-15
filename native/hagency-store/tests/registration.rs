//! G11 registration slice (spec `task-rust-project-side-registration`): the
//! `DomainRepository::register` facade is the sole writer of `registrations`;
//! these prove the store's own contract over the facade — shape validation, the
//! identical-content no-op, the stale-generation refusal, the rotate-and-
//! reconcile advance — unchanged by either production caller. The caller
//! surfaces themselves are proven in hagency/tests (console route) and the CLI
//! (bootstrap registration command).
mod common;
use common::*;
use hagency_store::{DomainRepository, DomainStore, Error};

fn open_raw(dir: &tempfile::TempDir) -> rusqlite::Connection {
    rusqlite::Connection::open(dir.path().join("state/domain.sqlite3")).unwrap()
}

fn rows(dir: &tempfile::TempDir) -> Vec<(String, u64)> {
    let db = open_raw(dir);
    let mut stmt = db
        .prepare("SELECT fleet_id,generation FROM registrations ORDER BY rowid")
        .unwrap();
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

/// Scenario: a re-registration with identical content is a no-op, and a stale
/// generation is refused with the stored row unchanged.
#[tokio::test]
async fn native_registration_facade_idempotent_and_stale_refused() {
    let dir = tempfile::tempdir().unwrap();
    let db = DomainRepository::open(&dir.path().join("state")).unwrap();
    let store = DomainStore::start(db, 16).unwrap();
    let reg = registration();
    store.register(reg.clone()).await.unwrap();
    assert_eq!(rows(&dir).len(), 1);
    // Identical content: succeeds, the row is unchanged, no second row.
    store.register(reg.clone()).await.unwrap();
    assert_eq!(rows(&dir), vec![(reg.fleet_id.clone(), 1)]);
    // A stale (non-advancing) generation is refused; the row still says 1.
    let mut stale = registration();
    stale.reception_room_id = "!other:example.test".into();
    let result = store.register(stale).await;
    assert!(matches!(result, Err(Error::Generation)));
    assert_eq!(rows(&dir), vec![(reg.fleet_id.clone(), 1)]);
    store.shutdown().await.unwrap();
}

/// Scenario: a generation advance rotates the registration and reconciles in the
/// same transaction; the previous row is replaced, not duplicated.
#[tokio::test]
async fn native_registration_facade_generation_advance_reconciles() {
    let dir = tempfile::tempdir().unwrap();
    let db = DomainRepository::open(&dir.path().join("state")).unwrap();
    let store = DomainStore::start(db, 16).unwrap();
    let reg = registration();
    store.register(reg.clone()).await.unwrap();
    let mut rotated = registration();
    rotated.generation = 2;
    store.register(rotated.clone()).await.unwrap();
    let rows = rows(&dir);
    assert_eq!(rows, vec![(reg.fleet_id.clone(), 2)]);
    // The stored config carries the new generation (the read joins on it).
    let raw = open_raw(&dir);
    let config: String = raw
        .query_row(
            "SELECT config FROM registrations WHERE fleet_id=?1",
            [&reg.fleet_id],
            |r| r.get(0),
        )
        .unwrap();
    assert!(config.contains("\"generation\":2"));
    store.shutdown().await.unwrap();
}

/// Scenario: an invalid registration is refused before any write.
#[tokio::test]
async fn native_registration_facade_refuses_an_invalid_record() {
    let dir = tempfile::tempdir().unwrap();
    let db = DomainRepository::open(&dir.path().join("state")).unwrap();
    let store = DomainStore::start(db, 16).unwrap();
    // Malformed fleet id.
    let mut bad = registration();
    bad.fleet_id = "not-a-fleet".into();
    assert!(matches!(store.register(bad).await, Err(Error::Invalid(_))));
    // Generation zero.
    let mut bad = registration();
    bad.generation = 0;
    assert!(matches!(store.register(bad).await, Err(Error::Invalid(_))));
    // A server name the mxids and room do not share.
    let mut bad = registration();
    bad.server_name = "elsewhere.test".into();
    assert!(matches!(store.register(bad).await, Err(Error::Invalid(_))));
    // Two identical operator mxids.
    let mut bad = registration();
    bad.approval_bot_mxid = bad.representative_mxid.clone();
    assert!(matches!(store.register(bad).await, Err(Error::Invalid(_))));
    assert!(rows(&dir).is_empty(), "no row may be written");
    store.shutdown().await.unwrap();
}
