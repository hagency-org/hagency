mod common;
use common::*;
use hagency_core::completions::*;
use hagency_core::{replies::*, tasks::*};
use hagency_store::{DomainRepository, EffectOutcome, Error, OwnedDispatchScope, OwnedFailure};
use serde_json::json;
use std::collections::BTreeSet;

struct Fixture {
    root: tempfile::TempDir,
    db: DomainRepository,
    engagement: String,
    room: MatrixRoomObservation,
    transport: MatrixTransportObservation,
    cap: RunnerCapability,
    admission: OwnedDispatchScope,
    started: OwnedDispatchScope,
}
fn dispatch(id: &str, session: &str, task: Option<&str>) -> DispatchInput {
    DispatchInput {
        id: id.into(),
        session_id: session.into(),
        task_id: task.map(str::to_owned),
        resources: vec![ResourceLease {
            id: "workspace".into(),
            exclusive: true,
        }],
        payload: json!({"instruction":"Verify result"}),
    }
}
impl Fixture {
    fn new(direct: bool, root: Option<&str>) -> Self {
        Self::named(direct, root, "dispatch")
    }
    fn named(direct: bool, root: Option<&str>, dispatch_id: &str) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let mut db = DomainRepository::open(&temp.path().join("state")).unwrap();
        db.register(&registration()).unwrap();
        let pool = resource("pool", "seat", 1000);
        db.put_resource(&pool).unwrap();
        let p = proof(&request("one", "Worker", &pool, 100));
        let e = db.admit(&p, 1000).unwrap();
        db.approve("approve", &p, 1000).unwrap();
        let effect = db.claim_effect().unwrap().unwrap();
        db.observe_effect(
            &effect.id,
            effect.fence,
            &EffectOutcome::Applied {
                receipt: "fixture provision".into(),
            },
        )
        .unwrap();
        let transport = MatrixTransportObservation {
            engagement_id: e.id.clone(),
            registration_generation: 1,
            generation: 1,
            sender_mxid: "@worker:example.test".into(),
            device_id: "DEVICE_1".into(),
        };
        db.observe_matrix_transport(&transport, 1001).unwrap();
        let room = MatrixRoomObservation {
            engagement_id: e.id.clone(),
            registration_generation: 1,
            transport_generation: 1,
            room_id: if direct {
                "!direct:example.test"
            } else {
                "!project:example.test"
            }
            .into(),
            generation: 1,
            privacy: if direct {
                RoomPrivacy::Direct {
                    human_mxid: "@owner:example.test".into(),
                }
            } else {
                RoomPrivacy::Group {}
            },
            joined: BTreeSet::from(["@worker:example.test".into(), "@owner:example.test".into()]),
            invite_only: true,
            encrypted: direct,
        };
        db.observe_matrix_room(&room, 1002).unwrap();
        let binding = SessionBinding {
            id: "session".into(),
            engagement_id: e.id.clone(),
            room_id: room.room_id.clone(),
            thread_root: root.map(str::to_owned),
        };
        db.resolve_verified_matrix_session(&binding, 1003).unwrap();
        db.create_canonical_task("task", &binding.id, "Verify task", 1004)
            .unwrap();
        db.register_workspace("workspace").unwrap();
        db.enqueue_dispatch(&dispatch(dispatch_id, &binding.id, Some("task")))
            .unwrap();
        let cap = db
            .claim_dispatch("runner", 1005, 60_000, 120_000, 8)
            .unwrap()
            .unwrap();
        let admission = db.owned_dispatch_scope(&cap, 1006).unwrap();
        let started = db
            .start_owned_dispatch(&cap, admission.fingerprint(), 1006)
            .unwrap();
        Self {
            root: temp,
            db,
            engagement: e.id,
            room,
            transport,
            cap,
            admission,
            started,
        }
    }
    fn finish(&mut self) -> CompletionReceipt {
        self.db
            .complete_task_with_reply(&self.cap, &content(), 1007)
            .unwrap()
    }
    fn count(&self, query: &str) -> u64 {
        self.sql().query_row(query, [], |r| r.get(0)).unwrap()
    }
    fn sql(&self) -> rusqlite::Connection {
        rusqlite::Connection::open(self.root.path().join("state/domain.sqlite3")).unwrap()
    }
}
fn content() -> CompleteTaskWithReply {
    CompleteTaskWithReply {
        id: "task".into(),
        call_id: "finish".into(),
        body: "Verified **中文 private final result**".into(),
    }
}

#[test]
fn native_owned_completion_atomic_finish() {
    for direct in [false, true] {
        let mut f = Fixture::new(direct, if direct { None } else { Some("$thread") });
        for bad in [
            CompleteTaskWithReply {
                body: "".into(),
                ..content()
            },
            CompleteTaskWithReply {
                body: "x".repeat(32769),
                ..content()
            },
            CompleteTaskWithReply {
                id: "foreign".into(),
                ..content()
            },
        ] {
            assert!(f.db.complete_task_with_reply(&f.cap, &bad, 1007).is_err());
            assert_eq!(
                f.db.canonical_task("task").unwrap().status,
                TaskState::InProgress
            );
            assert_eq!(f.count("SELECT COUNT(*) FROM owned_task_completions"), 0);
        }
        let held = f.finish();
        assert_eq!(held.state, CompletionState::Held);
        assert_eq!(held.execution_epoch, 1);
        assert_eq!(f.db.canonical_task("task").unwrap().status, TaskState::Done);
        assert_eq!(f.count("SELECT COUNT(*) FROM final_replies"), 0);
        assert_eq!(f.count("SELECT COUNT(*) FROM resource_leases"), 1);
        assert_eq!(f.count("SELECT dirty FROM workspace_resources"), 1);
        assert_eq!(f.count("SELECT quarantined FROM runner_sessions"), 1);
        assert!(
            f.db.check_owned_dispatch(&f.cap, f.started.fingerprint(), 1008)
                .is_err()
        );
        assert!(f.db.runner_task(&f.cap, "task", 1008).is_err());
        assert!(
            f.db.mutate_task(&f.cap, "task", "next", &TaskMutation::Accept, 1008)
                .is_err()
        );
        let replay =
            f.db.complete_task_with_reply(&f.cap, &content(), 1008)
                .unwrap();
        assert_eq!(replay.id, held.id);
        assert!(replay.replayed);
        assert!(matches!(
            f.db.complete_task_with_reply(
                &f.cap,
                &CompleteTaskWithReply {
                    body: "changed".into(),
                    ..content()
                },
                1008
            ),
            Err(Error::Conflict)
        ));
        assert_eq!(f.count("SELECT COUNT(*) FROM task_operation_receipts"), 1);
        assert_eq!(
            f.count("SELECT COUNT(*) FROM task_outbox WHERE kind='transition'"),
            1
        );
        let r =
            f.db.observe_owned_completion(&f.cap, &f.started)
                .unwrap()
                .unwrap();
        // Store-level trusted host seam test. This does not manufacture process
        // evidence: actual retained-owner qualification is in owned_mcp.rs.
        let ready =
            f.db.publish_owned_completion(&f.cap, &f.started, &r, 1009)
                .unwrap();
        assert_eq!(ready.state, CompletionState::Ready);
        assert_eq!(f.count("SELECT COUNT(*) FROM resource_leases"), 0);
        assert_eq!(f.count("SELECT dirty FROM workspace_resources"), 0);
        assert_eq!(f.count("SELECT quarantined FROM runner_sessions"), 0);
        assert_eq!(
            f.count("SELECT COUNT(*) FROM dispatch_stops WHERE settled_at IS NOT NULL"),
            1
        );
        assert_eq!(
            f.count("SELECT COUNT(*) FROM runner_outputs WHERE accepted=1"),
            0
        );
        let claim = f.db.claim_final_reply(1010, 1000).unwrap().unwrap();
        let preview = f.db.preview_final_reply(&claim, 1011).unwrap();
        assert_eq!(preview.body, content().body);
        assert_eq!(preview.route.encrypted, direct);
        assert_eq!(
            preview.route.thread_root,
            if direct { None } else { Some("$thread".into()) }
        );
        assert_eq!(preview.route.room_id, f.room.room_id);
    }
}

#[test]
fn native_owned_completion_scope_and_custody() {
    let mut f = Fixture::new(false, Some("$thread"));
    f.finish();
    let r =
        f.db.observe_owned_completion(&f.cap, &f.started)
            .unwrap()
            .unwrap();
    assert!(f.db.observe_owned_completion(&f.cap, &f.admission).is_err());
    assert!(
        f.db.publish_owned_completion(&f.cap, &f.admission, &r, 1008)
            .is_err()
    );
    let mut g = Fixture::named(false, Some("$thread"), "different_attempt");
    g.finish();
    let foreign_ref =
        g.db.observe_owned_completion(&g.cap, &g.started)
            .unwrap()
            .unwrap();
    assert!(
        f.db.publish_owned_completion(&f.cap, &f.started, &foreign_ref, 1008)
            .is_err()
    );
    assert!(
        f.db.publish_owned_completion(&f.cap, &g.started, &r, 1008)
            .is_err()
    );
    let wrong = RunnerCapability {
        fence: f.cap.fence + 1,
        ..f.cap.clone()
    };
    assert!(
        f.db.publish_owned_completion(&wrong, &f.started, &r, 1008)
            .is_err()
    );
    assert!(
        f.db.publish_owned_completion(&f.cap, &f.started, &r, 31007)
            .is_err()
    );
    assert_eq!(f.count("SELECT COUNT(*) FROM resource_leases"), 1);
    assert_eq!(f.count("SELECT COUNT(*) FROM final_replies"), 0);
    // A separate stop for the same session must not be retired by this owner.
    f.sql().execute("INSERT INTO runner_dispatches(id,session_id,task_id,input,digest,state,fence,not_before) SELECT 'other',session_id,task_id,json_set(input,'$.id','other'),digest,'outcome_unknown',1,0 FROM runner_dispatches WHERE id='dispatch'",[]).unwrap();
    f.sql().execute("INSERT INTO dispatch_stops(dispatch_id,fence,reason,created_at) VALUES('other',1,'unrelated',1008)",[]).unwrap();
    assert!(matches!(
        f.db.publish_owned_completion(&f.cap, &f.started, &r, 1009),
        Err(Error::Quarantined)
    ));
    assert_eq!(
        f.count("SELECT COUNT(*) FROM dispatch_stops WHERE settled_at IS NULL"),
        2
    );
    assert_eq!(f.count("SELECT COUNT(*) FROM resource_leases"), 1);
    // Move the unrelated attempt into a distinct session; shared resource custody
    // alone must still block release/publication by the original owner.
    f.sql().execute("INSERT INTO runner_sessions(id,engagement_id,binding) SELECT 'other_session',engagement_id,json_set(binding,'$.id','other_session','$.room_id','!other:example.test') FROM runner_sessions WHERE id='session'",[]).unwrap();
    f.sql()
        .execute(
            "UPDATE runner_dispatches SET session_id='other_session' WHERE id='other'",
            [],
        )
        .unwrap();
    f.sql().execute("INSERT INTO dispatch_resources(dispatch_id,resource_id,exclusive) VALUES('other','workspace',1)",[]).unwrap();
    f.sql().execute("INSERT INTO resource_leases(dispatch_id,resource_id,exclusive) VALUES('other','workspace',1)",[]).unwrap();
    assert!(matches!(
        f.db.publish_owned_completion(&f.cap, &f.started, &r, 1010),
        Err(Error::Quarantined)
    ));
    assert_eq!(
        f.count("SELECT COUNT(*) FROM dispatch_stops WHERE settled_at IS NULL"),
        2
    );
    assert_eq!(f.count("SELECT COUNT(*) FROM resource_leases"), 2);
}

#[test]
fn native_owned_completion_retirement_and_receipt_namespace() {
    for mode in ["cancel", "epoch", "revoke", "promotion", "transport"] {
        let mut f = Fixture::new(true, None);
        f.finish();
        let r =
            f.db.observe_owned_completion(&f.cap, &f.started)
                .unwrap()
                .unwrap();
        match mode {
            "cancel" => {
                f.db.observe_owned_failure(&f.cap, OwnedFailure::Cancelled, 1008)
                    .unwrap();
            }
            "epoch" => {
                f.sql()
                    .execute(
                        "UPDATE canonical_tasks SET config=json_set(config,'$.execution_epoch',2)",
                        [],
                    )
                    .unwrap();
            }
            "revoke" => {
                f.db.revoke("revoke", &f.engagement).unwrap();
            }
            "promotion" => {
                let mut room = f.room.clone();
                room.privacy = RoomPrivacy::Group {};
                room.generation += 1;
                room.joined.insert("@third:example.test".into());
                f.db.observe_matrix_room(&room, 1008).unwrap();
            }
            "transport" => {
                let mut transport = f.transport.clone();
                transport.generation += 1;
                transport.device_id = "NEW_DEVICE".into();
                f.db.observe_matrix_transport(&transport, 1008).unwrap();
            }
            _ => unreachable!(),
        }
        assert!(
            f.db.publish_owned_completion(&f.cap, &f.started, &r, 1009)
                .is_err(),
            "{mode}"
        );
        assert_eq!(f.count("SELECT COUNT(*) FROM final_replies"), 0);
        assert_eq!(f.count("SELECT COUNT(*) FROM resource_leases"), 1);
        assert_eq!(f.db.canonical_task("task").unwrap().status, TaskState::Done);
    }
    let mut f = Fixture::new(false, None);
    f.db.mutate_task(
        &f.cap,
        "task",
        "finish",
        &TaskMutation::Execution {
            heartbeat: true,
            waiting_reason: TextPatch::Missing,
            waiting_until: TextPatch::Missing,
        },
        1007,
    )
    .unwrap();
    assert!(matches!(
        f.db.complete_task_with_reply(&f.cap, &content(), 1008),
        Err(Error::Conflict)
    ));
    assert_eq!(
        f.db.canonical_task("task").unwrap().status,
        TaskState::InProgress
    );
    f.db.mutate_task(
        &f.cap,
        "task",
        "task_only",
        &TaskMutation::Transition {
            status: TaskState::Done,
            waiting_reason: None,
            waiting_until: None,
        },
        1008,
    )
    .unwrap();
    assert!(
        f.db.complete_task_with_reply(
            &f.cap,
            &CompleteTaskWithReply {
                call_id: "too_late".into(),
                ..content()
            },
            1009
        )
        .is_err()
    );
    assert_eq!(f.count("SELECT COUNT(*) FROM owned_task_completions"), 0);
}

#[test]
fn native_owned_completion_restart_has_no_reconstructed_owner() {
    let mut f = Fixture::new(false, None);
    f.finish();
    let state = f.root.path().join("state");
    drop(f.db);
    let mut db = DomainRepository::open(&state).unwrap();
    assert!(db.owned_dispatch_scope(&f.cap, 1008).is_err());
    assert!(
        db.start_owned_dispatch(&f.cap, f.started.fingerprint(), 1008)
            .is_err()
    );
    assert!(db.claim_final_reply(1008, 1000).unwrap().is_none());
    let replay = db
        .complete_task_with_reply(&f.cap, &content(), 1009)
        .unwrap();
    assert_eq!(replay.state, CompletionState::Held);
    assert!(replay.replayed);
    assert_eq!(db.canonical_task("task").unwrap().status, TaskState::Done);
}

#[test]
fn native_owned_completion_migration() {
    let f = Fixture::new(false, None);
    let path = f.root.path().join("state");
    drop(f.db);
    let sql = rusqlite::Connection::open(path.join("domain.sqlite3")).unwrap();
    remove_owned_completion_schema(&sql);
    sql.pragma_update(None, "user_version", 15).unwrap();
    drop(sql);
    let db = DomainRepository::open(&path).unwrap();
    drop(db);
    let sql = rusqlite::Connection::open(path.join("domain.sqlite3")).unwrap();
    assert_eq!(
        sql.query_row("PRAGMA user_version", [], |r| r.get::<_, u64>(0))
            .unwrap(),
        23
    );
    assert_eq!(
        sql.query_row("SELECT COUNT(*) FROM owned_task_completions", [], |r| r
            .get::<_, u64>(0))
            .unwrap(),
        0
    );
    sql.execute_batch(
        "ALTER TABLE owned_task_completions RENAME COLUMN deadline TO missing_deadline;",
    )
    .unwrap();
    drop(sql);
    assert!(matches!(DomainRepository::open(&path), Err(Error::Schema)));
}

#[test]
fn native_owned_completion_capacity_rolls_back_done() {
    let mut f = Fixture::new(false, None);
    let mut sql = f.sql();
    let tx = sql.transaction().unwrap();
    // Seed the bounded historical storage shape, not process/cleanup evidence.
    // The current attempt has no finish receipt and remains Started throughout.
    for n in 1..=128 {
        let call = format!("historical_{n}");
        tx.execute("INSERT INTO task_operation_receipts(dispatch_id,call_id,digest,response) VALUES('dispatch',?1,'historical','{}')",[&call]).unwrap();
        tx.execute("INSERT INTO owned_task_completions(id,dispatch_id,fence,task_id,execution_epoch,fingerprint,call_id,digest,body,route,deadline,state,created_at,updated_at) VALUES(?1,'dispatch',?2,'task',?2,'historical',?1,'historical','historical','{}',1,'cancelled',1,1)",rusqlite::params![call,n+200]).unwrap();
    }
    tx.commit().unwrap();
    drop(sql);
    assert!(matches!(
        f.db.complete_task_with_reply(&f.cap, &content(), 1007),
        Err(Error::Capacity)
    ));
    let task = f.db.canonical_task("task").unwrap();
    assert_eq!(task.status, TaskState::InProgress);
    assert_eq!(task.execution_epoch, 0);
    assert_eq!(
        f.count("SELECT COUNT(*) FROM task_operation_receipts WHERE call_id='finish'"),
        0
    );
    assert_eq!(f.count("SELECT COUNT(*) FROM dispatch_stops"), 0);
    assert_eq!(f.count("SELECT dirty FROM workspace_resources"), 0);
    assert!(
        f.db.check_owned_dispatch(&f.cap, f.started.fingerprint(), 1008)
            .is_ok()
    );
}
