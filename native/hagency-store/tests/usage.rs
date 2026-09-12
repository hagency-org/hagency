mod common;
use common::*;
use hagency_core::{JSON_SAFE_MAX, tasks::*};
use hagency_metering::{Framework, observation::UsageObservation};
use hagency_store::*;
use serde_json::json;
mod usage {
    use super::*;
    mod admission;
    mod bounds;
    mod ceiling;
    mod vectors;
}

struct Fixture {
    root: tempfile::TempDir,
    db: DomainRepository,
    engagement: String,
    next: usize,
}
impl Fixture {
    fn new(framework: Framework) -> Self {
        Self::with_ceiling(framework, 1000)
    }
    fn with_ceiling(framework: Framework, ceiling: u64) -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut db = DomainRepository::open(&root.path().join("state")).unwrap();
        db.register(&registration()).unwrap();
        let mut pool = resource("usage_pool", "usage_seat", ceiling);
        if framework == Framework::Claude {
            pool.framework = "claude".into();
            pool.model = "claude-sonnet-5".into();
            pool.reasoning = None;
        }
        db.put_resource(&pool).unwrap();
        let p = proof(&request("usage_request", "Worker", &pool, 100));
        let engagement = db.admit(&p, 1000).unwrap();
        db.approve("approve", &p, 1000).unwrap();
        let effect = db.claim_effect().unwrap().unwrap();
        db.observe_effect(
            &effect.id,
            effect.fence,
            &EffectOutcome::Applied {
                receipt: "offline fixture".into(),
            },
        )
        .unwrap();
        Self {
            root,
            db,
            engagement: engagement.id,
            next: 0,
        }
    }
    fn prepare(&mut self) -> (RunnerCapability, OwnedDispatchScope) {
        self.next += 1;
        let id = format!("usage_{}", self.next);
        self.db
            .register_session(&SessionBinding {
                id: id.clone(),
                engagement_id: self.engagement.clone(),
                room_id: "!project:example.test".into(),
                thread_root: Some(format!("$thread_{}", self.next)),
            })
            .unwrap();
        self.db
            .create_canonical_task(&id, &id, "Observe finite usage", 1001)
            .unwrap();
        self.db.register_workspace(&id).unwrap();
        self.db
            .enqueue_dispatch(&DispatchInput {
                id: id.clone(),
                session_id: id.clone(),
                task_id: Some(id.clone()),
                resources: vec![ResourceLease {
                    id,
                    exclusive: true,
                }],
                payload: json!({"untrusted_agent_hint":"someone else"}),
            })
            .unwrap();
        let cap = self
            .db
            .claim_dispatch("fixture_runner", 1002, 60000, 120000, 128)
            .unwrap()
            .unwrap();
        let admission = self.db.owned_dispatch_scope(&cap, 1003).unwrap();
        (cap, admission)
    }
    fn start(&mut self) -> (RunnerCapability, OwnedDispatchScope, UsageSource) {
        let (cap, admission) = self.prepare();
        let started = self
            .db
            .start_owned_dispatch(&cap, admission.fingerprint(), 1004)
            .unwrap();
        let source = self.db.bind_usage_source(&cap, &started, 1005).unwrap();
        (cap, started, source)
    }
    fn sql(&self) -> rusqlite::Connection {
        rusqlite::Connection::open(self.root.path().join("state/domain.sqlite3")).unwrap()
    }
    fn count(&self, table: &str) -> u64 {
        self.sql()
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    }
    fn totals(&self) -> UsageSummary {
        self.db.usage_summary(&self.engagement).unwrap()
    }
}
fn codex(fresh: u64, output: u64, cached: u64) -> UsageObservation {
    UsageObservation::parse(Framework::Codex,&json!({"payload":{"info":{"total_token_usage":{"input_tokens":fresh+cached,"output_tokens":output,"cached_input_tokens":cached,"reasoning_output_tokens":0,"total_tokens":fresh+cached+output}}}}).to_string()).unwrap()
}
fn claude(input: u64, output: u64, write: u64, read: u64) -> UsageObservation {
    UsageObservation::parse(Framework::Claude,&json!({"uuid":"message","message":{"usage":{"input_tokens":input,"output_tokens":output,"cache_creation_input_tokens":write,"cache_read_input_tokens":read}}}).to_string()).unwrap()
}
fn empty() -> UsageObservation {
    UsageObservation::parse(Framework::Codex, "").unwrap()
}

#[test]
fn native_usage_source_authority() {
    let mut f = Fixture::new(Framework::Codex);
    let (cap, admission) = f.prepare();
    assert!(f.db.bind_usage_source(&cap, &admission, 1003).is_err());
    let started =
        f.db.start_owned_dispatch(&cap, admission.fingerprint(), 1004)
            .unwrap();
    let mut wrong = cap.clone();
    wrong.secret = "0".repeat(64);
    assert!(f.db.bind_usage_source(&wrong, &started, 1005).is_err());
    let source = f.db.bind_usage_source(&cap, &started, 1005).unwrap();
    assert_eq!(
        f.db.bind_usage_source(&cap, &started, 1006).unwrap().id(),
        source.id()
    );
    assert_eq!(f.count("usage_sources"), 1);
    assert!(
        f.db.record_usage_observation(&source, "wrong_framework", &claude(1, 2, 3, 4), 2000)
            .is_err()
    );
    let mut other = Fixture::new(Framework::Codex);
    let (_, _, foreign) = other.start();
    assert!(
        f.db.record_usage_observation(&foreign, "foreign", &codex(1, 2, 3), 2000)
            .is_err()
    );
    let (_cap, other_scope) = f.prepare();
    assert!(f.db.bind_usage_source(&cap, &other_scope, 1006).is_err());
    f.db.revoke("retire", &f.engagement).unwrap();
    assert_eq!(
        f.db.bind_usage_source(&cap, &started, 2000).unwrap().id(),
        source.id()
    );
    f.db.record_usage_observation(&source, "historical", &codex(10, 2, 3), 2000)
        .unwrap();
    assert!(
        f.db.check_owned_dispatch(&cap, started.fingerprint(), 2001)
            .is_err()
    );
    assert_eq!(f.totals().known_high_water_lower_bound.unwrap().input, 10);
    // A Started scope that was never bound before retirement cannot create a new
    // historical identity by claiming that a completed source path matches.
    let mut g = Fixture::new(Framework::Codex);
    let (cap, admission) = g.prepare();
    let started =
        g.db.start_owned_dispatch(&cap, admission.fingerprint(), 1004)
            .unwrap();
    g.db.revoke("retire", &g.engagement).unwrap();
    assert!(g.db.bind_usage_source(&cap, &started, 2000).is_err());
    assert_eq!(g.count("usage_sources"), 0);
}

#[test]
fn native_usage_observation_evidence() {
    let mut f = Fixture::new(Framework::Codex);
    let (_, _, source) = f.start();
    let zero = codex(0, 0, 0);
    assert!(!zero.incomplete());
    assert!(
        f.db.usage_period(&f.engagement, UsagePeriodKind::Daily, 2000)
            .unwrap()
            .is_none()
    );
    f.db.record_usage_observation(&source, "zero", &zero, 2000)
        .unwrap();
    let period =
        f.db.usage_period(&f.engagement, UsagePeriodKind::Daily, 2000)
            .unwrap()
            .unwrap();
    assert_eq!(period.observed_growth.input, Some(0));
    assert!(!period.incomplete);
    f.db.record_usage_observation(&source, "growth", &codex(10, 2, 100), 2001)
        .unwrap();
    f.db.record_usage_observation(&source, "empty", &empty(), 2002)
        .unwrap();
    let summary = f.totals();
    assert_eq!(summary.latest_counts.unwrap().input, None);
    assert_eq!(summary.known_high_water_lower_bound.unwrap().input, 10);
    assert_eq!(summary.historically_incomplete_sources, 1);
    let malformed = UsageObservation::parse(Framework::Codex, "not json").unwrap();
    f.db.record_usage_observation(&source, "malformed", &malformed, 2003)
        .unwrap();
    let view = f.db.usage_source(&source).unwrap();
    assert_eq!(
        view.latest_observation.unwrap()["diagnostics"]["malformedLines"],
        1
    );
    let duplicate =
        UsageObservation::parse(Framework::Codex, "{\"payload\":{},\"payload\":{}}").unwrap();
    f.db.record_usage_observation(&source, "parse_error", &duplicate, 2004)
        .unwrap();
    let replay =
        f.db.record_usage_observation(&source, "parse_error", &duplicate, 0)
            .unwrap();
    assert!(replay.replayed);
    let view = f.db.usage_source(&source).unwrap();
    assert_eq!(view.latest_observation.unwrap()["failure"], "duplicate_key");
    assert_eq!(view.high_water.cache_read, Some(100));
    let lower =
        f.db.record_usage_observation(&source, "regression", &codex(1, 1, 1), 2005)
            .unwrap();
    assert!(lower.regressed);
    let summary = f.totals();
    assert_eq!(summary.latest_counts.unwrap().input, Some(1));
    assert_eq!(summary.known_high_water_lower_bound.unwrap().input, 10);
    assert_eq!(summary.regression_observations, 1);
    assert_eq!(summary.latest_incomplete_sources, 1);
    let current = f.db.usage_source(&source).unwrap();
    assert!(current.latest_incomplete && current.latest_regressed);
    assert_eq!(summary.historically_incomplete_sources, 1);
    // A later complete non-regressing snapshot clears only the latest flag;
    // missing/regressing historical coverage remains sticky across clean data.
    f.db.record_usage_observation(&source, "recovered_latest", &codex(10, 2, 100), 2006)
        .unwrap();
    let summary = f.totals();
    assert_eq!(summary.latest_incomplete_sources, 0);
    assert_eq!(summary.historically_incomplete_sources, 1);
    let view = f.db.usage_source(&source).unwrap();
    assert!(!view.latest_incomplete && !view.latest_regressed);
    assert_eq!(view.regressions, 1);
    let private = UsageObservation::parse(
        Framework::Codex,
        &json!({"payload":{"cwd":"/private/credential-home","model":"credential-secret"}})
            .to_string(),
    )
    .unwrap();
    let encoded = serde_json::to_string(&private).unwrap();
    assert!(!encoded.contains("credential"));
    assert!(!format!("{private:?}").contains("private"));
    assert!(
        UsageObservation::parse(
            Framework::Codex,
            &" ".repeat(hagency_metering::MAX_SNAPSHOT_BYTES + 1)
        )
        .is_err()
    );
}

#[test]
fn native_usage_restart_and_reappearance() {
    let mut f = Fixture::new(Framework::Codex);
    let (cap, started, source) = f.start();
    f.db.record_usage_observation(&source, "original", &codex(10, 2, 3), 2000)
        .unwrap();
    f.db.observe_owned_failure(&cap, OwnedFailure::Cancelled, 2001)
        .unwrap();
    let path = f.root.path().join("state");
    drop(f.db);
    f.db = DomainRepository::open(&path).unwrap();
    let restored = f.db.restore_usage_source(source.id()).unwrap();
    assert_eq!(
        f.db.bind_usage_source(&cap, &started, 3000).unwrap().id(),
        restored.id()
    );
    // The source file may have been pruned; no replacement identity is created.
    f.db.record_usage_observation(&restored, "missing", &empty(), 3000)
        .unwrap();
    f.db.record_usage_observation(&restored, "reappeared", &codex(10, 2, 3), 3001)
        .unwrap();
    assert_eq!(f.totals().known_high_water_lower_bound.unwrap().input, 10);
    f.db.record_usage_observation(&restored, "new_growth", &codex(15, 3, 4), 3002)
        .unwrap();
    let replay =
        f.db.record_usage_observation(&restored, "original", &codex(10, 2, 3), 0)
            .unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.observed_at, 2000);
    assert_eq!(f.count("usage_sources"), 1);
    assert_eq!(f.totals().known_high_water_lower_bound.unwrap().input, 15);
}

#[test]
fn native_usage_receipts_and_clock() {
    let mut f = Fixture::new(Framework::Codex);
    let (_, _, source) = f.start();
    f.db.record_usage_observation(&source, "one", &codex(10, 2, 3), 2000)
        .unwrap();
    assert!(matches!(
        f.db.record_usage_observation(&source, "one", &codex(11, 2, 3), 2001),
        Err(Error::Conflict)
    ));
    assert!(
        f.db.record_usage_observation(&source, "backward", &codex(20, 2, 3), 1999)
            .is_err()
    );
    assert!(
        f.db.record_usage_observation(&source, "future", &codex(20, 2, 3), u64::MAX)
            .is_err()
    );
    assert_eq!(f.count("usage_receipts"), 1);
    assert_eq!(f.totals().known_high_water_lower_bound.unwrap().input, 10);
    f.sql().execute_batch("CREATE TRIGGER reject_usage_receipt BEFORE INSERT ON usage_receipts BEGIN SELECT RAISE(FAIL,'fixture failure'); END;").unwrap();
    assert!(
        f.db.record_usage_observation(&source, "atomic", &codex(50, 2, 3), 86400000)
            .is_err()
    );
    assert_eq!(f.count("usage_receipts"), 1);
    assert_eq!(f.count("usage_periods"), 2);
    assert_eq!(f.totals().known_high_water_lower_bound.unwrap().input, 10);
    assert_eq!(
        f.sql()
            .query_row("SELECT observed_at FROM usage_clock", [], |r| r
                .get::<_, u64>(0))
            .unwrap(),
        2000
    );
    f.sql()
        .execute_batch("DROP TRIGGER reject_usage_receipt;")
        .unwrap();
    f.db.record_usage_observation(&source, "atomic", &codex(50, 2, 3), 3000)
        .unwrap();
    assert_eq!(f.totals().known_high_water_lower_bound.unwrap().input, 50);
}

#[test]
fn native_usage_migration() {
    let f = Fixture::new(Framework::Codex);
    let path = f.root.path().join("state");
    drop(f.db);
    let sql = rusqlite::Connection::open(path.join("domain.sqlite3")).unwrap();
    remove_usage_schema(&sql);
    sql.pragma_update(None, "user_version", 16).unwrap();
    drop(sql);
    for _ in 0..2 {
        let db = DomainRepository::open(&path).unwrap();
        assert_eq!(db.usage_summary(&f.engagement).unwrap().sources, 0);
    }
    let sql = rusqlite::Connection::open(path.join("domain.sqlite3")).unwrap();
    assert_eq!(
        sql.pragma_query_value(None, "user_version", |r| r.get::<_, u64>(0))
            .unwrap(),
        23
    );
    sql.execute_batch(
        "ALTER TABLE usage_receipts RENAME COLUMN observation TO missing_observation;",
    )
    .unwrap();
    drop(sql);
    assert!(matches!(DomainRepository::open(&path), Err(Error::Schema)));
}
