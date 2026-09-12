mod common;
#[path = "conversation_lifecycle/mod.rs"]
mod lifecycle;
#[path = "workflows/mod.rs"]
mod workflows;
use common::*;
use hagency_core::{conversations::*, messages::*, tasks::*};
use hagency_store::{DomainRepository, EffectOutcome, Error};
use serde_json::json;

fn dispatch(id: &str, session: &str, task: Option<&str>) -> DispatchInput {
    DispatchInput {
        id: id.into(),
        session_id: session.into(),
        task_id: task.map(str::to_owned),
        resources: vec![],
        payload: json!({"instruction":"Verify internal task ownership"}),
    }
}
fn claim(db: &mut DomainRepository, now: u64) -> RunnerCapability {
    db.claim_dispatch("runner", now, 60_000, 120_000, 8)
        .unwrap()
        .unwrap()
}
fn setup() -> (
    tempfile::TempDir,
    DomainRepository,
    Vec<String>,
    RunnerCapability,
) {
    setup_with_parent(false)
}
fn setup_with_parent(
    parent: bool,
) -> (
    tempfile::TempDir,
    DomainRepository,
    Vec<String>,
    RunnerCapability,
) {
    let root = tempfile::tempdir().unwrap();
    let mut db = DomainRepository::open(&root.path().join("state")).unwrap();
    db.register(&registration()).unwrap();
    let pool = resource("pool", "seat", 1000);
    db.put_resource(&pool).unwrap();
    let mut agents = Vec::new();
    for (id, name) in [("a", "小白"), ("b", "Edison"), ("c", "Other")] {
        let mut req = request(id, name, &pool, 100);
        if id == "c" {
            req.target_project_id = "other_project".into();
            req.target_room_id = "!other:example.test".into();
        }
        let p = proof(&req);
        let e = db.admit(&p, 1000).unwrap();
        db.approve(&format!("approve_{id}"), &p, 1000).unwrap();
        let effect = db.claim_effect().unwrap().unwrap();
        db.observe_effect(
            &effect.id,
            effect.fence,
            &EffectOutcome::Applied {
                receipt: format!("fixture_{id}"),
            },
        )
        .unwrap();
        db.register_session(&SessionBinding {
            id: id.into(),
            engagement_id: e.id.clone(),
            room_id: req.target_room_id,
            thread_root: None,
        })
        .unwrap();
        agents.push(e.id);
    }
    if parent {
        db.create_canonical_task("parent", "a", "Coordinate verified work", 1000)
            .unwrap();
    }
    db.enqueue_dispatch(&dispatch("creator", "a", parent.then_some("parent")))
        .unwrap();
    let cap = claim(&mut db, 1001);
    db.start_dispatch(&cap, 1002).unwrap();
    (root, db, agents, cap)
}
fn request_group(key: &str, target: &str) -> ConversationRequest {
    ConversationRequest {
        call_id: key.into(),
        label: "协作任务".into(),
        participant_engagements: vec![target.into()],
    }
}
fn participant<'a>(group: &'a Conversation, agent: &str) -> &'a str {
    &group
        .participants
        .iter()
        .find(|p| p.engagement_id == agent)
        .unwrap()
        .id
}
fn sql(root: &tempfile::TempDir) -> rusqlite::Connection {
    rusqlite::Connection::open(root.path().join("state/domain.sqlite3")).unwrap()
}
fn count(db: &rusqlite::Connection, table: &str) -> u64 {
    db.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

#[test]
fn native_internal_conversation_atomicity() {
    let (root, mut db, agents, cap) = setup();
    let inspect = sql(&root);
    let req = request_group("group", &agents[1]);
    inspect.execute_batch("CREATE TRIGGER fail_participant BEFORE INSERT ON internal_participants BEGIN SELECT RAISE(ABORT,'injected participant failure'); END;").unwrap();
    assert!(db.create_internal_conversation(&cap, &req, 1003).is_err());
    assert_eq!(count(&inspect, "internal_conversations"), 0);
    assert_eq!(count(&inspect, "internal_participants"), 0);
    assert_eq!(count(&inspect, "runner_sessions"), 3);
    inspect
        .execute_batch("DROP TRIGGER fail_participant")
        .unwrap();
    let created = db.create_internal_conversation(&cap, &req, 1004).unwrap();
    assert!(!created.replayed);
    assert_eq!(created.conversation.creator_session_id, "a");
    assert_eq!(created.conversation.participants.len(), 2);
    let replay = db.create_internal_conversation(&cap, &req, 1005).unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.conversation.id, created.conversation.id);
    let mut changed = req.clone();
    changed.label = "other".into();
    assert!(matches!(
        db.create_internal_conversation(&cap, &changed, 1005),
        Err(Error::Conflict)
    ));
    assert_eq!(count(&inspect, "internal_conversations"), 1);
    assert_eq!(count(&inspect, "internal_participants"), 2);
    assert_eq!(count(&inspect, "runner_sessions"), 5);
    let mut forged = value(&req);
    forged["creator_session_id"] = json!("operator");
    assert!(serde_json::from_value::<ConversationRequest>(forged).is_err());
}

#[test]
fn native_internal_conversation_authority() {
    let (_root, mut db, agents, cap) = setup();
    let req = request_group("group", &agents[1]);
    assert!(
        db.create_internal_conversation(&cap, &request_group("foreign", &agents[2]), 1003)
            .is_err()
    );
    let group = db
        .create_internal_conversation(&cap, &req, 1003)
        .unwrap()
        .conversation;
    assert!(
        db.create_internal_conversation(&cap, &req, 200_000)
            .is_err()
    );
    db.park_dispatch(&cap, true, 1004).unwrap();
    assert!(db.create_internal_conversation(&cap, &req, 1005).is_err());
    assert!(db.runner_conversation(&cap, &group.id, 1005).is_err());
    db.park_dispatch(&cap, false, 1006).unwrap();
    db.register_session(&SessionBinding {
        id: "other_session".into(),
        engagement_id: agents[0].clone(),
        room_id: "!project:example.test".into(),
        thread_root: Some("$different".into()),
    })
    .unwrap();
    db.enqueue_dispatch(&dispatch("different", "other_session", None))
        .unwrap();
    let other = claim(&mut db, 1007);
    db.start_dispatch(&other, 1008).unwrap();
    assert!(db.runner_conversation(&other, &group.id, 1009).is_err()); // Same Agent, wrong caller session.
    db.enqueue_dispatch(&dispatch(
        "participant",
        participant(&group, &agents[1]),
        None,
    ))
    .unwrap();
    let member = claim(&mut db, 1010);
    db.start_dispatch(&member, 1011).unwrap();
    assert_eq!(
        db.runner_conversation(&member, &group.id, 1012).unwrap().id,
        group.id
    );
    let another = db
        .create_internal_conversation(&cap, &request_group("second", &agents[1]), 1013)
        .unwrap()
        .conversation;
    assert!(db.runner_conversation(&member, &another.id, 1014).is_err());
    db.revoke("revoke_b", &agents[1]).unwrap();
    assert!(db.runner_conversation(&member, &group.id, 1015).is_err());
    assert!(db.create_internal_conversation(&cap, &req, 1015).is_err());
}

#[test]
fn native_internal_task_isolation() {
    let (root, mut db, agents, cap) = setup();
    let one = db
        .create_internal_conversation(&cap, &request_group("one", &agents[1]), 1003)
        .unwrap()
        .conversation;
    let two = db
        .create_internal_conversation(&cap, &request_group("two", &agents[1]), 1004)
        .unwrap()
        .conversation;
    let first = participant(&one, &agents[1]);
    let second = participant(&two, &agents[1]);
    assert_ne!(first, second);
    db.create_coordinator_task(&cap, "task_one", first, "First task", 1005)
        .unwrap();
    db.create_coordinator_task(&cap, "task_two", second, "Second task", 1005)
        .unwrap();
    db.enqueue_dispatch(&dispatch("first", first, Some("task_one")))
        .unwrap();
    db.enqueue_dispatch(&dispatch("second", second, Some("task_two")))
        .unwrap();
    let a = claim(&mut db, 1006);
    let b = claim(&mut db, 1006);
    assert_ne!(a.dispatch_id, b.dispatch_id);
    db.start_dispatch(&a, 1007).unwrap();
    db.start_dispatch(&b, 1007).unwrap();
    assert!(db.runner_task(&a, "task_two", 1008).is_err());
    assert!(db.runner_conversation(&a, &two.id, 1008).is_err());
    db.mutate_task(
        &a,
        "task_one",
        "done",
        &TaskMutation::Transition {
            status: TaskState::Done,
            waiting_reason: None,
            waiting_until: None,
        },
        1009,
    )
    .unwrap();
    db.complete_dispatch(&a, &json!({"result":"done"}), 1010)
        .unwrap();
    assert_eq!(
        db.canonical_task("task_two").unwrap().status,
        TaskState::InProgress
    );
    assert!(db.inbox(first, 0, 100, None).is_err()); // Matrix inbox cannot be reused as internal mailbox.
    drop(db);
    let mut db = DomainRepository::open(&root.path().join("state")).unwrap();
    assert!(db.runner_task(&b, "task_two", 1011).is_err());
    let inspect = sql(&root);
    assert!(
        !inspect
            .query_row(
                "SELECT quarantined FROM runner_sessions WHERE id=?1",
                [first],
                |r| r.get::<_, bool>(0)
            )
            .unwrap()
    );
    assert!(
        inspect
            .query_row(
                "SELECT quarantined FROM runner_sessions WHERE id=?1",
                [second],
                |r| r.get::<_, bool>(0)
            )
            .unwrap()
    );
    let mut recovery = dispatch("recovery", second, Some("task_two"));
    assert!(
        db.recover_dispatch("second", &recovery, "Fixture inspection", 1012)
            .is_err()
    );
    recovery.payload =
        json!({"instruction":"Inspect partial internal results before continuing the task"});
    db.recover_dispatch(
        "second",
        &recovery,
        "Fixture adapter inspected the stopped runner; no process was launched",
        1012,
    )
    .unwrap();
    let recovered = claim(&mut db, 1013);
    db.start_dispatch(&recovered, 1014).unwrap();
    assert_eq!(
        db.runner_conversation(&recovered, &two.id, 1015)
            .unwrap()
            .id,
        two.id
    );
    assert_eq!(
        db.canonical_task("task_one").unwrap().status,
        TaskState::Done
    );
}

#[test]
fn native_internal_matrix_separation() {
    let (root, mut db, agents, cap) = setup();
    let group = db
        .create_internal_conversation(&cap, &request_group("group", &agents[1]), 1003)
        .unwrap()
        .conversation;
    let internal = &group.participants[0];
    let encoded = value(internal);
    assert!(encoded.get("room_id").is_none());
    assert!(serde_json::from_value::<SessionBinding>(encoded.clone()).is_err());
    let decoded: StoredSession = serde_json::from_value(encoded.clone()).unwrap();
    assert!(decoded.matrix().is_none());
    let mut mixed = encoded.clone();
    mixed["room_id"] = json!("!project:example.test");
    assert!(serde_json::from_value::<StoredSession>(mixed).is_err());
    let mut unknown = encoded;
    unknown["kind"] = json!("unknown");
    assert!(serde_json::from_value::<StoredSession>(unknown).is_err());
    let message = InboundMessage {
        server_name: "example.test".into(),
        room_id: "!project:example.test".into(),
        event_id: "$incoming".into(),
        sender_mxid: "@owner:example.test".into(),
        thread_root: None,
        body: "Room input".into(),
        kind: "m.text".into(),
        origin_ts: 1004,
    };
    assert!(
        db.ingest_message(
            &message,
            &[MessageTarget {
                session_id: internal.id.clone(),
                wake: true
            }],
            1004
        )
        .is_err()
    );
    assert_eq!(count(&sql(&root), "admitted_messages"), 0);
    let matrix = SessionBinding {
        id: "duplicate_matrix".into(),
        engagement_id: agents[0].clone(),
        room_id: "!project:example.test".into(),
        thread_root: None,
    };
    assert!(db.register_session(&matrix).is_err());
    assert_eq!(db.resolve_session(&matrix).unwrap().id, "a");
    assert_eq!(
        sql(&root)
            .pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
            .unwrap(),
        23
    );
    db.enqueue_dispatch(&dispatch("closed", &internal.id, None))
        .unwrap();
    sql(&root)
        .execute(
            "UPDATE internal_conversations SET state='closed' WHERE id=?1",
            [&group.id],
        )
        .unwrap();
    assert!(
        db.claim_dispatch("runner", 1005, 1000, 2000, 8)
            .unwrap()
            .is_none()
    );
}
