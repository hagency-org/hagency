mod common;
use common::*;
use hagency_core::{authority::Registration, project::Resource};
use hagency_store::*;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
fn provision(db: &mut DomainRepository, pool: &Resource) -> Effect {
    db.register(&registration()).unwrap();
    db.put_resource(pool).unwrap();
    let approved = proof(&request("warm", "Worker", pool, 100));
    db.admit(&approved, 1000).unwrap();
    db.approve("approve", &approved, 1000).unwrap();
    db.claim_effect().unwrap().unwrap()
}
fn managed(db: &mut DomainRepository) -> (ManagedAccount, Resource) {
    let choice = db.reserve_account(ACCOUNT_PROFILE).unwrap();
    let choice = db.materialize_account(&choice.id).unwrap();
    let account = db.managed_account(&choice.id).unwrap();
    let access =
        AccountEnrollmentAccess::new(Instant::now() + Duration::from_secs(30), Default::default());
    let command = access
        .prepare(
            &account,
            choice.revision,
            "gpt-5.6-sol".into(),
            Some("medium".into()),
            Some(
                serde_json::from_value(serde_json::json!({"tokens":1000,"period":"monthly"}))
                    .unwrap(),
            ),
            Instant::now() + Duration::from_secs(5),
        )
        .unwrap();
    let result = db.enroll_account_resource(command).unwrap();
    let pool = db.resource_configuration(&result.resource_id).unwrap();
    (account, pool)
}
fn login(db: &mut DomainRepository, account: &ManagedAccount, outcome: LoginOutcome, expires: u64) {
    let clock = now();
    let attempt = db.begin_account_login(account.id(), clock).unwrap();
    db.settle_account_login(
        attempt,
        LoginVerdict {
            mode: AccountReadinessMode::Subscription,
            provider_state: "logged-in-subscription".into(),
            outcome,
            expires_at_ms: Some(expires),
        },
        clock,
    )
    .unwrap();
}
#[test]
fn native_provisioning_original_activation_scope() {
    // Writer admission/kernel evidence ONLY, not physical factory proof. The
    // native integration selectors must initialize real original owners first.
    for case in [
        "original",
        "unclaimed",
        "unknown",
        "revoked",
        "registration",
        "resource",
        "foreign",
        "managed-refused",
    ] {
        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("state");
        let mut db = DomainRepository::open(&state).unwrap();
        let (account, pool) = if case == "managed-refused" {
            let (account, pool) = managed(&mut db);
            login(&mut db, &account, LoginOutcome::Observed, now() + 60_000);
            (Some(account), pool)
        } else {
            (None, resource("pool", "seat", 1000))
        };
        let effect = provision(&mut db, &pool);
        let scope = db
            .provision_runtime_scope(&effect, &registration())
            .unwrap();
        let original = db.provision_runtime_account(&scope).unwrap();
        assert_eq!(
            original.as_ref().map(ManagedAccount::id),
            account.as_ref().map(ManagedAccount::id)
        );
        if case != "unclaimed" {
            scope.claim_warm().unwrap();
        }
        match case {
            "unknown" => {
                db.observe_effect(&effect.id, effect.fence, &EffectOutcome::Unknown)
                    .unwrap();
            }
            "revoked" => {
                db.revoke("revoke", &effect.engagement_id).unwrap();
            }
            "registration" => {
                db.register(&Registration {
                    generation: 2,
                    ..registration()
                })
                .unwrap();
            }
            "resource" => {
                rusqlite::Connection::open(state.join("domain.sqlite3")).unwrap().execute("UPDATE resources SET config=json_set(config,'$.model','gpt-5.6-terra') WHERE id=?1",[pool.id()]).unwrap();
            }
            "managed-refused" => {
                std::thread::sleep(Duration::from_millis(2));
                login(
                    &mut db,
                    account.as_ref().unwrap(),
                    LoginOutcome::Refused,
                    now() + 60_000,
                );
            }
            "foreign" => {
                let other = tempfile::tempdir().unwrap();
                let mut foreign = DomainRepository::open(&other.path().join("state")).unwrap();
                provision(&mut foreign, &pool);
                assert!(matches!(
                    foreign.complete_original_provision(&scope),
                    Err(Error::RunnerAuthority)
                ));
                assert!(foreign.provision_runtime_account(&scope).is_err());
                continue;
            }
            _ => {}
        }
        let result = db.complete_original_provision(&scope);
        if case == "original" {
            assert_eq!(
                result.unwrap().state,
                hagency_core::project::EngagementState::Active
            );
            assert!(
                db.complete_original_provision(&scope).is_err(),
                "no activation acknowledgment reconstruction/replay"
            );
            let receipt = format!(
                "inline_factory_{}",
                hagency_core::canonical::transport_digest(&serde_json::json!([
                    effect,
                    registration()
                ]))
                .unwrap()
            );
            assert_eq!(
                db.observe_effect(
                    &effect.id,
                    effect.fence,
                    &EffectOutcome::Applied { receipt }
                )
                .unwrap()
                .state,
                hagency_core::project::EngagementState::Active
            );
        } else {
            assert!(
                result.is_err(),
                "stale original activation accepted: {case}"
            );
            assert_ne!(
                db.get(&effect.engagement_id).unwrap().state,
                hagency_core::project::EngagementState::Active
            );
        }
    }
}
#[test]
fn native_warm_runtime_writer_scope() {
    let root = tempfile::tempdir().unwrap();
    let mut db = DomainRepository::open(&root.path().join("state")).unwrap();
    let pool = resource("pool", "seat", 1000);
    let effect = provision(&mut db, &pool);
    let scope = db
        .provision_runtime_scope(&effect, &registration())
        .unwrap();
    assert_eq!(scope.engagement_id(), effect.engagement_id);
    assert_eq!(
        value(scope.resource()),
        value(db.resource_configuration(&pool.id()).unwrap())
    );
    assert!(!scope.requires_managed_account());
    db.validate_warm_runtime_scope(&scope).unwrap();
    scope.claim_warm().unwrap();
    assert!(matches!(scope.clone().claim_warm(), Err(Error::Busy)));
    assert!(matches!(
        db.provision_runtime_scope(&effect, &registration())
            .unwrap()
            .claim_warm(),
        Err(Error::Busy)
    ));
    let sql = rusqlite::Connection::open(root.path().join("state/domain.sqlite3")).unwrap();
    assert_eq!(
        sql.query_row("SELECT state FROM effects WHERE id=?1", [&effect.id], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "started"
    );
    assert_eq!(
        sql.query_row("SELECT COUNT(*) FROM canonical_tasks", [], |r| r
            .get::<_, u64>(0))
            .unwrap(),
        0
    );
    db.observe_effect(
        &effect.id,
        effect.fence,
        &EffectOutcome::Applied {
            receipt: "offline fixture activation, not factory proof".into(),
        },
    )
    .unwrap();
    db.validate_warm_runtime_scope(&scope).unwrap();
    assert!(
        db.provision_runtime_scope(&effect, &registration())
            .is_err()
    );
}
#[test]
fn native_warm_runtime_scope_refusals() {
    for case in [
        "fence",
        "payload",
        "registration",
        "resource",
        "unknown",
        "revoked",
        "foreign",
        "reopen",
    ] {
        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("state");
        let mut db = DomainRepository::open(&state).unwrap();
        let mut pool = resource("pool", "seat", 1000);
        let effect = provision(&mut db, &pool);
        let scope = db
            .provision_runtime_scope(&effect, &registration())
            .unwrap();
        match case {
            "fence" => {
                let mut changed = effect.clone();
                changed.fence += 1;
                assert!(
                    db.provision_runtime_scope(&changed, &registration())
                        .is_err()
                );
            }
            "payload" => {
                let mut changed = effect.clone();
                changed.payload["resource"]["model"] = "foreign".into();
                assert!(
                    db.provision_runtime_scope(&changed, &registration())
                        .is_err()
                );
            }
            "registration" => {
                let changed = Registration {
                    generation: 2,
                    ..registration()
                };
                db.register(&changed).unwrap();
                assert!(db.validate_warm_runtime_scope(&scope).is_err());
            }
            "resource" => {
                pool.model = "gpt-5.6-terra".into();
                assert!(matches!(db.put_resource(&pool), Err(Error::State)));
                db.validate_warm_runtime_scope(&scope).unwrap();
                // Valid-shaped out-of-band fixture corruption, not a permitted
                // public profile mutation or a production authority write.
                let sql = rusqlite::Connection::open(state.join("domain.sqlite3")).unwrap();
                sql.execute("UPDATE resources SET config=json_set(config,'$.model','gpt-5.6-terra') WHERE id=?1",[pool.id()]).unwrap();
                assert!(db.validate_warm_runtime_scope(&scope).is_err());
            }
            "unknown" => {
                db.observe_effect(&effect.id, effect.fence, &EffectOutcome::Unknown)
                    .unwrap();
                assert!(db.validate_warm_runtime_scope(&scope).is_err());
            }
            "revoked" => {
                db.revoke("revoke", &effect.engagement_id).unwrap();
                assert!(db.validate_warm_runtime_scope(&scope).is_err());
            }
            "foreign" => {
                let other = tempfile::tempdir().unwrap();
                let mut foreign = DomainRepository::open(&other.path().join("state")).unwrap();
                let same = provision(&mut foreign, &pool);
                assert_eq!(value(same), value(&effect));
                assert!(matches!(
                    foreign.validate_warm_runtime_scope(&scope),
                    Err(Error::RunnerAuthority)
                ));
            }
            "reopen" => {
                drop(db);
                let mut reopened = DomainRepository::open(&state).unwrap();
                assert!(reopened.validate_warm_runtime_scope(&scope).is_err());
                assert!(
                    reopened
                        .provision_runtime_scope(&effect, &registration())
                        .is_err()
                );
            }
            _ => unreachable!(),
        }
    }
}
#[test]
fn native_warm_runtime_managed_readiness() {
    let root = tempfile::tempdir().unwrap();
    let mut db = DomainRepository::open(&root.path().join("state")).unwrap();
    let (account, pool) = managed(&mut db);
    let effect = provision(&mut db, &pool);
    assert!(
        db.provision_runtime_scope(&effect, &registration())
            .is_err()
    );
    login(&mut db, &account, LoginOutcome::Observed, now() + 60_000);
    let scope = db
        .provision_runtime_scope(&effect, &registration())
        .unwrap();
    assert!(scope.requires_managed_account());
    account.prepare_provision_launch(&scope).unwrap();
    db.validate_warm_runtime_scope(&scope).unwrap();
    let (other, _) = managed(&mut db);
    assert!(other.prepare_provision_launch(&scope).is_err());
    // Actual newer refused receipt shadows the usable older observation.
    std::thread::sleep(Duration::from_millis(2));
    login(&mut db, &account, LoginOutcome::Refused, now() + 60_000);
    assert!(db.validate_warm_runtime_scope(&scope).is_err());
    std::thread::sleep(Duration::from_millis(2));
    login(&mut db, &account, LoginOutcome::Observed, now() + 30);
    db.validate_warm_runtime_scope(&scope).unwrap();
    std::thread::sleep(Duration::from_millis(40));
    assert!(db.validate_warm_runtime_scope(&scope).is_err());
    login(&mut db, &account, LoginOutcome::Observed, now() + 60_000);
    account.retire();
    assert!(db.validate_warm_runtime_scope(&scope).is_err());
}
