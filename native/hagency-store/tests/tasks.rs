mod common;
use common::*;
use hagency_core::tasks::*;
use hagency_store::{DomainRepository, EffectOutcome, Error};
use serde_json::json;

fn setup() -> (tempfile::TempDir, DomainRepository, String) {
    let root = tempfile::tempdir().unwrap();
    let mut db = DomainRepository::open(&root.path().join("state")).unwrap();
    db.register(&registration()).unwrap();
    let pool = resource("pool", "seat", 1000);
    db.put_resource(&pool).unwrap();
    let request = request("allocation", "小白", &pool, 100);
    let proof = proof(&request);
    let e = db.admit(&proof, 1000).unwrap();
    db.approve("approve", &proof, 1000).unwrap();
    let effect = db.claim_effect().unwrap().unwrap();
    db.observe_effect(
        &effect.id,
        effect.fence,
        &EffectOutcome::Applied {
            receipt: "fixture_identity_observed".into(),
        },
    )
    .unwrap();
    bind(&mut db, "s1", &e.id);
    db.register_workspace("work").unwrap();
    (root, db, e.id)
}
fn bind(db: &mut DomainRepository, id: &str, engagement: &str) {
    db.register_session(&SessionBinding {
        id: id.into(),
        engagement_id: engagement.into(),
        room_id: "!project:example.test".into(),
        thread_root: Some(format!("${id}")),
    })
    .unwrap();
}
fn input(id: &str, session: &str, task: Option<&str>, exclusive: bool) -> DispatchInput {
    DispatchInput {
        id: id.into(),
        session_id: session.into(),
        task_id: task.map(str::to_owned),
        resources: vec![ResourceLease {
            id: "work".into(),
            exclusive,
        }],
        payload: json!({"instruction":"verify the code","inputIds":["message_1"]}),
    }
}
fn claim(db: &mut DomainRepository, now: u64) -> RunnerCapability {
    db.claim_dispatch("runner", now, 60_000, 120_000, 8)
        .unwrap()
        .unwrap()
}
fn transition(status: TaskState) -> TaskMutation {
    TaskMutation::Transition {
        status,
        waiting_reason: None,
        waiting_until: None,
    }
}
fn sql(root: &tempfile::TempDir) -> rusqlite::Connection {
    rusqlite::Connection::open(root.path().join("state/domain.sqlite3")).unwrap()
}
fn count(db: &rusqlite::Connection, table: &str) -> u64 {
    db.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

#[test]
fn native_payload_dispatch_replay() {
    let (root, mut db, _) = setup();
    let mut d = input("numeric", "s1", None, false);
    d.payload = json!({"score":0.25,"nested":{"weight":1e-7},"results":[true,null,0.1+0.2]});
    d.payload["large_data"] = json!(9007199254740993u64);
    let expected: serde_json::Value =
        serde_json::from_str(&hagency_core::canonical::encode_payload(&d.payload).unwrap())
            .unwrap();
    assert_eq!(expected["large_data"], json!(9007199254740992u64)); // JS Number data, never an authority identifier.
    db.enqueue_dispatch(&d).unwrap();
    db.enqueue_dispatch(&d).unwrap();
    let mut changed = d.clone();
    changed.payload["score"] = json!(0.5);
    assert!(matches!(
        db.enqueue_dispatch(&changed),
        Err(Error::Conflict)
    ));
    drop(db);
    let mut db = DomainRepository::open(&root.path().join("state")).unwrap();
    db.enqueue_dispatch(&d).unwrap();
    let cap = claim(&mut db, 1001);
    assert_eq!(db.start_dispatch(&cap, 1002).unwrap(), expected);
    db.complete_dispatch(&cap, &json!({"score":0.75}), 1003)
        .unwrap();
    db.enqueue_dispatch(&d).unwrap();
}

#[test]
fn native_task_capability_scope() {
    let (_root, mut db, engagement) = setup();
    db.create_canonical_task("task", "s1", "Scoped work", 1000)
        .unwrap();
    db.create_canonical_task("other", "s1", "Unbound other task", 1000)
        .unwrap();
    let original = input("d1", "s1", Some("task"), true);
    db.enqueue_dispatch(&original).unwrap();
    let cap = claim(&mut db, 1000);
    assert!(matches!(
        db.runner_task(&cap, "task", 1001),
        Err(Error::RunnerAuthority)
    ));
    assert!(
        db.mutate_task(
            &cap,
            "task",
            "early",
            &TaskMutation::Comment {
                text: "too soon".into()
            },
            1001
        )
        .is_err()
    );
    assert_eq!(db.start_dispatch(&cap, 1001).unwrap(), original.payload);
    assert!(db.start_dispatch(&cap, 1002).is_err()); // Lost start response never issues the payload twice.
    assert_eq!(
        db.runner_task(&cap, "task", 1002).unwrap().status,
        TaskState::InProgress
    );
    assert_eq!(db.runner_tasks(&cap, "", 100, 1002).unwrap().len(), 1);
    assert!(db.runner_tasks(&cap, "", 101, 1002).is_err());
    assert!(db.runner_task(&cap, "other", 1002).is_err());
    for bad in [
        RunnerCapability {
            secret: "0".repeat(64),
            ..cap.clone()
        },
        RunnerCapability {
            runner_id: "impostor".into(),
            ..cap.clone()
        },
        RunnerCapability {
            fence: cap.fence + 1,
            ..cap.clone()
        },
    ] {
        assert!(matches!(
            db.runner_task(&bad, "task", 1002),
            Err(Error::RunnerAuthority)
        ));
        assert!(
            db.mutate_task(&bad, "task", "bad", &transition(TaskState::Done), 1002)
                .is_err()
        );
    }
    assert!(db.runner_task(&cap, "task", 61_000).is_err());
    db.park_dispatch(&cap, true, 1003).unwrap();
    assert!(db.runner_task(&cap, "task", 1004).is_err());
    assert!(
        db.mutate_task(&cap, "task", "parked", &transition(TaskState::Done), 1004)
            .is_err()
    );
    db.renew_dispatch(&cap, 1004, 60_000).unwrap();
    db.park_dispatch(&cap, false, 1005).unwrap();
    let beat = TaskMutation::Execution {
        heartbeat: true,
        waiting_reason: TextPatch::Missing,
        waiting_until: TextPatch::Missing,
    };
    assert_eq!(
        db.mutate_task(&cap, "task", "beat", &beat, 1010)
            .unwrap()
            .task
            .heartbeat_at,
        Some(1010)
    );
    assert!(
        serde_json::from_value::<TaskMutation>(
            json!({"action":"execution","heartbeat":true,"status":"done"})
        )
        .is_err()
    );
    assert!(
        serde_json::from_value::<TaskMutation>(
            json!({"action":"comment","text":"verified","author":"impostor"})
        )
        .is_err()
    );
    db.mutate_task(
        &cap,
        "task",
        "comment",
        &TaskMutation::Comment {
            text: "Acceptance checks passed".into(),
        },
        1011,
    )
    .unwrap();
    let comments = db.runner_comments(&cap, "task", 0, 1, 1012).unwrap();
    assert_eq!(comments[0].author, "小白");
    assert!(
        db.runner_comments(&cap, "task", comments[0].sequence, 1, 1012)
            .unwrap()
            .is_empty()
    );
    assert!(
        db.mutate_task(&cap, "task", "accept", &TaskMutation::Accept, 1013)
            .is_err()
    );
    assert!(
        db.mutate_task(
            &cap,
            "task",
            "invalid",
            &transition(TaskState::Blocked),
            1013
        )
        .is_err()
    );
    let blocked = TaskMutation::Transition {
        status: TaskState::Blocked,
        waiting_reason: Some("waiting for dependency".into()),
        waiting_until: Some("2026-10-01T00:00:00Z".into()),
    };
    assert_eq!(
        db.mutate_task(&cap, "task", "block", &blocked, 1014)
            .unwrap()
            .task
            .status,
        TaskState::Blocked
    );
    let keep: TaskMutation =
        serde_json::from_value(json!({"action":"execution","heartbeat":true})).unwrap();
    assert!(
        db.mutate_task(&cap, "task", "keep_wait", &keep, 1014)
            .unwrap()
            .task
            .waiting_reason
            .is_some()
    );
    let clear: TaskMutation = serde_json::from_value(
        json!({"action":"execution","heartbeat":false,"waiting_reason":null,"waiting_until":null}),
    )
    .unwrap();
    let cleared = db
        .mutate_task(&cap, "task", "clear_wait", &clear, 1014)
        .unwrap()
        .task;
    assert!(cleared.waiting_reason.is_none());
    assert!(cleared.waiting_until.is_none());
    assert!(matches!(
        db.mutate_task(&cap, "task", "keep_wait", &clear, 1014),
        Err(Error::Conflict)
    ));
    db.mutate_task(
        &cap,
        "task",
        "resume",
        &transition(TaskState::InProgress),
        1015,
    )
    .unwrap();
    db.complete_dispatch(
        &cap,
        &json!({"text":"All done!","innerResult":"passed"}),
        1016,
    )
    .unwrap();
    assert_eq!(
        db.canonical_task("task").unwrap().status,
        TaskState::InProgress
    );
    assert!(db.runner_task(&cap, "task", 1017).is_err());
    db.enqueue_dispatch(&input("d2", "s1", Some("task"), true))
        .unwrap();
    let next = claim(&mut db, 1020);
    db.start_dispatch(&next, 1021).unwrap();
    db.revoke("revoke", &engagement).unwrap();
    assert!(
        db.mutate_task(&next, "task", "done", &transition(TaskState::Done), 1022)
            .is_err()
    );
}

#[test]
fn native_task_receipt_atomicity() {
    let (root, mut db, _) = setup();
    db.create_canonical_task("task", "s1", "Scoped work", 1000)
        .unwrap();
    db.enqueue_dispatch(&input("d1", "s1", Some("task"), true))
        .unwrap();
    let cap = claim(&mut db, 1000);
    db.start_dispatch(&cap, 1001).unwrap();
    let inspect = sql(&root);
    let before = count(&inspect, "task_outbox");
    inspect.execute_batch("CREATE TRIGGER fail_task_receipt BEFORE INSERT ON task_operation_receipts BEGIN SELECT RAISE(ABORT,'fixture receipt failure'); END;").unwrap();
    let comment = TaskMutation::Comment {
        text: "verified".into(),
    };
    assert!(matches!(
        db.mutate_task(&cap, "task", "comment", &comment, 1002),
        Err(Error::Sqlite(_))
    ));
    assert!(
        db.runner_comments(&cap, "task", 0, 100, 1003)
            .unwrap()
            .is_empty()
    );
    assert_eq!(count(&inspect, "task_outbox"), before);
    assert_eq!(db.canonical_task("task").unwrap().updated_at, 1001);
    assert!(
        db.mutate_task(&cap, "task", "done", &transition(TaskState::Done), 1003)
            .is_err()
    );
    assert_eq!(db.canonical_task("task").unwrap().execution_epoch, 0);
    assert_eq!(count(&inspect, "task_operation_receipts"), 0);
    inspect
        .execute_batch("DROP TRIGGER fail_task_receipt")
        .unwrap();
    assert!(
        !db.mutate_task(&cap, "task", "comment", &comment, 1004)
            .unwrap()
            .replayed
    );
    assert!(
        db.mutate_task(&cap, "task", "comment", &comment, 1005)
            .unwrap()
            .replayed
    );
    assert!(matches!(
        db.mutate_task(
            &cap,
            "task",
            "comment",
            &TaskMutation::Comment {
                text: "different".into()
            },
            1006
        ),
        Err(Error::Conflict)
    ));
    let done = db
        .mutate_task(&cap, "task", "done", &transition(TaskState::Done), 1007)
        .unwrap();
    assert_eq!(done.task.status, TaskState::Done);
    assert_eq!(done.task.execution_epoch, 1);
    assert_eq!(done.task.completed_at, Some(1007));
    assert_eq!(done.task.waiting_reason, None);
    let replay = db
        .mutate_task(&cap, "task", "done", &transition(TaskState::Done), 1008)
        .unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.task.execution_epoch, 1);
    assert!(
        db.mutate_task(&cap, "task", "new comment", &comment, 1009)
            .is_err()
    );
    assert_eq!(count(&inspect, "task_comments"), 1);
    assert_eq!(count(&inspect, "task_operation_receipts"), 2);
    let all = db.task_events(0, 100).unwrap();
    let page = db.task_events(0, 1).unwrap();
    assert_eq!(
        db.task_events(page[0].sequence, 100).unwrap().len(),
        all.len() - 1
    );
    let raw: String = inspect
        .query_row(
            "SELECT capability_hash FROM runner_dispatches WHERE id='d1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_ne!(raw, cap.secret);
    assert!(!format!("{cap:?}").contains(&cap.secret));
    drop(inspect);
    drop(db);
    let mut db = DomainRepository::open(&root.path().join("state")).unwrap();
    assert_eq!(db.canonical_task("task").unwrap().execution_epoch, 1);
    assert!(db.runner_task(&cap, "task", 1010).is_err());
    let inspect = sql(&root);
    assert_eq!(count(&inspect, "task_operation_receipts"), 2);
    assert_eq!(count(&inspect, "task_outbox"), before + 2);
    let mut recovered = input("completed_recovery", "s1", None, true);
    recovered.payload =
        json!({"instruction":"Report the inspected result of the already completed task"});
    db.recover_dispatch(
        "d1",
        &recovered,
        "Old process stopped; completed artifact inspected",
        1011,
    )
    .unwrap();
    let report = claim(&mut db, 1012);
    db.start_dispatch(&report, 1013).unwrap();
    assert!(
        db.mutate_task(
            &report,
            "task",
            "must_not_reopen",
            &transition(TaskState::InProgress),
            1013
        )
        .is_err()
    );
    db.complete_dispatch(
        &report,
        &json!({"text":"Verified completion recovered"}),
        1014,
    )
    .unwrap();
    assert_eq!(db.canonical_task("task").unwrap().execution_epoch, 1);
}

#[test]
fn native_dispatch_recovery() {
    let (root, mut db, _) = setup();
    db.create_canonical_task("task", "s1", "Scoped work", 1000)
        .unwrap();
    let original = input("d1", "s1", Some("task"), true);
    db.enqueue_dispatch(&original).unwrap();
    db.enqueue_dispatch(&original).unwrap();
    let mut changed = original.clone();
    changed.payload = json!({"different":true});
    assert!(matches!(
        db.enqueue_dispatch(&changed),
        Err(Error::Conflict)
    ));
    let first = claim(&mut db, 1000);
    drop(db);
    let mut db = DomainRepository::open(&root.path().join("state")).unwrap();
    let second = claim(&mut db, 1001);
    assert_eq!(first.dispatch_id, second.dispatch_id);
    assert!(second.fence > first.fence);
    assert!(db.start_dispatch(&first, 1002).is_err());
    db.fail_before_start(&second, 1002, 100).unwrap();
    assert!(
        db.claim_dispatch("runner", 1050, 100, 100, 8)
            .unwrap()
            .is_none()
    );
    drop(db);
    let mut db = DomainRepository::open(&root.path().join("state")).unwrap();
    assert!(
        db.claim_dispatch("runner", 1050, 100, 100, 8)
            .unwrap()
            .is_none()
    );
    let third = claim(&mut db, 1102);
    assert!(third.fence > second.fence);
    assert_eq!(db.start_dispatch(&third, 1103).unwrap(), original.payload);
    db.enqueue_dispatch(&input("old_queued", "s1", Some("task"), true))
        .unwrap();
    drop(db); // Covers process owner death and a lost start response alike.
    let mut db = DomainRepository::open(&root.path().join("state")).unwrap();
    assert!(
        db.claim_dispatch("runner", 1104, 100, 100, 8)
            .unwrap()
            .is_none()
    );
    assert!(matches!(
        db.enqueue_dispatch(&input("unsafe", "s1", Some("task"), true)),
        Err(Error::Quarantined)
    ));
    assert!(
        db.complete_dispatch(&third, &json!({"text":"late done"}), 1104)
            .is_err()
    );
    db.record_late_output(&third, &json!({"text":"late done"}))
        .unwrap();
    assert_eq!(
        db.canonical_task("task").unwrap().status,
        TaskState::InProgress
    );
    let mut recovery = original.clone();
    recovery.id = "recovery".into();
    assert!(
        db.recover_dispatch("d1", &recovery, "inspected workspace", 1105)
            .is_err()
    );
    recovery.payload = json!({"instruction":"Process has exited; inspect prior files before continuing","originalDispatch":"d1"});
    db.recover_dispatch(
        "d1",
        &recovery,
        "Fixture adapter confirms old process stopped and workspace inspected",
        1106,
    )
    .unwrap();
    let fourth = claim(&mut db, 1107);
    assert_eq!(fourth.dispatch_id, "recovery");
    db.start_dispatch(&fourth, 1108).unwrap();
    db.park_dispatch(&fourth, true, 1109).unwrap();
    db.reconcile_dispatches(61_107).unwrap();
    assert!(
        db.claim_dispatch("runner", 61_108, 100, 100, 8)
            .unwrap()
            .is_none()
    );
    let inspect = sql(&root);
    assert_eq!(
        inspect
            .query_row(
                "SELECT state FROM runner_dispatches WHERE id='recovery'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
        "outcome_unknown"
    );
    assert!(
        inspect
            .query_row(
                "SELECT dirty FROM workspace_resources WHERE id='work'",
                [],
                |r| r.get::<_, bool>(0)
            )
            .unwrap()
    );
    assert!(
        !inspect
            .query_row("SELECT accepted FROM runner_outputs LIMIT 1", [], |r| r
                .get::<_, bool>(0))
            .unwrap()
    );
    assert_eq!(count(&inspect, "runner_attempts"), 4);
    assert_eq!(count(&inspect, "dispatch_recoveries"), 1);
    assert_eq!(
        inspect
            .query_row(
                "SELECT state FROM runner_dispatches WHERE id='old_queued'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
        "superseded"
    );
    let (_expiry_root, mut expiry, _) = setup();
    expiry
        .enqueue_dispatch(&input("unstarted", "s1", None, true))
        .unwrap();
    let old = claim(&mut expiry, 1000);
    expiry.reconcile_dispatches(61_000).unwrap();
    assert!(expiry.start_dispatch(&old, 61_000).is_err());
    let fresh = claim(&mut expiry, 61_001);
    assert_eq!(fresh.dispatch_id, old.dispatch_id);
    assert!(fresh.fence > old.fence);
}

#[test]
fn native_recovery_payload_identity() {
    let (root, mut db, _) = setup();
    let mut original = input("numeric_original", "s1", None, true);
    original.payload = json!({"instruction":"Inspect code","weight":1});
    db.enqueue_dispatch(&original).unwrap();
    let cap = claim(&mut db, 1000);
    db.start_dispatch(&cap, 1001).unwrap();
    drop(db);
    let mut db = DomainRepository::open(&root.path().join("state")).unwrap();
    let mut replacement = original.clone();
    replacement.id = "numeric_recovery".into();
    replacement.payload["weight"] = json!(1.0);
    assert_ne!(original.payload, replacement.payload); // Rust Value distinguishes the numeric forms.
    assert!(
        db.recover_dispatch(
            &original.id,
            &replacement,
            "Inspected stopped process",
            1002
        )
        .is_err()
    );
    assert_eq!(count(&sql(&root), "dispatch_recoveries"), 0);
    replacement.payload["instruction"] = json!("Inspect partial output before proceeding");
    db.recover_dispatch(
        &original.id,
        &replacement,
        "Inspected stopped process",
        1003,
    )
    .unwrap();
    let recovery = claim(&mut db, 1004);
    db.start_dispatch(&recovery, 1005).unwrap();
}

#[tokio::test]
async fn native_dispatch_resource_and_coordinator_scope() {
    let (root, mut db, engagement) = setup();
    for id in ["s2", "s3", "worker"] {
        bind(&mut db, id, &engagement);
    }
    db.enqueue_dispatch(&input("d1", "s1", None, false))
        .unwrap();
    db.enqueue_dispatch(&input("d2", "s2", None, false))
        .unwrap();
    db.enqueue_dispatch(&input("d3", "s3", None, true)).unwrap();
    let a = db
        .claim_dispatch("a", 1000, 60_000, 120_000, 1)
        .unwrap()
        .unwrap();
    assert!(
        db.claim_dispatch("b", 1000, 60_000, 120_000, 1)
            .unwrap()
            .is_none()
    );
    let b = db
        .claim_dispatch("b", 1000, 60_000, 120_000, 3)
        .unwrap()
        .unwrap();
    assert_eq!(b.dispatch_id, "d2");
    assert!(
        db.claim_dispatch("c", 1000, 60_000, 120_000, 3)
            .unwrap()
            .is_none()
    );
    db.start_dispatch(&a, 1001).unwrap();
    db.start_dispatch(&b, 1001).unwrap();
    let child = db
        .create_coordinator_task(&a, "child", "worker", "Implement delegated step", 1002)
        .unwrap();
    db.create_coordinator_task(&a, "child", "worker", "Implement delegated step", 1002)
        .unwrap();
    assert_eq!(db.runner_task(&a, &child.id, 1002).unwrap().id, child.id);
    assert!(
        db.mutate_task(&a, &child.id, "no", &transition(TaskState::Done), 1002)
            .is_err()
    );
    assert!(db.runner_task(&b, &child.id, 1002).is_err());
    assert!(db.runner_tasks(&b, "", 100, 1002).unwrap().is_empty());
    db.park_dispatch(&a, true, 1003).unwrap();
    db.complete_dispatch(&b, &json!({}), 1003).unwrap();
    assert!(db.claim_dispatch("c", 1004, 100, 100, 3).unwrap().is_none());
    db.park_dispatch(&a, false, 1005).unwrap();
    db.complete_dispatch(&a, &json!({}), 1005).unwrap();
    let c = claim(&mut db, 1006);
    assert_eq!(c.dispatch_id, "d3");
    db.start_dispatch(&c, 1007).unwrap();
    db.complete_dispatch(&c, &json!({}), 1008).unwrap();
    db.enqueue_dispatch(&input("later", "s1", None, false))
        .unwrap();
    db.enqueue_dispatch(&input("same_session", "s1", None, false))
        .unwrap();
    let later = claim(&mut db, 1009);
    db.start_dispatch(&later, 1010).unwrap();
    assert_eq!(
        db.runner_tasks(&later, "", 100, 1011).unwrap()[0].id,
        "child"
    );
    assert!(
        db.claim_dispatch("extra", 1011, 100, 100, 8)
            .unwrap()
            .is_none()
    );
    let inspect = sql(&root);
    assert_eq!(count(&inspect, "resource_leases"), 1);
    let mut binding = SessionBinding {
        id: "s1".into(),
        engagement_id: engagement,
        room_id: "!other:example.test".into(),
        thread_root: None,
    };
    assert!(matches!(
        db.register_session(&binding),
        Err(Error::Conflict)
    ));
    binding.id = "missing_allocation".into();
    binding.engagement_id = "unknown".into();
    assert!(db.register_session(&binding).is_err());
    db.complete_dispatch(&later, &json!({}), 1012).unwrap();
    db.enqueue_dispatch(&input("concurrent", "s2", None, true))
        .unwrap();
    let store = hagency_store::DomainStore::start(db, 16).unwrap();
    let (a, b) = tokio::join!(
        store.claim_dispatch("one".into(), 1013, 100, 100, 8),
        store.claim_dispatch("two".into(), 1013, 100, 100, 8)
    );
    assert_eq!(
        usize::from(a.unwrap().is_some()) + usize::from(b.unwrap().is_some()),
        1
    );
    store.shutdown().await.unwrap();
}
