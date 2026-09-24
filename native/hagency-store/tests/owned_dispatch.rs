mod common;
use common::*;
use hagency_core::tasks::*;
use hagency_store::{DomainRepository, EffectOutcome, Error, OwnedFailure, OwnedObservation};
use serde_json::json;

fn setup() -> (
    tempfile::TempDir,
    DomainRepository,
    String,
    RunnerCapability,
) {
    let root = tempfile::tempdir().unwrap();
    let mut db = DomainRepository::open(&root.path().join("state")).unwrap();
    db.register(&registration()).unwrap();
    let pool = resource("pool", "seat", 1000);
    db.put_resource(&pool).unwrap();
    let proof = proof(&request("allocated", "Worker", &pool, 100));
    let e = db.admit(&proof, 1000).unwrap();
    db.approve("approved", &proof, 1000).unwrap();
    let effect = db.claim_effect().unwrap().unwrap();
    db.observe_effect(
        &effect.id,
        effect.fence,
        &EffectOutcome::Applied {
            receipt: "offline_identity".into(),
        },
    )
    .unwrap();
    db.register_session(&SessionBinding {
        id: "session".into(),
        engagement_id: e.id.clone(),
        room_id: "!project:example.test".into(),
        thread_root: Some("$thread".into()),
    })
    .unwrap();
    db.register_workspace("workspace").unwrap();
    db.create_canonical_task("task", "session", "Host-bound work", 1000)
        .unwrap();
    db.enqueue_dispatch(&DispatchInput { id: "dispatch".into(), session_id: "session".into(), task_id: Some("task".into()), resources: vec![ResourceLease { id: "workspace".into(), exclusive: true }], payload: json!({"instruction":"verify 中文", "cwd":"/model/path", "task_id":"impostor", "score":0.25}) }).unwrap();
    let cap = db
        .claim_dispatch("runner", 1001, 1000, 2000, 1)
        .unwrap()
        .unwrap();
    (root, db, e.id, cap)
}
fn sql(root: &tempfile::TempDir) -> rusqlite::Connection {
    rusqlite::Connection::open(root.path().join("state/domain.sqlite3")).unwrap()
}

#[test]
fn native_owned_stopped_inspection_store() {
    let (root, mut db, _, cap) = setup();
    let before = db.owned_dispatch_scope(&cap, 1002).unwrap();
    // Store-only contract fixture: not claimed as physical process evidence.
    let inventory =
        json!({"profile":"stopped-content-inventory-v1","root":{"fixture":true},"entries":[]});
    assert!(matches!(
        db.record_owned_stop_inspection(&cap, &before, &inventory, 1002),
        Err(Error::RunnerAuthority)
    ));
    let started = db
        .start_owned_dispatch(&cap, before.fingerprint(), 1003)
        .unwrap();
    assert!(matches!(
        db.record_owned_stop_inspection(&cap, &started, &inventory, 1004),
        Err(Error::State)
    ));
    assert_eq!(
        db.observe_owned_failure(&cap, OwnedFailure::Protocol, 4000)
            .unwrap(),
        OwnedObservation::Fenced
    );
    let digest = db
        .record_owned_stop_inspection(&cap, &started, &inventory, 4001)
        .unwrap();
    let receipt = db
        .owned_stop_inspection(&cap.dispatch_id, cap.fence)
        .unwrap()
        .unwrap();
    assert_eq!(receipt["digest"], digest);
    assert_eq!(
        db.record_owned_stop_inspection(&cap, &started, &inventory, 4002)
            .unwrap(),
        digest
    );
    assert_eq!(
        db.owned_stop_inspection(&cap.dispatch_id, cap.fence)
            .unwrap(),
        Some(receipt.clone())
    );
    let mut changed = inventory.clone();
    changed["entries"] = json!([{"path":"changed"}]);
    assert!(matches!(
        db.record_owned_stop_inspection(&cap, &started, &changed, 4003),
        Err(Error::Conflict)
    ));
    for field in ["secret", "runner", "dispatch", "fence"] {
        let mut foreign = cap.clone();
        match field {
            "secret" => foreign.secret.push('x'),
            "runner" => foreign.runner_id.push('x'),
            "dispatch" => foreign.dispatch_id.push('x'),
            _ => foreign.fence += 1,
        }
        assert!(matches!(
            db.record_owned_stop_inspection(&foreign, &started, &inventory, 4004),
            Err(Error::RunnerAuthority)
        ));
    }
    assert_eq!(
        db.canonical_task("task").unwrap().status,
        TaskState::InProgress
    );
    let counts:(u64,u64,u64,u64)=sql(&root).query_row("SELECT (SELECT COUNT(*) FROM resource_leases),(SELECT COUNT(*) FROM dispatch_stops WHERE settled_at IS NULL),(SELECT dirty FROM workspace_resources),(SELECT quarantined FROM runner_sessions)",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).unwrap();
    assert_eq!(counts, (1, 1, 1, 1));
    drop(db);
    let mut db = DomainRepository::open(&root.path().join("state")).unwrap();
    assert_eq!(
        db.record_owned_stop_inspection(&cap, &started, &inventory, 5000)
            .unwrap(),
        digest
    );
    assert_eq!(
        db.owned_stop_inspection(&cap.dispatch_id, cap.fence)
            .unwrap(),
        Some(receipt)
    );
}

#[tokio::test]
async fn native_fleet_runner_service_scope() {
    let (root, mut db, first, cap) = setup();
    let pool = resource("pool", "seat", 1000);
    let proof = proof(&request("second_allocated", "OtherWorker", &pool, 100));
    let second = db.admit(&proof, 1000).unwrap();
    db.approve("second_approved", &proof, 1000).unwrap();
    let effect = db.claim_effect().unwrap().unwrap();
    db.observe_effect(
        &effect.id,
        effect.fence,
        &EffectOutcome::Applied {
            receipt: "offline second routing fixture".into(),
        },
    )
    .unwrap();
    db.register_session(&SessionBinding {
        id: "second_session".into(),
        engagement_id: second.id.clone(),
        room_id: "!other:example.test".into(),
        thread_root: None,
    })
    .unwrap();
    db.register_workspace("second_workspace").unwrap();
    db.create_canonical_task("second_task", "second_session", "Other original task", 1000)
        .unwrap();
    db.enqueue_dispatch(&DispatchInput {
        id: "second_dispatch".into(),
        session_id: "second_session".into(),
        task_id: Some("second_task".into()),
        resources: vec![ResourceLease {
            id: "second_workspace".into(),
            exclusive: true,
        }],
        payload: json!({"instruction":"original backend selection"}),
    })
    .unwrap();
    let other = db
        .claim_dispatch("other_runner", 1002, 1000, 2000, 2)
        .unwrap()
        .unwrap();
    assert_eq!(db.runner_service_engagement(&cap).unwrap(), first);
    assert_eq!(db.runner_service_engagement(&other).unwrap(), second.id);
    let before: String=sql(&root).query_row("SELECT json_group_array(json_array(id,state,fence,runner_id,lease_until,capability_until)) FROM runner_dispatches",[],|row|row.get(0)).unwrap();
    for field in ["secret", "runner", "dispatch", "fence"] {
        let mut foreign = cap.clone();
        match field {
            "secret" => foreign.secret = other.secret.clone(),
            "runner" => foreign.runner_id = other.runner_id.clone(),
            "dispatch" => foreign.dispatch_id = other.dispatch_id.clone(),
            _ => foreign.fence += 1,
        }
        assert!(matches!(
            db.runner_service_engagement(&foreign),
            Err(Error::RunnerAuthority)
        ));
    }
    // Expired credentials still name the historical backend, but do not
    // resurrect a current scope, extend leases, or mutate either attempt.
    assert!(db.owned_dispatch_scope(&cap, 4000).is_err());
    assert_eq!(db.runner_service_engagement(&cap).unwrap(), first);
    let after: String=sql(&root).query_row("SELECT json_group_array(json_array(id,state,fence,runner_id,lease_until,capability_until)) FROM runner_dispatches",[],|row|row.get(0)).unwrap();
    assert_eq!(before, after);
    let store = hagency_store::DomainStore::start(db, 8).unwrap();
    assert_eq!(store.runner_service_engagement(cap).await.unwrap(), first);
    assert_eq!(
        store.runner_service_engagement(other).await.unwrap(),
        second.id
    );
    store.shutdown().await.unwrap();
}

#[test]
fn native_owned_dispatch_start_scope() {
    let (root, mut db, _, cap) = setup();
    let before = db.owned_dispatch_scope(&cap, 1002).unwrap();
    assert_eq!(before.task().id, "task");
    assert_eq!(before.task().status, TaskState::Created);
    assert_eq!(before.input().resources[0].id, "workspace");
    assert_eq!(before.input().payload["cwd"], "/model/path"); // Data only.
    let mut edited = resource("pool", "seat", 1000);
    edited.model = "gpt-5.5".into();
    assert!(matches!(db.edit_resource(&edited, None), Err(Error::State)));
    // Deliberate stored-current-profile drift: the completed provision effect
    // remains the frozen execution profile, not this mutable catalog record.
    sql(&root)
        .execute(
            "UPDATE resources SET config=?1",
            [serde_json::to_string(&edited).unwrap()],
        )
        .unwrap();
    assert_eq!(
        db.owned_dispatch_scope(&cap, 1002)
            .unwrap()
            .resource()
            .model,
        "gpt-5.6-sol"
    );
    assert!(matches!(
        db.start_owned_dispatch(&cap, "wrong", 1003),
        Err(Error::RunnerAuthority)
    ));
    assert_eq!(
        db.canonical_task("task").unwrap().status,
        TaskState::Created
    );
    let fingerprint = before.fingerprint().to_owned();
    let started = db.start_owned_dispatch(&cap, &fingerprint, 1003).unwrap();
    assert_eq!(started.fingerprint(), fingerprint);
    assert_eq!(started.task().status, TaskState::InProgress);
    assert!(db.start_owned_dispatch(&cap, &fingerprint, 1004).is_err());
    assert!(db.check_owned_dispatch(&cap, "other_scope", 1004).is_err());
    assert_eq!(
        db.check_owned_dispatch(&cap, &fingerprint, 1004)
            .unwrap()
            .status,
        TaskState::InProgress
    );
    sql(&root)
        .execute(
            "UPDATE workspace_resources SET dirty=1 WHERE id='workspace'",
            [],
        )
        .unwrap();
    assert!(matches!(
        db.check_owned_dispatch(&cap, &fingerprint, 1005),
        Err(Error::Quarantined)
    ));
    assert_eq!(
        db.observe_owned_failure(&cap, OwnedFailure::CleanupUnknown, 1006)
            .unwrap(),
        OwnedObservation::Fenced
    );
    let held: u64 = sql(&root)
        .query_row("SELECT COUNT(*) FROM resource_leases", [], |r| r.get(0))
        .unwrap();
    assert_eq!(held, 1);
}

#[test]
fn native_owned_dispatch_negative_historical_fence() {
    let (root, mut db, engagement, cap) = setup();
    let digest = db
        .owned_dispatch_scope(&cap, 1002)
        .unwrap()
        .fingerprint()
        .to_owned();
    db.start_owned_dispatch(&cap, &digest, 1003).unwrap();
    db.revoke("revoke", &engagement).unwrap();
    assert_eq!(
        db.observe_owned_failure(&cap, OwnedFailure::LostAuthority, 4000)
            .unwrap(),
        OwnedObservation::Fenced
    );
    assert_eq!(
        db.canonical_task("task").unwrap().status,
        TaskState::InProgress
    );
    assert_eq!(
        sql(&root)
            .query_row("SELECT COUNT(*) FROM resource_leases", [], |r| r
                .get::<_, u64>(0))
            .unwrap(),
        1
    );
    let (root, mut db, _, old) = setup();
    db.reconcile_dispatches(2500).unwrap(); // Unstarted attempt expires/requeues.
    let replacement = db
        .claim_dispatch("replacement", 2501, 1000, 2000, 1)
        .unwrap()
        .unwrap();
    assert!(replacement.fence > old.fence);
    assert_eq!(
        db.observe_owned_failure(&old, OwnedFailure::StartUnknown, 2502)
            .unwrap(),
        OwnedObservation::Historical
    );
    let current = db.owned_dispatch_scope(&replacement, 2503).unwrap();
    db.start_owned_dispatch(&replacement, current.fingerprint(), 2503)
        .unwrap();
    assert_eq!(
        sql(&root)
            .query_row("SELECT COUNT(*) FROM resource_leases", [], |r| r
                .get::<_, u64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        sql(&root)
            .query_row(
                "SELECT dirty FROM workspace_resources WHERE id='workspace'",
                [],
                |r| r.get::<_, u64>(0)
            )
            .unwrap(),
        0
    );
    let forged = RunnerCapability {
        secret: "0".repeat(64),
        ..old
    };
    assert!(matches!(
        db.observe_owned_failure(&forged, OwnedFailure::Cancelled, 2504),
        Err(Error::RunnerAuthority)
    ));
}
