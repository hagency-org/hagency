mod common;
use hagency_store::*;
use std::time::{Duration, Instant};
fn prepared(db: &mut DomainRepository) -> AccountChoice {
    let original = db.reserve_account(ACCOUNT_PROFILE).unwrap();
    db.materialize_account(&original.id).unwrap()
}
fn enroll(db: &mut DomainRepository, choice: &AccountChoice) -> hagency_core::project::Resource {
    let account = db.managed_account(&choice.id).unwrap();
    let access =
        AccountEnrollmentAccess::new(Instant::now() + Duration::from_secs(30), Default::default());
    let command = access
        .prepare(
            &account,
            choice.revision.clone(),
            "gpt-5.6-sol".into(),
            Some("medium".into()),
            None,
            Instant::now() + Duration::from_secs(5),
        )
        .unwrap();
    let result = db.enroll_account_resource(command).unwrap();
    db.resource_configuration(&result.resource_id).unwrap()
}
#[test]
fn native_account_preparation() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    let mut db = DomainRepository::open(&state).unwrap();
    assert!(db.reserve_account("subscription-only").is_err());
    let first = db.reserve_account(ACCOUNT_PROFILE).unwrap();
    let collision = state.join(&first.id);
    private::create_directory_new(&collision).unwrap();
    std::fs::write(collision.join("original"), b"retain original").unwrap();
    assert!(db.materialize_account(&first.id).is_err());
    assert!(db.materialize_account(&first.id).is_err());
    assert_eq!(
        std::fs::read(collision.join("original")).unwrap(),
        b"retain original"
    );
    assert!(matches!(
        db.account_choices().unwrap()[0].state,
        AccountState::Uncertain
    ));
    let lost = db.reserve_account(ACCOUNT_PROFILE).unwrap();
    let actual = prepared(&mut db);
    assert!(matches!(actual.state, AccountState::Active));
    assert_eq!(actual.authentication, "unknown");
    assert_eq!(actual.quota, None);
    let handle = db.managed_account(&actual.id).unwrap();
    assert!(matches!(DomainRepository::open(&state), Err(Error::Locked)));
    drop(db);
    let mut db = DomainRepository::open(&state).unwrap();
    assert!(db.materialize_account(&lost.id).is_err());
    assert!(matches!(
        db.account_choices().unwrap()[1].state,
        AccountState::Uncertain
    ));
    let access =
        AccountEnrollmentAccess::new(Instant::now() + Duration::from_secs(30), Default::default());
    let command = access
        .prepare(
            &handle,
            actual.revision,
            "gpt-5.6-sol".into(),
            Some("medium".into()),
            None,
            Instant::now() + Duration::from_secs(5),
        )
        .unwrap();
    assert!(matches!(
        db.enroll_account_resource(command),
        Err(Error::LocalAuthority)
    ));
    let sql = rusqlite::Connection::open(state.join("domain.sqlite3")).unwrap();
    sql.execute("DELETE FROM account_identity_key", []).unwrap();
    assert!(db.managed_account(&actual.id).is_err());
    drop(db);
    assert!(DomainRepository::open(&state).is_err());
}
#[test]
fn native_account_association() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    let mut db = DomainRepository::open(&state).unwrap();
    let choice = prepared(&mut db);
    let first = enroll(&mut db, &choice);
    let access = ResourceConfigurationAccess::new(
        Instant::now() + Duration::from_secs(30),
        Default::default(),
    );
    let command = access
        .prepare(
            first.id(),
            resource_publication_revision(&first).unwrap(),
            true,
            ProfileChange::Preserve {},
            CeilingChange::Preserve {},
            Instant::now() + Duration::from_secs(5),
        )
        .unwrap();
    let result = db.configure_resource(command).unwrap();
    let second = db.resource_configuration(&result.resource_id).unwrap();
    assert_ne!(first.id(), second.id());
    assert_eq!(first.seat_id, second.seat_id);
    let sql = rusqlite::Connection::open(state.join("domain.sqlite3")).unwrap();
    let mappings: u64 = sql
        .query_row(
            "SELECT COUNT(*) FROM resource_accounts WHERE account_id=?1",
            [&choice.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(mappings, 2);
    let seat: String = sql
        .query_row(
            "SELECT config FROM seats WHERE id=?1",
            [&first.seat_id],
            |r| r.get(0),
        )
        .unwrap();
    assert!(serde_json::from_str::<serde_json::Value>(&seat).unwrap()["declaration"].is_null());
    let mut forged = first.clone();
    forged.preset_id = "forged_preset".into();
    assert!(db.put_resource(&forged).is_err());
    let mut rebound = first.clone();
    rebound.seat_id = "other-seat".into();
    assert!(db.put_resource(&rebound).is_err());
    assert!(
        db.put_seat(&hagency_core::project::Seat {
            id: first.seat_id.clone(),
            declaration: None
        })
        .is_err()
    );
    let command = access
        .prepare(
            second.id(),
            result.revision,
            false,
            ProfileChange::Preserve {},
            CeilingChange::Monthly {
                tokens: 123u64.try_into().unwrap(),
            },
            Instant::now() + Duration::from_secs(5),
        )
        .unwrap();
    db.configure_resource(command).unwrap();
    assert_eq!(
        db.resource_configuration(&second.id()).unwrap().seat_id,
        first.seat_id
    );
    assert!(
        sql.execute("UPDATE resource_accounts SET account_id='forged'", [])
            .is_err()
    );
    let before: usize = sql
        .query_row("SELECT COUNT(*) FROM resources", [], |r| r.get(0))
        .unwrap();
    let account = db.managed_account(&choice.id).unwrap();
    let enroll =
        AccountEnrollmentAccess::new(Instant::now() + Duration::from_secs(30), Default::default());
    let cmd = enroll
        .prepare(
            &account,
            choice.revision,
            "unsupported".into(),
            None,
            None,
            Instant::now() + Duration::from_secs(5),
        )
        .unwrap();
    assert!(db.enroll_account_resource(cmd).is_err());
    assert_eq!(
        sql.query_row("SELECT COUNT(*) FROM resources", [], |r| r
            .get::<_, usize>(0))
            .unwrap(),
        before
    );
}
#[test]
fn native_account_retirement() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    let mut db = DomainRepository::open(&state).unwrap();
    let choice = prepared(&mut db);
    let mut resource = enroll(&mut db, &choice);
    resource.ceiling = Some(
        serde_json::from_value(serde_json::json!({"tokens":1000,"period":"monthly"})).unwrap(),
    );
    db.put_resource(&resource).unwrap();
    db.register(&common::registration()).unwrap();
    let proof = common::proof(&common::request(
        "account-commitment",
        "Worker",
        &resource,
        100,
    ));
    db.admit(&proof, 1000).unwrap();
    db.approve("approval", &proof, 1000).unwrap();
    let sql = rusqlite::Connection::open(state.join("domain.sqlite3")).unwrap();
    let before: String = sql
        .query_row("SELECT json_group_array(config) FROM resources", [], |r| {
            r.get(0)
        })
        .unwrap();
    let obligations: Vec<(String, String)> = sql
        .prepare("SELECT id,context FROM engagements")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let account = db.managed_account(&choice.id).unwrap();
    assert!(!db.catalog("", 100).unwrap().is_empty());
    account.retire();
    assert!(db.catalog("", 100).unwrap().is_empty());
    let access = ResourcePublicationAccess::new(
        Instant::now() + Duration::from_secs(30),
        Default::default(),
    );
    let cmd = access
        .prepare(
            resource.id(),
            resource_publication_revision(&resource).unwrap(),
            true,
            Instant::now() + Duration::from_secs(5),
        )
        .unwrap();
    assert!(db.publish_resource(cmd).is_err());
    db.retire_account(&choice.id, LogoutObservation::unobserved())
        .unwrap();
    assert!(!db.resource_configuration(&resource.id()).unwrap().published);
    assert!(db.managed_account(&choice.id).is_err());
    assert_ne!(
        sql.query_row("SELECT json_group_array(config) FROM resources", [], |r| {
            r.get::<_, String>(0)
        })
        .unwrap(),
        before
    );
    let after: Vec<(String, String)> = sql
        .prepare("SELECT id,context FROM engagements")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(after, obligations);
    drop(db);
    let db = DomainRepository::open(&state).unwrap();
    assert!(matches!(
        db.account_choices().unwrap()[0].state,
        AccountState::Retired
    ));
}
/// The DomainStore wrappers mirror the repository lifecycle (MA-S3a store half):
/// reserve through the writer queue lands 'preparing', materialize lands
/// 'active', and retire lands 'retired' while unpublishing bound resources.
#[tokio::test]
async fn native_console_account_wrappers_mirror_the_store() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    let mut db = DomainRepository::open(&state).unwrap();
    let seed_choice = prepared(&mut db);
    let seed_resource = enroll(&mut db, &seed_choice);
    let store = DomainStore::start(db, 16).unwrap();
    let reserved = store
        .reserve_account(ACCOUNT_PROFILE.to_owned())
        .await
        .unwrap();
    assert!(matches!(reserved.state, AccountState::Preparing));
    assert!(matches!(
        store.account_choices().await.unwrap()[0].state,
        AccountState::Active
    ));
    let materialized = store
        .materialize_account(reserved.id.clone())
        .await
        .unwrap();
    assert!(matches!(materialized.state, AccountState::Active));
    // Materialize returns the SAME account id it was given: the id is stable
    // across the preparing -> active transition (the wrapper does not mint a
    // second reservation). Only the revision moves.
    assert_eq!(materialized.id, reserved.id);
    // The revision digests {version,id,state,ordinal,profile}, so the state
    // transition preparing -> active must move it.
    assert_ne!(materialized.revision, reserved.revision);
    let retired = store.retire_account(seed_choice.id.clone()).await.unwrap();
    assert!(matches!(retired.state, AccountState::Retired));
    assert!(store.managed_account(seed_choice.id).await.is_err());
    assert!(
        !store
            .resource_configuration(seed_resource.id())
            .await
            .unwrap()
            .published
    );
    assert!(
        store
            .reserve_account("subscription-only".to_owned())
            .await
            .is_err()
    );
    store.shutdown().await.unwrap();
}

// ---- MA-S1: the observed provider-login readiness fact (migration 028).

/// Settle one observed login through the store's own receipt path.
fn login_observed(
    db: &mut DomainRepository,
    id: &str,
    mode: AccountReadinessMode,
    expires_at_ms: Option<u64>,
    now: u64,
) -> AccountReadiness {
    let attempt = db.begin_account_login(id, now).unwrap();
    db.settle_account_login(
        attempt,
        LoginVerdict {
            mode,
            provider_state: match mode {
                AccountReadinessMode::Subscription => "logged-in-subscription".into(),
                AccountReadinessMode::ApiKey => "logged-in-api-key".into(),
                AccountReadinessMode::Unknown => "not-logged-in".into(),
            },
            outcome: LoginOutcome::Observed,
            expires_at_ms,
        },
        now + 100,
    )
    .unwrap()
}

#[test]
fn native_account_login_readiness() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    let mut db = DomainRepository::open(&state).unwrap();
    let choice = prepared(&mut db);
    // No observation: the answer is unknown — never a filesystem-derived
    // guess (D1: directory existence establishes nothing).
    assert_eq!(
        db.account_readiness(&choice.id, 2000).unwrap().mode,
        AccountReadinessMode::Unknown
    );
    // The only writer of a readiness fact is a settled login receipt.
    let settled = login_observed(
        &mut db,
        &choice.id,
        AccountReadinessMode::Subscription,
        Some(9000),
        2100,
    );
    assert_eq!(settled.mode, AccountReadinessMode::Subscription);
    let after = db.account_readiness(&choice.id, 2300).unwrap();
    assert_eq!(after.mode, AccountReadinessMode::Subscription);
    assert_eq!(after.observed_at_ms, 2200);
    assert_eq!(after.expires_at_ms, 9000);
    // A refused login is a fact too — but not a usable one.
    let attempt = db.begin_account_login(&choice.id, 2400).unwrap();
    db.settle_account_login(
        attempt,
        LoginVerdict {
            mode: AccountReadinessMode::Unknown,
            provider_state: "plan-refused".into(),
            outcome: LoginOutcome::Refused,
            expires_at_ms: None,
        },
        2500,
    )
    .unwrap();
    assert_eq!(
        db.account_readiness(&choice.id, 2600).unwrap().mode,
        AccountReadinessMode::Unknown,
        "a refused login never reads as ready"
    );
}

#[test]
fn native_account_login_interrupted_is_uncertain() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    let mut db = DomainRepository::open(&state).unwrap();
    let choice = prepared(&mut db);
    // The attempt is allocated and committed; the process dies before the
    // child settles (the SQLite-before-effect ordering materialise uses).
    let _ = db.begin_account_login(&choice.id, 2100).unwrap();
    drop(db);
    let mut db = DomainRepository::open(&state).unwrap();
    // Reconciliation settled the attempt as `uncertain`...
    let sql = rusqlite::Connection::open(state.join("domain.sqlite3")).unwrap();
    let (attempt_state, outcome): (String, String) = sql
        .query_row(
            "SELECT a.state,o.outcome FROM account_login_attempts a \
             JOIN account_login_observations o ON o.id=a.receipt_id WHERE a.account_id=?1",
            [&choice.id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        (attempt_state.as_str(), outcome.as_str()),
        ("settled", "uncertain")
    );
    // ... and `uncertain` is never usable: the read answers unknown.
    assert_eq!(
        db.account_readiness(&choice.id, 2200).unwrap().mode,
        AccountReadinessMode::Unknown
    );
    // A later read never promotes it; only a new login receipt can.
    assert_eq!(
        db.account_readiness(&choice.id, 2300).unwrap().mode,
        AccountReadinessMode::Unknown
    );
    // The interrupted attempt does not block the next login.
    let settled = login_observed(
        &mut db,
        &choice.id,
        AccountReadinessMode::ApiKey,
        None,
        2400,
    );
    assert_eq!(settled.mode, AccountReadinessMode::ApiKey);
}

#[test]
fn native_account_readiness_expires_to_unknown() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    let mut db = DomainRepository::open(&state).unwrap();
    let choice = prepared(&mut db);
    login_observed(
        &mut db,
        &choice.id,
        AccountReadinessMode::ApiKey,
        Some(5000),
        2100,
    );
    assert_eq!(
        db.account_readiness(&choice.id, 4999).unwrap().mode,
        AccountReadinessMode::ApiKey
    );
    // Expiry is a read-time test: at the boundary the answer degrades.
    assert_eq!(
        db.account_readiness(&choice.id, 5000).unwrap().mode,
        AccountReadinessMode::Unknown
    );
    // ... and the expired row stays on disk as history: a read never
    // writes, never promotes, never deletes.
    let sql = rusqlite::Connection::open(state.join("domain.sqlite3")).unwrap();
    let (outcome, expires): (String, u64) = sql
        .query_row(
            "SELECT outcome,expires_at_ms FROM account_login_observations WHERE account_id=?1",
            [&choice.id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((outcome.as_str(), expires), ("observed", 5000));
}

#[test]
fn native_account_login_records_no_credential_byte() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    let mut db = DomainRepository::open(&state).unwrap();
    let choice = prepared(&mut db);
    // The provider's own output carried a token-shaped string; the parent
    // classifies it into the closed vocabulary and nothing else crosses.
    // The token itself never enters any argument the store accepts.
    const TOKEN: &str = "sk-live-0123456789abcdef0123456789abcdef";
    login_observed(
        &mut db,
        &choice.id,
        AccountReadinessMode::ApiKey,
        Some(9000),
        2100,
    );
    let sql = rusqlite::Connection::open(state.join("domain.sqlite3")).unwrap();
    for table in ["account_login_observations", "account_login_attempts"] {
        // No COLUMN name matches /credential/ (ADR-014:411's guard).
        let mut names = sql
            .prepare(&format!("SELECT name FROM pragma_table_info('{table}')"))
            .unwrap();
        let columns: Vec<String> = names
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert!(
            columns.iter().all(|name| !name.contains("credential")),
            "no key matches /credential/ on {table}"
        );
        // No CELL carries the token bytes. The guard is about string
        // bytes, so only TEXT columns are scanned — `account_generation`
        // and the millisecond columns are INTEGER by design, and reading
        // them as strings is a type error, not a redaction gap.
        for column in &columns {
            let declared: String = sql
                .query_row(
                    &format!("SELECT type FROM pragma_table_info('{table}') WHERE name=?1"),
                    [column],
                    |r| r.get(0),
                )
                .unwrap();
            if declared != "TEXT" {
                continue;
            }
            let mut statement = sql
                .prepare(&format!("SELECT {column} FROM {table}"))
                .unwrap();
            let cells: Vec<Option<String>> = statement
                .query_map([], |r| r.get(0))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
            for cell in cells.into_iter().flatten() {
                assert!(
                    !cell.contains(TOKEN),
                    "a credential byte reached {table}.{column}"
                );
            }
        }
    }
}

#[test]
fn native_account_readiness_matches_retained_detect() {
    // The retained oracle (native/scripts/account-vectors.mjs): the retained
    // probeFramework state machine is mirrored with its citations and the
    // fixture records, per vector, the retained verdict and the native
    // answer for the same tree. The agreement being pinned: retained
    // 'ready' (a usable namespace) corresponds to an OBSERVED native fact
    // whose mode discriminates subscription from api_key; every other
    // retained state corresponds to native unknown — the retained caveat
    // (backend-v2.js:13541) says existence is not a session, and native
    // refuses the inference the other way too.
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/account-identity.json")).unwrap();
    let vectors = fixture["readiness"].as_array().unwrap();
    assert!(
        vectors.len() >= 5,
        "the oracle covers every retained state and both modes"
    );
    for vector in vectors {
        let retained = vector["retained"].as_str().unwrap();
        let expected: &str = vector["native"]["mode"].as_str().unwrap();
        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("state");
        let mut db = DomainRepository::open(&state).unwrap();
        let choice = prepared(&mut db);
        if retained == "ready" {
            // The vector's mode is the login receipt's classification.
            let mode = match expected {
                "subscription" => AccountReadinessMode::Subscription,
                "api_key" => AccountReadinessMode::ApiKey,
                other => {
                    panic!("retained 'ready' must pair with a discriminating mode, got {other}")
                }
            };
            login_observed(&mut db, &choice.id, mode, Some(9000), 2100);
        }
        let answer = db.account_readiness(&choice.id, 3000).unwrap();
        let actual = match answer.mode {
            AccountReadinessMode::Subscription => "subscription",
            AccountReadinessMode::ApiKey => "api_key",
            AccountReadinessMode::Unknown => "unknown",
        };
        assert_eq!(
            actual, expected,
            "native answer for retained state {retained}"
        );
        // The agreement itself, both directions.
        assert_eq!(
            retained == "ready",
            answer.mode != AccountReadinessMode::Unknown,
            "retained {retained} and the native answer disagree"
        );
    }
}

// ---- MA-S4: account retirement logs out and audits the transition (migration 029).

/// The ADR-014:411 guard applied to the audit row: no credential, token or
/// session column, and no cell carries a token-shaped byte. Scans the store's
/// own table over TEXT columns.
fn assert_no_credential_byte(sql: &rusqlite::Connection, table: &str) {
    const TOKEN: &str = "sk-live-0123456789abcdef0123456789abcdef";
    let mut names = sql
        .prepare(&format!("SELECT name FROM pragma_table_info('{table}')"))
        .unwrap();
    let columns: Vec<String> = names
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(
        columns.iter().all(|name| !name.contains("credential")
            && !name.contains("token")
            && !name.contains("session")),
        "no /credential/-, /token/- or /session/-matching key on {table}"
    );
    for column in &columns {
        let mut statement = sql
            .prepare(&format!("SELECT {column} FROM {table}"))
            .unwrap();
        let cells: Vec<Option<String>> = statement
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        for cell in cells.into_iter().flatten() {
            assert!(
                !cell.contains(TOKEN),
                "a credential byte reached {table}.{column}"
            );
        }
    }
}

#[test]
fn native_account_retire_logs_out_and_audits() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    let mut db = DomainRepository::open(&state).unwrap();
    let choice = prepared(&mut db);
    // The account is active and its login was observed (MA-S1's fact).
    login_observed(
        &mut db,
        &choice.id,
        AccountReadinessMode::Subscription,
        Some(9000),
        2100,
    );
    assert_eq!(
        db.account_readiness(&choice.id, 2300).unwrap().mode,
        AccountReadinessMode::Subscription
    );
    // The operator ran the provider's own logout in the namespace; native
    // records the derived outcome and transitions active -> retired in the
    // same transaction, unpublishing the resources.
    let retired = db
        .retire_account(
            &choice.id,
            LogoutObservation {
                readiness: LogoutReadiness::Observed,
                detail: "logged-out-subscription".into(),
            },
        )
        .unwrap();
    assert!(matches!(retired.state, AccountState::Retired));
    // The audit row exists carrying the account id, the transition clock and
    // the observed readiness word.
    let sql = rusqlite::Connection::open(state.join("domain.sqlite3")).unwrap();
    let (account_id, retired_at, readiness): (String, u64, String) = sql
        .query_row(
            "SELECT account_id,retired_at_ms,readiness FROM account_logout_receipts WHERE account_id=?1",
            [&choice.id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(account_id, choice.id);
    assert_eq!(readiness, "observed");
    assert!(retired_at >= 2100, "the transition clock is set");
    // No credential, token or session byte reaches the audit row.
    assert_no_credential_byte(&sql, "account_logout_receipts");
}

#[test]
fn native_account_retire_logout_failure_is_unknown() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    let mut db = DomainRepository::open(&state).unwrap();
    let choice = prepared(&mut db);
    // A login was observed, so the namespace reads ready before retirement.
    login_observed(
        &mut db,
        &choice.id,
        AccountReadinessMode::ApiKey,
        Some(9000),
        2100,
    );
    assert_eq!(
        db.account_readiness(&choice.id, 2300).unwrap().mode,
        AccountReadinessMode::ApiKey
    );
    // The logout cannot be observed (failure / refusal / unclassifiable).
    let retired = db
        .retire_account(
            &choice.id,
            LogoutObservation {
                readiness: LogoutReadiness::Unknown,
                detail: "logout-failed".into(),
            },
        )
        .unwrap();
    assert!(matches!(retired.state, AccountState::Retired));
    // The readiness read is unknown: the newest observation is the uncertain
    // shadow, so MA-S1's latest-of-any-outcome rule answers unknown.
    assert_eq!(
        db.account_readiness(&choice.id, 2400).unwrap().mode,
        AccountReadinessMode::Unknown,
        "a failed logout never reads as ready"
    );
    // The audit row records the unknown outcome — no clean-retirement claim.
    let sql = rusqlite::Connection::open(state.join("domain.sqlite3")).unwrap();
    let (readiness, detail): (String, String) = sql
        .query_row(
            "SELECT readiness,logout_detail FROM account_logout_receipts WHERE account_id=?1",
            [&choice.id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(readiness, "unknown");
    assert_eq!(detail, "logout-failed");
    // No credential, token or session byte reaches the audit row.
    assert_no_credential_byte(&sql, "account_logout_receipts");
}
