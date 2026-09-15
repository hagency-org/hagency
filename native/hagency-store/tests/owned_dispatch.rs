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
