#[path = "../../hagency-store/tests/common/mod.rs"]
mod common;
use common::*;
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
}
impl Fixture {
    fn new() -> Self {
        Self::configured(false)
    }
    fn configured(approvals: bool) -> Self {
        Self::configured_account(approvals, false)
    }
    fn configured_account(approvals: bool, managed: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let work = root.path().join("固定 工作目录");
        hagency_store::private::directory(&work).unwrap();
        let work = work.canonicalize().unwrap();
        let mut db = DomainRepository::open(&root.path().join("state")).unwrap();
        db.register(&registration()).unwrap();
        let mut accounts = Vec::new();
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
            db.resource_configuration(&result.resource_id).unwrap()
        } else {
            let pool = resource("pool", "seat", 1000);
            db.put_resource(&pool).unwrap();
            pool
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
        }
    }
    fn host(&self, mode: &str, workspace: &str, missing: bool) -> Host {
        let mut environment = BTreeMap::from([
            ("PATH".into(), "".into()),
            ("HAGENCY_OFFLINE_MODE".into(), mode.into()),
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
        assert_eq!(self.state(), "started");
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
        // The original initialize frame was fully written. The transport keeps
        // a fully written frame as unconfirmed until its flush is observed, so
        // an injected failure that lands before that observation reports the
        // complete frame rather than None; a partial frame would be a defect.
        assert!(
            original
                .write
                .is_none_or(|write| write.accepted_bytes == write.total_bytes),
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
