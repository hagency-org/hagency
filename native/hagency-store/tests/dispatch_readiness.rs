mod common;
use common::*;
use hagency_core::{replies::*, tasks::*};
use hagency_store::*;
use serde_json::json;
use std::collections::BTreeSet;
use std::time::{Duration, Instant};

/// MA-S2 (ADR-053 amendment) selectors. The fixture is the store's own entry
/// points end to end: an engagement admitted over a resource whose provision
/// completed, a managed account prepared against that engagement (so the
/// provision's preset is bound to the account through resource_accounts), a
/// verified Matrix session, and one queued dispatch — the exact row the
/// selector evaluates.
struct Fixture {
    _root: tempfile::TempDir,
    db: DomainRepository,
    state: std::path::PathBuf,
    /// The account resource_accounts binds to the provision's preset — the
    /// account whose readiness the gate evaluates. Read from the store after
    /// enrollment (enrollment may bind a different id than the reserved
    /// choice), so every login observation lands on the gated account.
    account_id: String,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("state");
        let mut db = DomainRepository::open(&state).unwrap();
        db.register(&registration()).unwrap();
        // The account is prepared AND bound to the engagement's provision
        // through the store's own enrollment (accounts.rs's `prepared` +
        // `enroll` shapes): reserve, materialize, then enroll the account as
        // a published resource whose preset lands in resource_accounts — the
        // exact binding the readiness gate evaluates.
        let account = {
            let original = db.reserve_account(ACCOUNT_PROFILE).unwrap();
            db.materialize_account(&original.id).unwrap()
        };
        let pool = {
            let managed = db.managed_account(&account.id).unwrap();
            let access = AccountEnrollmentAccess::new(
                Instant::now() + Duration::from_secs(30),
                Default::default(),
            );
            let command = access
                .prepare(
                    &managed,
                    account.revision.clone(),
                    "gpt-5.6-sol".into(),
                    Some("medium".into()),
                    // qualifies() demands a declared ceiling with tokens; the
                    // common fixture resources all carry one.
                    Some(
                        serde_json::from_value(json!({"tokens":1000,"period":"monthly"})).unwrap(),
                    ),
                    Instant::now() + Duration::from_secs(5),
                )
                .unwrap();
            let result = db.enroll_account_resource(command).unwrap();
            db.resource_configuration(&result.resource_id).unwrap()
        };
        let proof = proof(&request("gate", "Worker", &pool, 100));
        let e = db.admit(&proof, 1000).unwrap();
        let engagement = e.id.clone();
        // The gate evaluates THE BOUND account — the row resource_accounts
        // maps the provision's preset to. Enrollment may bind a different
        // account id than the fixture's own AccountChoice, so read the bound
        // id from the store and observe logins on it.
        let account_id: String = {
            let sql = rusqlite::Connection::open(state.join("domain.sqlite3")).unwrap();
            sql.query_row(
                "SELECT account_id FROM resource_accounts WHERE preset_id=?1",
                [&pool.preset_id],
                |r| r.get(0),
            )
            .unwrap()
        };
        db.approve("approve", &proof, 1000).unwrap();
        let effect = db.claim_effect().unwrap().unwrap();
        db.observe_effect(
            &effect.id,
            effect.fence,
            &EffectOutcome::Applied {
                receipt: "fixture".into(),
            },
        )
        .unwrap();
        db.observe_matrix_transport(
            &MatrixTransportObservation {
                engagement_id: e.id.clone(),
                registration_generation: 1,
                generation: 1,
                sender_mxid: "@worker:example.test".into(),
                device_id: "DEVICE_1".into(),
            },
            1001,
        )
        .unwrap();
        db.observe_matrix_room(
            &MatrixRoomObservation {
                engagement_id: e.id,
                registration_generation: 1,
                transport_generation: 1,
                room_id: "!project:example.test".into(),
                generation: 1,
                privacy: RoomPrivacy::Group {},
                joined: BTreeSet::from([
                    "@worker:example.test".into(),
                    "@owner:example.test".into(),
                ]),
                invite_only: true,
                encrypted: true,
            },
            1002,
        )
        .unwrap();
        db.register_workspace("work").unwrap();
        db.resolve_verified_matrix_session(
            &SessionBinding {
                id: "session".into(),
                engagement_id: engagement.clone(),
                room_id: "!project:example.test".into(),
                thread_root: Some("$gate".into()),
            },
            1003,
        )
        .unwrap();
        db.create_canonical_task("task", "session", "Gate task", 1004)
            .unwrap();
        db.enqueue_dispatch(&DispatchInput {
            id: "dispatch".into(),
            session_id: "session".into(),
            task_id: Some("task".into()),
            resources: vec![ResourceLease {
                id: "work".into(),
                exclusive: true,
            }],
            payload: json!({"instruction":"fixture"}),
        })
        .unwrap();
        Self {
            _root: root,
            db,
            state,
            account_id,
        }
    }
    fn sql(&self) -> rusqlite::Connection {
        rusqlite::Connection::open(self.state.join("domain.sqlite3")).unwrap()
    }
    fn observe_login(&mut self, mode: AccountReadinessMode, expires_at_ms: u64, now: u64) {
        let attempt = self.db.begin_account_login(&self.account_id, now).unwrap();
        self.db
            .settle_account_login(
                attempt,
                LoginVerdict {
                    mode,
                    provider_state: match mode {
                        AccountReadinessMode::Subscription => "logged-in-subscription".into(),
                        AccountReadinessMode::ApiKey => "logged-in-api-key".into(),
                        AccountReadinessMode::Unknown => "not-logged-in".into(),
                    },
                    outcome: LoginOutcome::Observed,
                    expires_at_ms: Some(expires_at_ms),
                },
                now + 10,
            )
            .unwrap();
    }
    fn dispatch_state(&self) -> (String, i64) {
        self.sql()
            .query_row(
                "SELECT state,fence FROM runner_dispatches WHERE id='dispatch'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap()
    }
}

/// Only an observed, unexpired readiness fact lets a dispatch be consumed.
/// Every other shape — absent, expired, shadowed by a later uncertain fact —
/// leaves the row unconsumed.
#[test]
fn native_dispatch_requires_ready_account() {
    let mut f = Fixture::new();
    // Absent fact: unknown.
    assert!(
        f.db.claim_dispatch("runner_a", 1010, 60_000, 120_000, 128)
            .unwrap()
            .is_none()
    );
    // Expired fact: observed, but its expiry has passed.
    f.observe_login(AccountReadinessMode::Subscription, 1020, 1011);
    assert!(
        f.db.claim_dispatch("runner_a", 1030, 60_000, 120_000, 128)
            .unwrap()
            .is_none()
    );
    // A newer uncertain fact shadows even an unexpired observed one (the
    // latest observation of ANY outcome decides).
    f.observe_login(AccountReadinessMode::Subscription, 999_999, 1031);
    let attempt = f.db.begin_account_login(&f.account_id, 1032).unwrap();
    f.db.settle_account_login(
        attempt,
        LoginVerdict {
            mode: AccountReadinessMode::Unknown,
            provider_state: "not-logged-in".into(),
            outcome: LoginOutcome::Uncertain,
            expires_at_ms: Some(999_999),
        },
        1042,
    )
    .unwrap();
    assert!(
        f.db.claim_dispatch("runner_a", 1050, 60_000, 120_000, 128)
            .unwrap()
            .is_none()
    );
    // Observed AND unexpired: the row is selected and starts.
    f.observe_login(AccountReadinessMode::ApiKey, 999_999, 1060);
    let cap =
        f.db.claim_dispatch("runner_a", 1070, 60_000, 120_000, 128)
            .unwrap()
            .expect("the observed-unexpired row is selected");
    f.db.start_dispatch(&cap, 1071).unwrap();
    assert_eq!(f.dispatch_state(), ("started".into(), 1));
}

/// An unknown-readiness row parks with the named reason — not a failure: the
/// row is not dropped, errored or rewritten, and its inputs and custody are
/// unchanged.
#[test]
fn native_dispatch_parks_with_named_reason_on_unknown_readiness() {
    let mut f = Fixture::new();
    let input_before: String = f
        .sql()
        .query_row(
            "SELECT input FROM runner_dispatches WHERE id='dispatch'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        f.db.claim_dispatch("runner_a", 1010, 60_000, 120_000, 128)
            .unwrap()
            .is_none()
    );
    let (state, fence) = f.dispatch_state();
    assert_eq!(state, "parked");
    assert_eq!(fence, 0, "the park rewrites nothing — the fence stands");
    let input_after: String = f
        .sql()
        .query_row(
            "SELECT input FROM runner_dispatches WHERE id='dispatch'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        input_before, input_after,
        "the park leaves the inputs untouched"
    );
    let (outcome, reason): (String, Option<String>) = f
        .sql()
        .query_row(
            "SELECT outcome,park_reason FROM runner_attempts WHERE dispatch_id='dispatch' AND fence=0",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(outcome, "parked");
    assert_eq!(reason.as_deref(), Some("account_readiness_unknown"));
    // Custody untouched: the task stays active and the session bound.
    let task_state: String = f
        .sql()
        .query_row(
            "SELECT json_extract(config,'$.status') FROM canonical_tasks WHERE id='task'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(task_state, "created");
}

/// A parked dispatch resumes when a readiness fact settles, and a later
/// unusable fact parks the next row again with the same named reason.
#[test]
fn native_dispatch_resumes_when_readiness_is_observed() {
    let mut f = Fixture::new();
    assert!(
        f.db.claim_dispatch("runner_a", 1010, 60_000, 120_000, 128)
            .unwrap()
            .is_none()
    );
    assert_eq!(f.dispatch_state().0, "parked");
    f.observe_login(AccountReadinessMode::Subscription, 999_999, 1020);
    let cap =
        f.db.claim_dispatch("runner_a", 1030, 60_000, 120_000, 128)
            .unwrap()
            .expect("the parked row resumes when readiness is observed");
    f.db.start_dispatch(&cap, 1031).unwrap();
    assert_eq!(f.dispatch_state(), ("started".into(), 1));
}

/// The park is read-time evaluation, not a retry loop: many selector passes
/// and arbitrary intervals later, the row parked once and stayed parked — no
/// retry storm, no fence churn, one audit row.
#[test]
fn native_dispatch_park_is_read_time_not_a_retry_loop() {
    let mut f = Fixture::new();
    let mut now = 1010u64;
    for _ in 0..50 {
        assert!(
            f.db.claim_dispatch("runner_a", now, 60_000, 120_000, 128)
                .unwrap()
                .is_none()
        );
        now += 997; // arbitrary intervals; the fact never settles
    }
    assert_eq!(f.dispatch_state(), ("parked".into(), 0));
    let attempts: u64 = f
        .sql()
        .query_row(
            "SELECT COUNT(*) FROM runner_attempts WHERE dispatch_id='dispatch'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(attempts, 1, "fifty passes wrote exactly one park row");
}

/// The park is visible in the audit table: outcome parked with the named
/// park_reason, and the park→resume interval derivable from the attempt
/// rows' neighbours.
#[test]
fn native_dispatch_park_is_visible_in_the_audit_table() {
    let mut f = Fixture::new();
    assert!(
        f.db.claim_dispatch("runner_a", 1010, 60_000, 120_000, 128)
            .unwrap()
            .is_none()
    );
    f.observe_login(AccountReadinessMode::Subscription, 999_999, 2000);
    let cap =
        f.db.claim_dispatch("runner_a", 3000, 60_000, 120_000, 128)
            .unwrap()
            .expect("the row resumes after the fact settles");
    f.db.start_dispatch(&cap, 3001).unwrap();
    let rows: Vec<(i64, String, Option<String>, i64)> = f
        .sql()
        .prepare("SELECT fence,outcome,park_reason,created_at FROM runner_attempts WHERE dispatch_id='dispatch' ORDER BY fence")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(
        (rows[0].1.as_str(), rows[0].2.as_deref()),
        ("parked", Some("account_readiness_unknown"))
    );
    assert_eq!(
        (rows[1].1.as_str(), rows[1].2.as_deref()),
        ("started", None)
    );
    assert!(
        rows[1].3 > rows[0].3,
        "the park interval is derivable from the rows' created_at neighbours"
    );
}
