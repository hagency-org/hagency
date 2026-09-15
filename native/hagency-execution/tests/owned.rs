#[path = "../../hagency-store/tests/common/mod.rs"]
mod common;
use common::*;
use hagency_core::project::Resource;
use hagency_core::tasks::*;
use hagency_execution::{Failure, Host, Limits, Operation, Protocol, Settlement};
use hagency_runtime::owned::Cleanup;
use hagency_store::{DomainRepository, DomainStore, EffectOutcome, OwnedObservation};
use serde_json::json;
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[path = "owned/accounts.rs"]
mod accounts;
#[path = "owned/approval_fixture.rs"]
mod approval_fixture;
#[path = "owned/approvals.rs"]
mod approvals;
#[path = "owned/idle.rs"]
mod idle;
#[path = "owned/receive.rs"]
mod receive;
#[path = "owned/registration.rs"]
mod registration;
#[path = "owned/usage.rs"]
mod usage;
#[path = "owned/workspace.rs"]
mod workspace;

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
fn binary() -> PathBuf {
    env!("CARGO_BIN_EXE_hagency-execution-probe").into()
}
fn limits() -> Limits {
    Limits {
        operation_ms: 25_000,
        response_ms: 1500,
    }
}
struct Fixture {
    root: tempfile::TempDir,
    work: PathBuf,
    domain: DomainStore,
    cap: RunnerCapability,
    engagement: String,
    /// The account `resource_accounts` binds to the provision's preset — the
    /// id the readiness gate evaluates. Populated only for managed fixtures.
    bound_account: Option<String>,
}
impl Fixture {
    fn new() -> Self {
        Self::configured(false)
    }
    fn configured(approvals: bool) -> Self {
        Self::configured_account(approvals, false)
    }
    fn configured_account(approvals: bool, managed: bool) -> Self {
        Self::configured_resource(approvals, managed, resource("pool", "seat", 1000))
    }
    fn configured_resource(approvals: bool, managed: bool, unmanaged_pool: Resource) -> Self {
        let root = tempfile::tempdir().unwrap();
        let work = root.path().join("固定 工作目录");
        hagency_store::private::directory(&work).unwrap();
        let work = work.canonicalize().unwrap();
        let mut db = DomainRepository::open(&root.path().join("state")).unwrap();
        db.register(&registration()).unwrap();
        let mut accounts = Vec::new();
        let mut bound_account = None;
        let pool = if managed {
            for marker in ["selected-A", "other-B"] {
                let prepared = db.reserve_account(hagency_store::ACCOUNT_PROFILE).unwrap();
                let choice = db.materialize_account(&prepared.id).unwrap();
                fs::write(
                    root.path()
                        .join("state")
                        .join(&choice.id)
                        .join("fixture-account-marker"),
                    marker,
                )
                .unwrap();
                accounts.push(choice.id);
            }
            let account = db.managed_account(&accounts[0]).unwrap();
            let choice = db.account_choices().unwrap().remove(0);
            let access = hagency_store::AccountEnrollmentAccess::new(
                std::time::Instant::now() + Duration::from_secs(30),
                Default::default(),
            );
            let command = access
                .prepare(
                    &account,
                    choice.revision,
                    "gpt-5.6-sol".into(),
                    Some("medium".into()),
                    Some(
                        serde_json::from_value(json!({"tokens":1000,"period":"monthly"})).unwrap(),
                    ),
                    std::time::Instant::now() + Duration::from_secs(5),
                )
                .unwrap();
            let result = db.enroll_account_resource(command).unwrap();
            let pool = db.resource_configuration(&result.resource_id).unwrap();
            // MA-S2 (ADR-053 amendment): the readiness gate evaluates the account
            // `resource_accounts` binds to the provision's preset. Seed an
            // observed, unexpired fact so the managed fixture is consumable —
            // the selector parks an unobserved account, and the Host admission
            // re-checks the same predicate. Read the bound id back the same way
            // the predicate does (enrollment may bind a different id than the
            // reserved choice).
            bound_account = Some(
                rusqlite::Connection::open(root.path().join("state/domain.sqlite3"))
                    .unwrap()
                    .query_row(
                        "SELECT account_id FROM resource_accounts WHERE preset_id=?1",
                        [&pool.preset_id],
                        |r| r.get::<_, String>(0),
                    )
                    .unwrap(),
            );
            let attempt = db
                .begin_account_login(bound_account.as_ref().unwrap(), now())
                .unwrap();
            db.settle_account_login(
                attempt,
                hagency_store::LoginVerdict {
                    mode: hagency_store::AccountReadinessMode::Subscription,
                    provider_state: "logged-in-subscription".into(),
                    outcome: hagency_store::LoginOutcome::Observed,
                    expires_at_ms: Some(now() + 3_600_000),
                },
                now(),
            )
            .unwrap();
            pool
        } else {
            db.put_resource(&unmanaged_pool).unwrap();
            unmanaged_pool
        };
        let proof = proof(&request("allocation", "Worker", &pool, 100));
        let e = db.admit(&proof, 1000).unwrap();
        db.approve("approved", &proof, 1000).unwrap();
        let effect = db.claim_effect().unwrap().unwrap();
        db.observe_effect(
            &effect.id,
            effect.fence,
            &EffectOutcome::Applied {
                receipt: "offline_provisioned".into(),
            },
        )
        .unwrap();
        if approvals {
            approval_fixture::bindings(&mut db, &e.id);
        }
        let binding = SessionBinding {
            id: "session".into(),
            engagement_id: e.id.clone(),
            room_id: "!project:example.test".into(),
            thread_root: Some("$thread".into()),
        };
        if approvals {
            db.resolve_verified_matrix_session(&binding, now()).unwrap();
        } else {
            db.register_session(&binding).unwrap();
        }
        db.register_workspace("work").unwrap();
        db.create_canonical_task("task", "session", "Exact frozen task", now())
            .unwrap();
        db.enqueue_dispatch(&DispatchInput { id: "dispatch".into(), session_id: "session".into(), task_id: Some("task".into()), resources: vec![ResourceLease { id:"work".into(), exclusive:true }], payload: json!({"instruction":"do the offline work", "task_id":"impostor", "cwd":"/model/override", "model":"model-override", "done":true}) }).unwrap();
        let cap = db
            .claim_dispatch("owned_host", now(), 60_000, 60_000, 1)
            .unwrap()
            .unwrap();
        let domain = DomainStore::start(db, 16).unwrap();
        Self {
            root,
            work,
            domain,
            cap,
            engagement: e.id,
            bound_account,
        }
    }
    fn host(&self, mode: &str, workspace: &str, missing: bool) -> Host {
        let mut environment = BTreeMap::from([
            ("PATH".into(), "".into()),
            ("HAGENCY_OFFLINE_MODE".into(), mode.into()),
            // The operation budget every operation below grants — the probe
            // derives every one of its waits from this value (never its own
            // literal), so the two sides can never disagree about time.
            (
                "HAGENCY_OPERATION_BUDGET_MS".into(),
                limits().operation_ms.to_string().into(),
            ),
        ]);
        if let Some(system) = std::env::var_os("SystemRoot") {
            environment.insert("SystemRoot".into(), system);
        }
        Host::new(
            binary(),
            if missing {
                self.work.join("missing-executable")
            } else {
                binary()
            },
            environment,
            BTreeMap::from([(workspace.into(), self.work.clone())]),
        )
        .unwrap()
    }
    fn operation(&self, mode: &str) -> Operation {
        Operation::start(
            self.domain.clone(),
            self.cap.clone(),
            self.host(mode, "work", false),
            limits(),
        )
        .unwrap()
    }
    fn account_ids(&self) -> Vec<String> {
        self.sql()
            .prepare("SELECT id FROM managed_accounts ORDER BY ordinal")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }
    fn sql(&self) -> rusqlite::Connection {
        rusqlite::Connection::open(self.root.path().join("state/domain.sqlite3")).unwrap()
    }
    fn state(&self) -> String {
        self.sql()
            .query_row(
                "SELECT state FROM runner_dispatches WHERE id='dispatch'",
                [],
                |r| r.get(0),
            )
            .unwrap()
    }
    fn count(&self, query: &str) -> u64 {
        self.sql().query_row(query, [], |r| r.get(0)).unwrap()
    }
    fn marker(&self) -> PathBuf {
        self.work.join("owned-dispatch.entered")
    }
    async fn entered(&self) {
        let until = tokio::time::Instant::now() + Duration::from_secs(4);
        while !self.marker().exists() {
            assert!(
                tokio::time::Instant::now() < until,
                "actual fixture never entered"
            );
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        // The marker proves the child entered; the state read must not race the
        // transient "started" snapshot. Under parallel load the whole lifecycle
        // (marker -> Done -> stop -> settlement) can complete between this
        // fixture's own 5ms polls, and the operation's failure teardown
        // (observe_owned_failure -> conversation_lifecycle::fence_dispatch)
        // settles started -> outcome_unknown. Every state accepted below is
        // reachable ONLY through started: fence_dispatch writes outcome_unknown
        // from started/parked only (a leased dispatch loses to queued),
        // park_dispatch authorizes from
        // ["started"], and completed is started's own terminal write.
        assert!(
            matches!(
                self.state().as_str(),
                "started" | "parked" | "completed" | "outcome_unknown"
            ),
            "dispatch never reached started: {}",
            self.state()
        );
    }
    fn quarantined(&self) {
        assert_eq!(self.state(), "outcome_unknown");
        assert_eq!(
            self.count("SELECT dirty FROM workspace_resources WHERE id='work'"),
            1
        );
        assert_eq!(
            self.count("SELECT quarantined FROM runner_sessions WHERE id='session'"),
            1
        );
        assert_eq!(self.count("SELECT COUNT(*) FROM resource_leases"), 1);
        assert_eq!(
            self.count("SELECT COUNT(*) FROM runner_outputs WHERE accepted=1"),
            0
        );
        assert_eq!(self.count("SELECT COUNT(*) FROM final_replies"), 0);
        assert_eq!(
            self.count(
                "SELECT COUNT(*) FROM canonical_tasks WHERE json_extract(config,'$.status')='done'"
            ),
            0
        );
    }
}

#[tokio::test]
async fn native_owned_dispatch_real_pipes() {
    let f = Fixture::new();
    let mut operation = f.operation("normal");
    let report = operation.wait().await.unwrap();
    assert_eq!(report.protocol, Protocol::Completed);
    assert_eq!(report.canonical_status, Some(TaskState::InProgress));
    assert_eq!(report.text.as_deref(), Some("离线管道验证完成"));
    let Cleanup::Observed(cleanup) = report.cleanup else {
        panic!("cleanup observation missing");
    };
    assert!(cleanup.scope.leader_exited);
    if cfg!(target_os = "macos") {
        assert!(!cleanup.scope.whole_tree_stopped);
        assert!(report.retains_process_custody());
        assert_eq!(report.failure, Some(Failure::CleanupUnknown));
        f.quarantined();
    } else {
        assert!(cleanup.scope.whole_tree_stopped);
        assert!(!report.retains_process_custody());
        assert_eq!(report.settlement, Settlement::Completed);
        assert_eq!(report.failure, None);
        // A clean completion records no settlement cause: the marker exists
        // only to name the store refusal behind a lost settlement.
        assert_eq!(report.settlement_cause, None);
        assert_eq!(f.state(), "completed");
        assert_eq!(f.count("SELECT COUNT(*) FROM resource_leases"), 0);
        assert_eq!(f.count("SELECT COUNT(*) FROM final_replies"), 0);
    }
    let requests = fs::read_to_string(f.work.join("owned-dispatch.requests")).unwrap();
    let values: Vec<serde_json::Value> = requests
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let thread = values
        .iter()
        .find(|v| v["method"] == "thread/start")
        .unwrap();
    assert_eq!(thread["params"]["model"], "gpt-5.6-sol");
    // The runner is launched in the ordinary projection of the retained root
    // (ADR-116 amendment); on Windows that drops the verbatim prefix.
    assert_eq!(
        thread["params"]["cwd"],
        hagency_execution::ordinary_launch_path(&f.work).unwrap()
    );
    let turn = values.iter().find(|v| v["method"] == "turn/start").unwrap();
    assert_eq!(turn["params"]["approvalPolicy"], "on-request");
    assert_eq!(turn["params"]["sandboxPolicy"]["networkAccess"], false);
    assert!(
        turn["params"]["input"][0]["text"]
            .as_str()
            .unwrap()
            .contains("impostor")
    );
    drop(report);
    f.domain.shutdown().await.unwrap();
}

#[tokio::test]
async fn native_owned_dispatch_cancel_and_unknown() {
    for mode in ["cancel-wait", "expired", "revoked", "drop"] {
        let f = Fixture::new();
        let mut operation = f.operation("silent");
        f.entered().await;
        if mode == "drop" {
            let at = std::time::Instant::now();
            drop(operation);
            assert!(at.elapsed() < Duration::from_secs(15));
        } else {
            match mode {
                "cancel-wait" => {
                    assert!(
                        tokio::time::timeout(Duration::from_millis(30), operation.wait())
                            .await
                            .is_err()
                    );
                    // Fresh wait on the retained result only, never repoll the
                    // cancelled native/domain operation future.
                }
                "expired" => {
                    f.sql()
                        .execute(
                            "UPDATE runner_dispatches SET capability_until=?1 WHERE id='dispatch'",
                            [now()],
                        )
                        .unwrap();
                }
                "revoked" => {
                    f.domain
                        .revoke("revoke".into(), f.engagement.clone())
                        .await
                        .unwrap();
                }
                _ => unreachable!(),
            }
            let report = operation.wait().await.unwrap();
            assert_eq!(
                report.failure,
                Some(if mode == "cancel-wait" {
                    Failure::Cancelled
                } else {
                    Failure::LostAuthority
                })
            );
            drop(report);
        }
        f.quarantined();
        let path = f.work.join("owned-dispatch.pulse");
        let before = fs::metadata(&path).map_or(0, |v| v.len());
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(fs::metadata(&path).map_or(0, |v| v.len()), before);
        f.domain.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn native_owned_runtime_failure_observation() {
    use hagency_execution::RuntimeStage;
    use hagency_runtime::codex::{session, transport};
    for (mode, cause) in [
        ("silent", transport::Error::Timeout),
        ("eof", transport::Error::PeerEof),
    ] {
        let f = Fixture::new();
        let mut operation = f.operation(mode);
        let mut report = operation.wait().await.unwrap();
        assert!(
            f.marker().is_file(),
            "the original child must actually enter"
        );
        assert_eq!(report.failure, Some(Failure::Protocol));
        assert_eq!(report.protocol, Protocol::Unknown);
        let original = *report.runtime_observation().unwrap();
        assert_eq!(original.stage, RuntimeStage::Initialize);
        assert_eq!(
            original.session_error,
            Some(session::Error::Transport(cause))
        );
        assert_eq!(original.transport_cause, Some(cause));
        assert_eq!(original.pending_requests, Some(1));
        assert_eq!(original.pending_server_requests, Some(0));
        // The original initialize frame's write custody is honest in three
        // shapes: no writer installed (`write: None`), the complete frame
        // held unconfirmed until its flush is observed, or — the Windows
        // case — ARMED with zero bytes, the injected failure landing after
        // the frame was handed to the writer but before any byte was
        // accepted. A partial frame (some bytes accepted, not all) would be
        // the only defect; those three are each truthful observations.
        assert!(
            original.write.is_none_or(|write| {
                write.accepted_bytes == write.total_bytes || write.accepted_bytes == 0
            }),
            "{:?}",
            original.write
        );
        report.retry_stop();
        assert_eq!(report.runtime_observation(), Some(&original));
        assert_eq!(report.failure, Some(Failure::Protocol));
        assert_eq!(report.protocol, Protocol::Unknown);
        if cfg!(target_os = "macos") {
            assert!(report.retains_process_custody());
        } else {
            assert!(!report.retains_process_custody());
        }
        f.quarantined();
        drop(report);
        f.domain.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn native_owned_dispatch_admission_and_protocol_failure() {
    for mode in [
        "wrong-workspace",
        "spawn-failed",
        "eof",
        "wrong-scope",
        "approval",
    ] {
        let f = Fixture::new();
        let host = f.host(
            mode,
            if mode == "wrong-workspace" {
                "not-work"
            } else {
                "work"
            },
            mode == "spawn-failed",
        );
        let mut operation =
            Operation::start(f.domain.clone(), f.cap.clone(), host, limits()).unwrap();
        let report = operation.wait().await.unwrap();
        if mode == "wrong-workspace" {
            assert_eq!(report.failure, Some(Failure::Admission));
            assert_eq!(
                report.settlement,
                Settlement::Negative(OwnedObservation::Unstarted)
            );
            assert!(!f.marker().exists());
            assert_eq!(f.count("SELECT COUNT(*) FROM resource_leases"), 0);
        } else {
            assert_eq!(
                report.failure,
                Some(match mode {
                    "spawn-failed" => Failure::SpawnFailed,
                    "approval" => Failure::UnsupportedApproval,
                    _ => Failure::Protocol,
                })
            );
            f.quarantined();
        }
        drop(report);
        f.domain.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn native_claude_dispatch_refuses_without_runner() {
    // ADR-142 / RUN-E: a dispatch naming claude is refused by name before
    // any workspace is resolved, custody-checked or process spawned. The
    // host below deliberately maps a wrong workspace id and a missing
    // executable: before the hoist this dispatch died inside the workspace
    // get as the generic Failure::Admission; only the hoisted named guard
    // can answer this configuration with the framework itself.
    let claude_pool: Resource = serde_json::from_value(json!({
        "presetId":"pool","seatId":"seat","framework":"claude",
        "model":"claude-opus-5","provider":"anthropic",
        "ceiling":{"tokens":1000,"period":"monthly"}
    }))
    .unwrap();
    let f = Fixture::configured_resource(false, false, claude_pool);
    let mut operation = Operation::start(
        f.domain.clone(),
        f.cap.clone(),
        f.host("usage-gate", "not-work", true),
        limits(),
    )
    .unwrap();
    let report = operation.wait().await.unwrap();
    assert_eq!(
        report.failure,
        Some(Failure::UnsupportedRunner {
            framework: "claude".into()
        })
    );
    assert_eq!(
        report.settlement,
        Settlement::Negative(OwnedObservation::Unstarted)
    );
    assert!(!f.marker().exists()); // refused before any spawn
    assert_eq!(f.count("SELECT COUNT(*) FROM resource_leases"), 0);
    // The operator-facing refusal word is the payload-carrying projection:
    // the closed OwnedFailure enum serializes the framework name itself,
    // not only the category, so the word an operator reads is claude.
    let output: String = f
        .sql()
        .query_row(
            "SELECT output FROM runner_outputs WHERE dispatch_id='dispatch'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let refusal: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert_eq!(
        refusal["host_owned_failure"]["unsupported_runner"]["framework"],
        "claude"
    );
    drop(report);
    f.domain.shutdown().await.unwrap();
}

#[tokio::test]
async fn native_owned_dispatch_database_lock_refuses_before_spawn() {
    let f = Fixture::new();
    let lock = f.sql();
    lock.execute_batch("BEGIN IMMEDIATE").unwrap();
    let mut operation = f.operation("normal");
    // Existing SQLite busy timeout (100ms) remains unchanged. This is a known
    // lock failure before Started, not a claimed lost-response simulation.
    let mut report = operation.wait().await.unwrap();
    assert!(!f.marker().exists());
    assert_eq!(f.state(), "leased");
    lock.execute_batch("ROLLBACK").unwrap();
    assert_eq!(report.failure, Some(Failure::Admission));
    assert!(!f.marker().exists());
    assert_eq!(report.settlement, Settlement::Unknown);
    assert_eq!(f.state(), "leased");
    assert_eq!(f.count("SELECT COUNT(*) FROM resource_leases"), 1);
    assert_eq!(
        report.retry_reconcile().await,
        Settlement::Negative(OwnedObservation::Unstarted)
    );
    assert_eq!(f.state(), "superseded");
    assert_eq!(f.count("SELECT COUNT(*) FROM resource_leases"), 0);
    drop(report);
    f.domain.shutdown().await.unwrap();
}

#[tokio::test]
async fn native_owned_dispatch_deadline_stops_and_fences() {
    let f = Fixture::new();
    let mut operation = Operation::start(
        f.domain.clone(),
        f.cap.clone(),
        f.host("silent", "work", false),
        Limits {
            operation_ms: 2000,
            response_ms: 2000,
        },
    )
    .unwrap();
    let at = std::time::Instant::now();
    let report = operation.wait().await.unwrap();
    assert_eq!(report.failure, Some(Failure::Deadline));
    assert!(f.marker().exists(), "deadline fixture must actually launch");
    assert!(at.elapsed() < Duration::from_secs(15));
    f.quarantined();
    drop(report);
    f.domain.shutdown().await.unwrap();
}

/// Scenario "A spawn that outlives the operation budget is abandoned, fenced
/// and never orphaned": the double stalls the blocking spawn thread past the
/// granted budget; the bounded await expires; the operation returns
/// SpawnFailed with the ADR's Cleanup::Unknown{TimedOut}; the fence is
/// recorded BEFORE the late child is stopped and reaped (asserted by row
/// observation after wait(), not by assuming the no-child ordering); the late
/// child — a REAL probe — is reaped through the custody handoff or the
/// process-group Drop backstop, never orphaned; the guardian's own 5s Prepare
/// watch is the third layer and may fire or be pre-empted, both acceptable.
#[tokio::test]
async fn native_owned_dispatch_spawn_outliving_budget_is_fenced() {
    let f = Fixture::new();
    // `silent`: the late child pulses while alive, so "appeared, then went
    // quiet" is real reaping evidence against a live process.
    let host = f
        .host("silent", "work", false)
        .with_guardian_prepare_stall();
    let mut operation = Operation::start(
        f.domain.clone(),
        f.cap.clone(),
        host,
        Limits {
            operation_ms: 2000,
            response_ms: 2000,
        },
    )
    .unwrap();
    let at = std::time::Instant::now();
    let report = operation.wait().await.unwrap();
    // The operation returned within its budget plus the checkpoint cadence —
    // the deadline was enforced across the spawn, not just around it.
    assert!(at.elapsed() < Duration::from_secs(15));
    // The uncertain-start word, never a plain Deadline: not-started is
    // unprovable through a timed-out handshake.
    assert_eq!(report.failure, Some(Failure::SpawnFailed));
    assert_eq!(
        report.settlement,
        Settlement::Negative(OwnedObservation::Fenced)
    );
    // The fence itself, with the attempt fenced exactly as today's
    // SpawnFailed path (ADR-053 amendment): the store rows precede the stop
    // of the late child, asserted by reading them at this point — the child
    // may still be running while these hold.
    f.quarantined();
    drop(report);
    // The late child is stopped and reaped, never orphaned: the pulse marker
    // stops growing once the custody handoff (or the Drop backstop's socket
    // EOF, which triggers the guardian's own process-group kill) reaches it.
    // The stall exceeds the budget, so the child started only after the
    // fence; its pulse may exist briefly and must go quiet.
    let pulse = f.work.join("owned-dispatch.pulse");
    let bounded = std::time::Instant::now() + Duration::from_secs(15);
    let mut before = fs::metadata(&pulse).map_or(0, |v| v.len());
    while std::time::Instant::now() < bounded {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let now = fs::metadata(&pulse).map_or(0, |v| v.len());
        if now == before {
            break;
        }
        before = now;
    }
    let settled = fs::metadata(&pulse).map_or(0, |v| v.len());
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        fs::metadata(&pulse).map_or(0, |v| v.len()),
        settled,
        "the late child's pulse went quiet: stopped and reaped, never orphaned"
    );
    f.domain.shutdown().await.unwrap();
}

#[tokio::test]
async fn native_owned_dispatch_fixture_artifacts_are_temp_owned() {
    let root_path = {
        let f = Fixture::new();
        let root_canonical = f.root.path().canonicalize().unwrap();
        let mut operation = f.operation("normal");
        f.entered().await;
        let report = operation.wait().await.unwrap();
        drop(report);
        // Every marker lives under the TempDir-owned work directory — never a
        // production state directory. Paths are compared canonicalized, not by
        // a separator-assuming string (the fixture work dir is a Unicode path).
        assert!(
            f.work.starts_with(&root_canonical),
            "fixture markers must live under the temp root"
        );
        assert!(
            f.work.join("owned-dispatch.entered").is_file(),
            "fixture must write its entry marker"
        );
        assert!(
            f.work.join("owned-dispatch.requests").is_file(),
            "fixture must write its request journal"
        );
        // No owned-dispatch.* path is written anywhere the production state
        // directory reaches.
        let state = f.root.path().join("state");
        for entry in std::fs::read_dir(&state).unwrap() {
            let entry = entry.unwrap();
            assert!(
                !entry
                    .file_name()
                    .to_string_lossy()
                    .contains("owned-dispatch"),
                "no owned-dispatch path may reach the production state directory"
            );
        }
        f.domain.shutdown().await.unwrap();
        f.root.path().to_owned()
    };
    // The owner is dropped: nothing survives the drop.
    assert!(
        !root_path.exists(),
        "temp-owned fixture artifacts must self-clean on drop"
    );
}
