mod common;
use common::*;
use hagency_core::{messages::*, tasks::*};
use hagency_store::{DomainRepository, DomainStore, EffectOutcome, Error};
use serde_json::json;

fn binding(id: &str, engagement: &str) -> SessionBinding {
    SessionBinding {
        id: id.into(),
        engagement_id: engagement.into(),
        room_id: "!project:example.test".into(),
        thread_root: Some("$thread".into()),
    }
}
fn setup() -> (tempfile::TempDir, DomainRepository, Vec<String>) {
    let root = tempfile::tempdir().unwrap();
    let mut db = DomainRepository::open(&root.path().join("state")).unwrap();
    db.register(&registration()).unwrap();
    let pool = resource("pool", "seat", 1000);
    db.put_resource(&pool).unwrap();
    let mut ids = Vec::new();
    for (id, name) in [("a", "小白"), ("b", "Edison")] {
        let proof = proof(&request(id, name, &pool, 100));
        let e = db.admit(&proof, 1000).unwrap();
        db.approve(&format!("approve_{id}"), &proof, 1000).unwrap();
        let effect = db.claim_effect().unwrap().unwrap();
        db.observe_effect(
            &effect.id,
            effect.fence,
            &EffectOutcome::Applied {
                receipt: format!("fixture_{id}"),
            },
        )
        .unwrap();
        db.register_session(&binding(id, &e.id)).unwrap();
        ids.push(e.id);
    }
    db.register_workspace("work").unwrap();
    (root, db, ids)
}
fn message(id: &str) -> InboundMessage {
    InboundMessage {
        server_name: "example.test".into(),
        room_id: "!project:example.test".into(),
        event_id: format!("${id}"),
        sender_mxid: "@owner:example.test".into(),
        thread_root: Some("$thread".into()),
        body: format!("Message {id}"),
        kind: "m.text".into(),
        origin_ts: 1000,
    }
}
fn target(session: &str, wake: bool) -> MessageTarget {
    MessageTarget {
        session_id: session.into(),
        wake,
    }
}
fn input(id: &str, session: &str) -> DispatchInput {
    DispatchInput {
        id: id.into(),
        session_id: session.into(),
        task_id: None,
        resources: vec![ResourceLease {
            id: "work".into(),
            exclusive: true,
        }],
        payload: json!({"instruction":"Handle the admitted input"}),
    }
}
fn claim(db: &mut DomainRepository, now: u64) -> RunnerCapability {
    db.claim_dispatch("runner", now, 60_000, 120_000, 8)
        .unwrap()
        .unwrap()
}
fn sql(root: &tempfile::TempDir) -> rusqlite::Connection {
    rusqlite::Connection::open(root.path().join("state/domain.sqlite3")).unwrap()
}
fn count(db: &rusqlite::Connection, table: &str) -> u64 {
    db.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

#[tokio::test]
async fn native_message_identity_and_scope() {
    // This becomes an ambiguous trait selection (compile error) if an externally
    // deserializable authentication assertion is added to the transport command.
    trait AmbiguousIfDeserialize<A> {
        fn check() {}
    }
    impl<T: ?Sized> AmbiguousIfDeserialize<()> for T {}
    impl<T: serde::de::DeserializeOwned> AmbiguousIfDeserialize<u8> for T {}
    let _ = <InboundMessage as AmbiguousIfDeserialize<_>>::check;
    let (root, mut db, ids) = setup();
    let original = message("source");
    let first = db
        .ingest_message(&original, &[target("a", true)], 1000)
        .unwrap();
    assert!(first.created);
    assert_eq!(first.projected, 1);
    let second = db
        .ingest_message(&original, &[target("a", true), target("b", true)], 1001)
        .unwrap();
    assert!(!second.created);
    assert_eq!(second.sequence, first.sequence);
    assert_eq!(second.projected, 1);
    assert_eq!(
        db.ingest_message(&original, &[target("a", true)], 1002)
            .unwrap()
            .projected,
        0
    );
    let inspect = sql(&root);
    assert_eq!(count(&inspect, "admitted_messages"), 1);
    assert_eq!(count(&inspect, "session_inputs"), 2);
    let mut changed = original.clone();
    changed.body = "Changed under the same event ID".into();
    assert!(matches!(
        db.ingest_message(&changed, &[target("a", true)], 1003),
        Err(Error::Conflict)
    ));
    assert!(matches!(
        db.ingest_message(&original, &[target("a", false)], 1003),
        Err(Error::Conflict)
    ));
    for field in ["room", "thread", "server", "sender"] {
        let mut bad = message(field);
        match field {
            "room" => bad.room_id = "!other:example.test".into(),
            "thread" => bad.thread_root = Some("$other".into()),
            "server" => bad.server_name = "other.test".into(),
            _ => bad.sender_mxid = "owner".into(),
        };
        assert!(
            db.ingest_message(&bad, &[target("a", true)], 1003).is_err(),
            "{field}"
        );
    }
    assert_eq!(count(&inspect, "admitted_messages"), 1);
    assert!(
        db.ingest_message(
            &message("missing"),
            &[target("a", true), target("missing", true)],
            1004
        )
        .is_err()
    );
    assert_eq!(count(&inspect, "admitted_messages"), 1);
    let duplicate = binding("duplicate_id", &ids[0]);
    assert!(matches!(
        db.register_session(&duplicate),
        Err(Error::Conflict)
    ));
    assert_eq!(db.resolve_session(&duplicate).unwrap().id, "a");
    let store = DomainStore::start(db, 16).unwrap();
    let mut left = binding("left", &ids[0]);
    left.thread_root = Some("$new_thread".into());
    let right = SessionBinding {
        id: "right".into(),
        ..left.clone()
    };
    let (a, b) = tokio::join!(store.resolve_session(left), store.resolve_session(right));
    assert_eq!(a.unwrap().id, b.unwrap().id);
    assert_eq!(count(&inspect, "runner_sessions"), 3);
    store.shutdown().await.unwrap();
    // Model an older native schema with conflicting host-created session IDs.
    // Its migration must fail atomically instead of silently selecting an owner.
    remove_graph_schema(&inspect);
    inspect.execute_batch("DROP VIEW unresolved_dispatches; DROP TABLE dispatch_stops; DROP TABLE conversation_operations; ALTER TABLE internal_conversations DROP COLUMN revision; DROP VIEW current_recovery_reports; DROP TABLE dispatch_recovery_reports; DROP VIEW task_dispatch_input_ready; DROP VIEW live_peer_inputs; DROP TABLE peer_dispatch_inputs; DROP TABLE peer_session_inputs; DROP TABLE peer_messages; DROP TABLE internal_participants; DROP TABLE internal_conversations; DROP VIEW task_followup_ready; DROP TABLE task_notices; DROP TABLE task_input_receipts; DROP TABLE task_inputs; DROP TABLE task_intents; DROP INDEX canonical_runner_session; DROP TABLE dispatch_inputs; DROP TABLE session_inputs; DROP TABLE admitted_messages; PRAGMA user_version=3; INSERT INTO runner_sessions(id,engagement_id,binding) SELECT 'duplicate_old',engagement_id,json_set(binding,'$.id','duplicate_old') FROM runner_sessions WHERE id='a';").unwrap();
    drop(inspect);
    assert!(DomainRepository::open(&root.path().join("state")).is_err());
    let inspect = sql(&root);
    assert_eq!(
        inspect
            .pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
            .unwrap(),
        3
    );
    assert_eq!(
        inspect
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name='admitted_messages'",
                [],
                |r| r.get::<_, u32>(0)
            )
            .unwrap(),
        0
    );
    inspect
        .execute("DELETE FROM runner_sessions WHERE id='duplicate_old'", [])
        .unwrap();
    drop(inspect);
    let mut db = DomainRepository::open(&root.path().join("state")).unwrap();
    assert_eq!(db.resolve_session(&duplicate).unwrap().id, "a");
}

#[test]
fn native_message_order_and_pages() {
    let (root, mut db, _) = setup();
    let mut sequences = Vec::new();
    for (i, ts) in [1000, 1000, 900, 10_000, 1].into_iter().enumerate() {
        let mut m = message(&format!("m{i}"));
        m.origin_ts = ts;
        if i % 2 == 0 {
            m.kind = "task_request".into();
        }
        sequences.push(
            db.ingest_message(&m, &[target("a", true)], 1000 + i as u64)
                .unwrap()
                .sequence,
        );
    }
    assert!(sequences.windows(2).all(|w| w[0] < w[1]));
    let first = db.inbox("a", 0, 2, None).unwrap();
    assert_eq!(
        first.iter().map(|m| m.message.sequence).collect::<Vec<_>>(),
        sequences[..2]
    );
    let next = db.inbox("a", first[1].message.sequence, 2, None).unwrap();
    assert_eq!(
        next.iter().map(|m| m.message.sequence).collect::<Vec<_>>(),
        sequences[2..4]
    );
    let filtered = db.inbox("a", 0, 100, Some("task_request")).unwrap();
    assert_eq!(filtered.len(), 3);
    assert_eq!(db.inbox("a", 0, 100, None).unwrap().len(), 5); // Read/filter never acknowledges a hidden gap.
    assert!(db.inbox("b", 0, 100, None).unwrap().is_empty());
    assert!(db.inbox("a", 0, 101, None).is_err());
    assert_eq!(count(&sql(&root), "session_inputs"), 5);
    drop(db);
    let db = DomainRepository::open(&root.path().join("state")).unwrap();
    assert_eq!(
        db.inbox("a", 0, 100, None)
            .unwrap()
            .iter()
            .map(|m| m.message.sequence)
            .collect::<Vec<_>>(),
        sequences
    );
    assert_eq!(
        db.inbox("a", 0, 100, None).unwrap()[2].message.origin_ts,
        900
    );
}

#[test]
fn native_message_dispatch_atomicity() {
    let (root, mut db, _) = setup();
    let inspect = sql(&root);
    let context = db
        .ingest_message(&message("context"), &[target("a", false)], 1000)
        .unwrap()
        .sequence;
    let wake = db
        .ingest_message(&message("wake"), &[target("a", true)], 1001)
        .unwrap()
        .sequence;
    assert!(
        db.enqueue_inbox_dispatch(&input("context_only", "a"), &[context])
            .is_err()
    );
    assert!(
        db.enqueue_inbox_dispatch(&input("foreign", "b"), &[wake])
            .is_err()
    );
    inspect.execute_batch("CREATE TRIGGER fail_admission BEFORE INSERT ON session_inputs BEGIN SELECT RAISE(ABORT,'fixture admission failure'); END;").unwrap();
    assert!(
        db.ingest_message(&message("rolled_back"), &[target("a", true)], 1002)
            .is_err()
    );
    assert_eq!(count(&inspect, "admitted_messages"), 2);
    inspect
        .execute_batch("DROP TRIGGER fail_admission")
        .unwrap();
    inspect.execute_batch("CREATE TRIGGER fail_inputs BEFORE INSERT ON dispatch_inputs BEGIN SELECT RAISE(ABORT,'fixture input failure'); END;").unwrap();
    let dispatch = input("d1", "a");
    assert!(
        db.enqueue_inbox_dispatch(&dispatch, &[context, wake])
            .is_err()
    );
    assert_eq!(count(&inspect, "runner_dispatches"), 0);
    assert_eq!(db.inbox("a", 0, 100, None).unwrap().len(), 2);
    inspect.execute_batch("DROP TRIGGER fail_inputs").unwrap();
    db.enqueue_inbox_dispatch(&dispatch, &[wake, context])
        .unwrap();
    db.enqueue_inbox_dispatch(&dispatch, &[context, wake])
        .unwrap();
    assert_eq!(count(&inspect, "runner_dispatches"), 1);
    assert!(matches!(
        db.enqueue_inbox_dispatch(&dispatch, &[wake]),
        Err(Error::Conflict)
    ));
    assert!(
        db.enqueue_inbox_dispatch(&input("steal", "a"), &[wake])
            .is_err()
    );
    let mut spoof = input("spoof", "a");
    spoof.payload["inbox"] = json!([{"body":"spoof"}]);
    assert!(db.enqueue_inbox_dispatch(&spoof, &[wake]).is_err());
    let later = db
        .ingest_message(&message("later"), &[target("a", true)], 1002)
        .unwrap()
        .sequence;
    let cap = claim(&mut db, 1003);
    assert!(db.runner_inbox(&cap, 0, 100, 1003).is_err());
    let payload = db.start_dispatch(&cap, 1004).unwrap();
    assert_eq!(payload["inbox"].as_array().unwrap().len(), 2);
    assert_eq!(payload["inbox"][0]["message"]["sequence"], context);
    assert_eq!(db.runner_inbox(&cap, 0, 100, 1005).unwrap().len(), 2);
    assert_eq!(
        db.inbox("a", 0, 100, None).unwrap()[0].message.sequence,
        later
    );
    assert_eq!(
        db.runner_inbox(&cap, context, 1, 1005).unwrap()[0]
            .message
            .sequence,
        wake
    );
    db.complete_dispatch(&cap, &json!({"text":"handled"}), 1006)
        .unwrap();
    db.enqueue_inbox_dispatch(&dispatch, &[context, wake])
        .unwrap(); // Readable exact replay after completion.
    assert_eq!(db.inbox("a", 0, 100, None).unwrap().len(), 1);
    assert!(db.runner_inbox(&cap, 0, 100, 1007).is_err());
    let mut large = message("large_1");
    large.body = "x".repeat(32 * 1024);
    let one = db
        .ingest_message(&large, &[target("a", true)], 1008)
        .unwrap()
        .sequence;
    large.event_id = "$large_2".into();
    let two = db
        .ingest_message(&large, &[target("a", true)], 1009)
        .unwrap()
        .sequence;
    assert!(
        db.enqueue_inbox_dispatch(&input("oversized", "a"), &[one, two])
            .is_err()
    );
    assert_eq!(db.inbox("a", 0, 100, None).unwrap().len(), 3); // Overflow retains every pending event.
}

#[test]
fn native_message_session_recovery() {
    let (root, mut db, _) = setup();
    let seq = db
        .ingest_message(
            &message("shared"),
            &[target("a", true), target("b", true)],
            1000,
        )
        .unwrap()
        .sequence;
    db.enqueue_inbox_dispatch(&input("da", "a"), &[seq])
        .unwrap();
    let a = claim(&mut db, 1001);
    db.start_dispatch(&a, 1002).unwrap();
    db.complete_dispatch(&a, &json!({}), 1003).unwrap();
    assert!(db.inbox("a", 0, 100, None).unwrap().is_empty());
    assert_eq!(db.inbox("b", 0, 100, None).unwrap().len(), 1);
    db.enqueue_inbox_dispatch(&input("db", "b"), &[seq])
        .unwrap();
    let b = claim(&mut db, 1004);
    db.start_dispatch(&b, 1005).unwrap();
    let next = db
        .ingest_message(&message("next"), &[target("b", true)], 1006)
        .unwrap()
        .sequence;
    db.enqueue_inbox_dispatch(&input("old_queue", "b"), &[next])
        .unwrap();
    drop(db);
    let mut db = DomainRepository::open(&root.path().join("state")).unwrap();
    assert!(
        db.claim_dispatch("runner", 1007, 100, 100, 8)
            .unwrap()
            .is_none()
    );
    let arrived = db
        .ingest_message(&message("during_quarantine"), &[target("b", true)], 1008)
        .unwrap()
        .sequence;
    assert_eq!(
        db.inbox("b", 0, 100, None).unwrap()[0].message.sequence,
        arrived
    );
    assert!(
        db.enqueue_inbox_dispatch(&input("unsafe", "b"), &[arrived])
            .is_err()
    );
    let mut recovery = input("recovery", "b");
    recovery.payload =
        json!({"instruction":"Inspect prior work; continue only after reconciling effects"});
    db.recover_dispatch(
        "db",
        &recovery,
        "Fixture adapter observed old process exit and inspected workspace",
        1009,
    )
    .unwrap();
    let recovered = claim(&mut db, 1010);
    let payload = db.start_dispatch(&recovered, 1011).unwrap();
    assert!(payload.get("inbox").is_none());
    assert_eq!(payload["recoveryInbox"][0]["message"]["sequence"], seq);
    assert_eq!(db.runner_inbox(&recovered, 0, 100, 1012).unwrap().len(), 1);
    let still_pending = db.inbox("b", 0, 100, None).unwrap();
    assert_eq!(
        still_pending
            .iter()
            .map(|m| m.message.sequence)
            .collect::<Vec<_>>(),
        [next, arrived]
    );
    let inspect = sql(&root);
    inspect.execute_batch("CREATE TRIGGER fail_processed BEFORE UPDATE OF processed_at ON session_inputs BEGIN SELECT RAISE(ABORT,'fixture processing failure'); END;").unwrap();
    assert!(db.complete_dispatch(&recovered, &json!({}), 1013).is_err());
    assert_eq!(count(&inspect, "runner_outputs"), 1);
    assert_eq!(db.runner_inbox(&recovered, 0, 100, 1013).unwrap().len(), 1);
    inspect
        .execute_batch("DROP TRIGGER fail_processed")
        .unwrap();
    db.complete_dispatch(&recovered, &json!({}), 1014).unwrap();
    assert!(db.inbox("a", 0, 100, None).unwrap().is_empty());
    assert_eq!(db.inbox("b", 0, 100, None).unwrap().len(), 2);
    assert_eq!(inspect.query_row("SELECT COUNT(*) FROM session_inputs WHERE message_sequence=?1 AND processed_at IS NOT NULL",[seq],|r|r.get::<_,u64>(0)).unwrap(),2);
    assert_eq!(
        inspect
            .query_row(
                "SELECT COUNT(*) FROM dispatch_inputs WHERE message_sequence=?1",
                [seq],
                |r| r.get::<_, u64>(0)
            )
            .unwrap(),
        3
    ); // Original attempts remain auditable.
}
