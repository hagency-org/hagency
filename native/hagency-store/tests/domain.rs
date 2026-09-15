mod common;
use common::*;
use hagency_core::{
    allocation::Tokens,
    project::{CleanupState, EngagementState, Seat},
};
use hagency_store::{DomainRepository, DomainStore, EffectOutcome, EffectState, Error};
use serde_json::json;

fn setup() -> (tempfile::TempDir, DomainRepository) {
    let dir = tempfile::tempdir().unwrap();
    let mut db = DomainRepository::open(&dir.path().join("state")).unwrap();
    db.register(&registration()).unwrap();
    (dir, db)
}
#[test]
fn domain_request_replay_and_publication() {
    let (_dir, mut db) = setup();
    let mut pool = resource("preset", "seat", 0);
    db.put_resource(&pool).unwrap();
    assert_eq!(db.catalog("", 100).unwrap().len(), 1); // Publication defaults on.
    let a = request("one", "E\u{301}dison", &pool, 100);
    let original = db.admit(&proof(&a), 1000).unwrap(); // Zero budget does not deny intake.
    assert_eq!(original.agent_name.as_str(), "Édison");
    assert_eq!(original.project_name.as_deref(), Some("实际项目名称"));
    assert_eq!(
        db.resource_budget(&pool.id()).unwrap().pool.committed,
        Tokens::default()
    );
    assert_eq!(value(db.admit(&proof(&a), 1001).unwrap()), value(&original));
    let mut changed = a.clone();
    changed.requested_tokens = 101.try_into().unwrap();
    assert!(matches!(
        db.admit(&proof(&changed), 1001),
        Err(Error::Conflict)
    ));
    let mut duplicate = request("two", "Édison", &pool, 100);
    assert!(matches!(
        db.admit(&proof(&duplicate), 1000),
        Err(Error::Conflict)
    ));
    duplicate.target_project_id = "another_project".into();
    duplicate.target_room_id = "!another:example.test".into();
    let other = db.admit(&proof(&duplicate), 1000).unwrap();
    assert_ne!(other.runtime_name, original.runtime_name);
    pool.published = false;
    db.put_resource(&pool).unwrap();
    assert!(db.catalog("", 100).unwrap().is_empty());
    assert_eq!(value(db.admit(&proof(&a), 1000).unwrap()), value(&original));
    let novel = request("three", "小白", &pool, 100);
    assert!(matches!(
        db.admit(&proof(&novel), 1000),
        Err(Error::Unqualified)
    ));
    assert!(matches!(
        db.approve("approve", &proof(&a), 1000),
        Err(Error::Unqualified)
    ));
    pool.published = true;
    pool.reasoning = Some("low".into());
    db.put_resource(&pool).unwrap();
    assert!(matches!(
        db.approve("approve", &proof(&a), 1000),
        Err(Error::Unqualified)
    ));
    db.reject("reject", &original.id).unwrap();
    pool.reasoning = Some("medium".into());
    db.put_resource(&pool).unwrap();
    db.admit(&proof(&request("replacement", "Édison", &pool, 1)), 1000)
        .unwrap();
    let projection = serde_json::to_string(&db.engagements("", 100).unwrap()).unwrap();
    for private in [
        "ownerDmRoomId",
        "!private:",
        "presetId",
        "seatId",
        "@owner:",
    ] {
        assert!(!projection.contains(private));
    }
}

#[tokio::test]
async fn domain_reservations_are_atomic() {
    let (dir, mut db) = setup();
    let pool = resource("preset_a", "shared", 100);
    let other = resource("preset_b", "shared", 100);
    db.put_resource(&pool).unwrap();
    db.put_resource(&other).unwrap();
    let seat: Seat = serde_json::from_value(
        json!({"id":"shared","declaration":{"quotaTokens":150,"period":"monthly"}}),
    )
    .unwrap();
    db.put_seat(&seat).unwrap();
    let a = request("one", "小白", &pool, 60);
    let b = request("two", "edison", &pool, 60);
    let c = request("three", "other", &other, 100);
    for request in [&a, &b, &c] {
        db.admit(&proof(request), 1000).unwrap();
    }
    // Trigger failure after the engagement was mutated but before the outbox intent exists.
    let inspection = rusqlite::Connection::open(dir.path().join("state/domain.sqlite3")).unwrap();
    inspection.execute_batch("CREATE TRIGGER fail_outbox BEFORE INSERT ON effects BEGIN SELECT RAISE(ABORT,'fixture disk failure'); END;").unwrap();
    assert!(db.approve("fail", &proof(&a), 1000).is_err());
    assert_eq!(
        db.get(&a.engagement_id().unwrap()).unwrap().state,
        EngagementState::Pending
    );
    assert_eq!(
        u64::from(db.resource_budget(&pool.id()).unwrap().pool.committed),
        0
    );
    assert_eq!(
        inspection
            .query_row("SELECT COUNT(*) FROM decisions", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    inspection
        .execute_batch("DROP TRIGGER fail_outbox")
        .unwrap();
    let store = DomainStore::start(db, 16).unwrap();
    // Two asynchronous approvals race through the actual single transactional writer.
    let (first, second) = tokio::join!(
        store.approve("approve_a".into(), proof(&a), 1000),
        store.approve("approve_b".into(), proof(&b), 1000)
    );
    assert_eq!(usize::from(first.is_ok()) + usize::from(second.is_ok()), 1);
    let (winner, command, request) = if let Ok(a_result) = first {
        // The pool ceiling is declared, so the loser is an over-commit with the
        // binding-draw wording, not an unknown-capacity refusal.
        assert!(matches!(second, Err(Error::OverCommit { .. })));
        (a_result, "approve_a", &a)
    } else {
        (second.unwrap(), "approve_b", &b)
    };
    assert_eq!(
        value(
            store
                .approve(command.into(), proof(request), 1000)
                .await
                .unwrap()
        ),
        value(&winner)
    );
    let budget = store.resource_budget(pool.id()).await.unwrap();
    assert_eq!(u64::from(budget.pool.committed), 60);
    assert_eq!(u64::from(budget.remaining_tokens.unwrap()), 40);
    // Other pool still has 100 but its shared seat has only 90: the seat is
    // the binding side, which is the resource-pool refusal this identity
    // predates the ceiling split for and still carries.
    assert!(matches!(
        store.approve("approve_c".into(), proof(&c), 1000).await,
        Err(Error::InsufficientCapacity)
    ));
    store
        .put_seat(Seat {
            id: "shared".into(),
            declaration: None,
        })
        .await
        .unwrap();
    store
        .approve("approve_c".into(), proof(&c), 1000)
        .await
        .unwrap(); // No invented seat quota.
    let budget = store.resource_budget(pool.id()).await.unwrap();
    assert_eq!(u64::from(budget.remaining_tokens.unwrap()), 40);
    assert_eq!(u64::from(budget.seat.committed), 160);
    assert!(budget.seat.remaining.is_none());
    store
        .revoke("revoke".into(), winner.id.clone())
        .await
        .unwrap();
    assert_eq!(
        u64::from(
            store
                .resource_budget(other.id())
                .await
                .unwrap()
                .pool
                .committed
        ),
        100
    );
    store.shutdown().await.unwrap();
    let reopened = DomainRepository::open(&dir.path().join("state")).unwrap();
    assert_eq!(
        reopened.get(&winner.id).unwrap().state,
        EngagementState::Revoked
    );
    assert_eq!(
        reopened.get(&c.engagement_id().unwrap()).unwrap().state,
        EngagementState::Reserved
    );
}

#[test]
fn domain_effect_recovery_and_revocation() {
    let (dir, mut db) = setup();
    let pool = resource("preset", "seat", 100);
    db.put_resource(&pool).unwrap();
    let request = request("one", "小白", &pool, 50);
    let proof = proof(&request);
    let engagement = db.admit(&proof, 1000).unwrap();
    db.approve("approve", &proof, 1000).unwrap();
    let effect = db.claim_effect().unwrap().unwrap();
    assert_eq!(effect.state, EffectState::Started);
    assert_eq!(effect.payload["runtimeName"], engagement.runtime_name);
    assert!(db.claim_effect().unwrap().is_none());
    drop(db);
    let mut db = DomainRepository::open(&dir.path().join("state")).unwrap();
    assert_eq!(db.effect(&effect.id).unwrap().state, EffectState::Uncertain);
    assert!(db.claim_effect().unwrap().is_none());
    assert_eq!(
        u64::from(db.resource_budget(&pool.id()).unwrap().pool.committed),
        50
    );
    assert!(matches!(
        db.observe_effect(
            &effect.id,
            effect.fence + 1,
            &EffectOutcome::Applied {
                receipt: "observed_identity".into()
            }
        ),
        Err(Error::Generation)
    ));
    db.observe_effect(&effect.id, effect.fence, &EffectOutcome::Unknown)
        .unwrap();
    let observed = EffectOutcome::Applied {
        receipt: "observed_identity".into(),
    };
    assert_eq!(
        db.observe_effect(&effect.id, effect.fence, &observed)
            .unwrap()
            .state,
        EngagementState::Active
    );
    assert_eq!(
        db.observe_effect(&effect.id, effect.fence, &observed)
            .unwrap()
            .state,
        EngagementState::Active
    );
    let revoked = db.revoke("revoke", &engagement.id).unwrap();
    assert_eq!(revoked.cleanup, CleanupState::Pending);
    assert_eq!(
        u64::from(db.resource_budget(&pool.id()).unwrap().pool.committed),
        0
    );
    assert!(matches!(
        db.observe_effect(&effect.id, effect.fence, &observed),
        Err(Error::Generation)
    ));
    let cleanup = db.claim_effect().unwrap().unwrap();
    assert_eq!(cleanup.kind, "retire");
    drop(db);
    let mut db = DomainRepository::open(&dir.path().join("state")).unwrap();
    assert_eq!(
        db.get(&engagement.id).unwrap().cleanup,
        CleanupState::Uncertain
    );
    assert!(db.claim_effect().unwrap().is_none());
    assert_eq!(
        db.observe_effect(&cleanup.id, cleanup.fence, &EffectOutcome::Unknown)
            .unwrap()
            .cleanup,
        CleanupState::Uncertain
    );
    assert!(db.claim_effect().unwrap().is_none());
    assert!(matches!(
        db.retry_cleanup("premature_retry", &engagement.id),
        Err(Error::State)
    ));
    db.observe_effect(
        &cleanup.id,
        cleanup.fence,
        &EffectOutcome::NotApplied {
            receipt: "confirmed_no_retirement_applied".into(),
        },
    )
    .unwrap();
    assert!(db.claim_effect().unwrap().is_none());
    db.retry_cleanup("retry_cleanup", &engagement.id).unwrap();
    let retried = db.claim_effect().unwrap().unwrap();
    assert!(retried.fence > cleanup.fence);
    assert!(matches!(
        db.observe_effect(&cleanup.id, cleanup.fence, &EffectOutcome::Unknown),
        Err(Error::Generation)
    ));
    let completed = db
        .observe_effect(
            &cleanup.id,
            retried.fence,
            &EffectOutcome::Applied {
                receipt: "observed_deactivated_without_erasing_history".into(),
            },
        )
        .unwrap();
    assert_eq!(completed.cleanup, CleanupState::Complete);
    assert_eq!(db.engagements("", 100).unwrap().len(), 1);
    assert_eq!(
        value(db.revoke("revoke", &engagement.id).unwrap()),
        value(revoked)
    );
    assert_eq!(
        db.get(&engagement.id).unwrap().cleanup,
        CleanupState::Complete
    );
}

#[test]
fn native_ceiling_no_ceiling_distinct_from_over_commit() {
    let (_dir, mut db) = setup();
    // Unknown capacity is not an over-commit: a declared ceiling whose seat
    // declaration mismatches the pool period leaves remaining unknown, which
    // the retained JavaScript refuses as no_ceiling.
    let pool = resource("preset_no_ceiling", "mismatched_seat", 100);
    db.put_resource(&pool).unwrap();
    let seat: Seat = serde_json::from_value(
        json!({"id":"mismatched_seat","declaration":{"quotaTokens":50,"period":"daily"}}),
    )
    .unwrap();
    db.put_seat(&seat).unwrap();
    let a = request("no_ceiling", "NoCeilingWorker", &pool, 10);
    db.admit(&proof(&a), 1000).unwrap();
    match db.approve("approve_no_ceiling", &proof(&a), 1000) {
        Err(Error::NoCeiling) => {}
        Err(Error::OverCommit { message }) => {
            panic!("unknown capacity must not carry over-commit wording: {message}")
        }
        Err(Error::InsufficientCapacity) => {
            panic!("unknown capacity must be named, not the pool-refusal variant")
        }
        other => panic!("expected NoCeiling, got {other:?}"),
    }
    // The mirror on the same store: a known ceiling the allocation exceeds
    // refuses as over_commit and names the draw, the preset and the period.
    let pool = resource("preset_over_commit", "aligned_seat", 100);
    db.put_resource(&pool).unwrap();
    let b = request("over_commit", "OverCommitWorker", &pool, 500);
    db.admit(&proof(&b), 1000).unwrap();
    match db.approve("approve_over_commit", &proof(&b), 1000) {
        Err(Error::NoCeiling) => panic!("a declared ceiling must not read as unknown"),
        Err(Error::OverCommit { message }) => {
            assert!(message.contains("would exceed the 100 left on OverCommitWorker"));
            assert!(message.contains("its ceiling is 100 per monthly"));
            assert!(message.contains("nothing has been measured"));
            assert!(message.contains("preset_over_commit"));
        }
        Err(Error::InsufficientCapacity) => {
            panic!("a ceiling exceeded by the allocation must be named, not pooled")
        }
        other => panic!("expected OverCommit, got {other:?}"),
    }
}
