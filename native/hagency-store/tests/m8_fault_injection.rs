//! M8 item-4 disk-full injection: the store fails closed under an engine-level
//! page limit. This is the one slice-owned scenario; the two outbound fault
//! tests live in the matrix crate (glm7).
//!
//! The injection uses SQLite's `max_page_count` pragma on the store's own
//! writer connection (the licensed `Repository::open_with_page_limit` seam),
//! never a real filesystem fill. The pragma is connection-local, so the cap is
//! applied to exactly the connection that will attempt the write.

use hagency_core::custody::{Delivery, Kind, Lane};
use hagency_store::{Error, Repository};
use serde_json::json;

fn delivery(id: &str, payload: serde_json::Value) -> Delivery {
    Delivery {
        binding: "fault-fixture".into(),
        generation: 1,
        id: id.into(),
        lane: Lane::Matrix,
        kind: Kind::Transaction,
        payload,
    }
}

#[test]
fn native_store_fails_closed_on_disk_full() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");

    // Establish the schema and one baseline row so the page count is stable,
    // then read the live page count from a raw connection (read-only pragma).
    {
        let mut repo = Repository::open(&state).unwrap();
        repo.receive(&delivery("a", json!({"seed": 1})), 1).unwrap();
    }
    let page_count: i64 = {
        let conn = rusqlite::Connection::open(state.join("custody.sqlite3")).unwrap();
        conn.pragma_query_value(None, "page_count", |r| r.get(0))
            .unwrap()
    };
    assert!(page_count > 0);

    // Inject disk-full at the exact current size, then attempt a write whose
    // payload must allocate new pages — past the cap.
    let err = {
        let mut repo = Repository::open_with_page_limit(&state, page_count as u64).unwrap();
        // A payload far larger than any remaining in-page free space must grow
        // the database file, which the cap refuses.
        let big = json!({"data": "x".repeat(128 * 1024)});
        repo.receive(&delivery("b", big), 2).unwrap_err()
    };

    // The store yields its own engine failure, never a swallowed success.
    let message = match &err {
        Error::Sqlite(e) => e.to_string(),
        other => panic!("expected Error::Sqlite, got {other:?}"),
    };
    assert!(
        message.contains("full") || message.contains("disk"),
        "engine message must name full/disk, got: {message}"
    );

    // No partial row survives, and the store stays readable and writable within
    // the limit afterwards (the cap is connection-local, so a fresh open both
    // proves readability and demonstrates the limit is gone).
    let mut reopened = Repository::open(&state).unwrap();
    let count: i64 = {
        let conn = rusqlite::Connection::open(state.join("custody.sqlite3")).unwrap();
        conn.query_row("SELECT COUNT(*) FROM inbox", [], |r| r.get(0))
            .unwrap()
    };
    assert_eq!(count, 1, "no partial row for the refused write survives");
    // The store is still writable within the limit: a fresh (uncapped) open
    // accepts a normal-sized delivery.
    reopened
        .receive(&delivery("c", json!({"seed": 3})), 3)
        .unwrap();
}
