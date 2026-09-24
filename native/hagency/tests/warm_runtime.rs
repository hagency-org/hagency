//! Real native pipes/helper/API with an offline peer; not a live factory/model.
#[path = "../../hagency-store/tests/common/mod.rs"]
mod common;
use common::*;
use hagency_core::{replies::*, tasks::*};
use hagency_execution::{ApprovalHost, Failure, Host, Limits, Protocol, WarmLimits, WarmRuntime};
use hagency_store::{
    DomainRepository, DomainStore, Effect, EffectOutcome, ManagedAccount, OwnedProvisionScope,
    agent_home::{HomeProject, ManagedAgentHome, ManagedHomePlan, ProjectMode},
};
use salvo::Listener;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
fn limits() -> Limits {
    Limits {
        operation_ms: 30_000,
        response_ms: 2000,
    }
}
fn warm_limits() -> WarmLimits {
    WarmLimits {
        initialize: Limits {
            operation_ms: 3000,
            response_ms: 2000,
        },
        idle_ms: 10_000,
    }
}
fn binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_hagency-owned-mcp-probe"))
        .canonicalize()
        .unwrap()
}
struct Fixture {
    root: tempfile::TempDir,
    work: PathBuf,
    domain: DomainStore,
    effect: Effect,
    scope: OwnedProvisionScope,
    home: Arc<ManagedAgentHome>,
    account: Option<ManagedAccount>,
    context: Arc<hagency_store::task_context::RetainedTaskContext>,
    custody: hagency_store::Store,
    handle: salvo::server::ServerHandle,
    server: tokio::task::JoinHandle<()>,
    address: std::net::SocketAddr,
}
impl Fixture {
    async fn new(managed: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("state");
        let custody =
            hagency_store::Store::start(hagency_store::Repository::open(&state).unwrap(), 16)
                .unwrap();
        let mut db = DomainRepository::open(&state).unwrap();
        db.register(&registration()).unwrap();
        let (account, pool) = if managed {
            let choice = db.reserve_account(hagency_store::ACCOUNT_PROFILE).unwrap();
            let choice = db.materialize_account(&choice.id).unwrap();
            let account = db.managed_account(&choice.id).unwrap();
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
            let clock = now();
            let attempt = db.begin_account_login(account.id(), clock).unwrap();
            db.settle_account_login(
                attempt,
                hagency_store::LoginVerdict {
                    mode: hagency_store::AccountReadinessMode::Subscription,
                    provider_state: "logged-in-subscription".into(),
                    outcome: hagency_store::LoginOutcome::Observed,
                    expires_at_ms: Some(clock + 60_000),
                },
                clock,
            )
            .unwrap();
            (Some(account), pool)
        } else {
            (None, resource("pool", "seat", 1000))
        };
        db.put_resource(&pool).unwrap();
        let approved = proof(&request("warm", "Worker", &pool, 100));
        db.admit(&approved, 1000).unwrap();
        db.approve("approve", &approved, 1000).unwrap();
        let effect = db.claim_effect().unwrap().unwrap();
        let scope = db
            .provision_runtime_scope(&effect, &registration())
            .unwrap();
        let domain = DomainStore::start(db, 16).unwrap();
        let homes = root.path().join("homes");
        hagency_store::private::directory(&homes).unwrap();
        let project = root.path().join("source-project");
        hagency_store::private::directory(&project).unwrap();
        fs::write(project.join("source.txt"), b"offline source").unwrap();
        let plan = ManagedHomePlan::new(
            homes.canonicalize().unwrap(),
            vec![HomeProject {
                project_id: "project_one".into(),
                source: project.canonicalize().unwrap(),
                mode: ProjectMode::Copy,
            }],
            PathBuf::from(env!("CARGO_BIN_EXE_hagency"))
                .canonicalize()
                .unwrap(),
        )
        .unwrap();
        let home = plan
            .materialize(
                domain.clone(),
                effect.clone(),
                registration(),
                std::time::Instant::now() + Duration::from_secs(10),
                Arc::new(AtomicBool::new(false)),
            )
            .await
            .unwrap();
        home.check_provision_scope(&scope).unwrap();
        let work = home.workdir_path().unwrap();
        let contexts = root.path().join("contexts");
        hagency_store::private::directory(&contexts).unwrap();
        let context = hagency_store::task_context::RetainedTaskContext::new(
            contexts.canonicalize().unwrap(),
            &"a".repeat(64),
        )
        .unwrap();
        let acceptor = salvo::conn::TcpListener::new("127.0.0.1:0")
            .try_bind()
            .await
            .unwrap();
        let address = acceptor.local_addr().unwrap();
        let app = hagency::App::new(
            custody.clone(),
            b"fixture_operator_token_32_bytes_minimum",
            address,
        )
        .unwrap()
        .with_domain(domain.clone());
        let service = salvo::Server::new(acceptor);
        let handle = service.handle();
        let server = tokio::spawn(async move {
            service.try_serve(app.router()).await.unwrap();
        });
        Self {
            root,
            work,
            domain,
            effect,
            scope,
            home,
            account,
            context,
            custody,
            handle,
            server,
            address,
        }
    }
    fn host(&mut self, policy: Option<ApprovalHost>) -> hagency_execution::SharedHost {
        self.build_host(policy).into_shared()
    }
    fn build_host(&mut self, policy: Option<ApprovalHost>) -> Host {
        let environment = BTreeMap::from([
            ("PATH".into(), "".into()),
            ("HAGENCY_OFFLINE_MODE".into(), "warm".into()),
        ]);
        #[cfg(windows)]
        let environment = {
            let mut environment = environment;
            environment.insert("SystemRoot".into(), std::env::var_os("SystemRoot").unwrap());
            environment
        };
        let mut host = Host::new(
            binary(),
            binary(),
            environment,
            BTreeMap::from([("work".into(), self.work.clone())]),
        )
        .unwrap()
        .with_task_helper(
            PathBuf::from(env!("CARGO_BIN_EXE_hagency"))
                .canonicalize()
                .unwrap(),
            self.address,
        )
        .unwrap()
        .with_retained_task_context(self.context.clone())
        .unwrap();
        if let Some(account) = self.account.take() {
            host = host.with_managed_account(account).unwrap();
        }
        if let Some(policy) = policy {
            host = host.with_approvals(policy).unwrap();
        }
        host
    }
    fn start(&mut self, policy: Option<ApprovalHost>) -> WarmRuntime {
        let host = self.host(policy);
        WarmRuntime::start(
            self.domain.clone(),
            self.scope.clone(),
            self.home.clone(),
            host,
            "work".into(),
            warm_limits(),
        )
        .unwrap()
    }
    async fn activate(&self, capability_ms: u64) -> RunnerCapability {
        // Explicit offline fixture activation. This is NOT evidence that the
        // product factory can observe its own physical Applied/Active outcome.
        self.domain
            .observe_effect(
                self.effect.id.clone(),
                self.effect.fence,
                EffectOutcome::Applied {
                    receipt: "offline fixture activation only".into(),
                },
            )
            .await
            .unwrap();
        self.domain
            .observe_matrix_transport(MatrixTransportObservation {
                engagement_id: self.effect.engagement_id.clone(),
                registration_generation: 1,
                generation: 1,
                sender_mxid: "@worker:example.test".into(),
                device_id: "DEVICE".into(),
            })
            .await
            .unwrap();
        self.domain
            .observe_matrix_room(MatrixRoomObservation {
                engagement_id: self.effect.engagement_id.clone(),
                registration_generation: 1,
                transport_generation: 1,
                room_id: "!project:example.test".into(),
                generation: 1,
                privacy: RoomPrivacy::Group {},
                joined: std::collections::BTreeSet::from([
                    "@worker:example.test".into(),
                    "@owner:example.test".into(),
                ]),
                invite_only: true,
                encrypted: false,
            })
            .await
            .unwrap();
        self.domain
            .observe_approval_room(hagency_core::approvals::ApprovalRoomObservation {
                engagement_id: self.effect.engagement_id.clone(),
                registration_generation: 1,
                generation: 1,
                room_id: "!private:example.test".into(),
                device_id: "BOT".into(),
                joined: std::collections::BTreeSet::from([
                    "@owner:example.test".into(),
                    "@approval:example.test".into(),
                ]),
                invite_only: true,
                encrypted: true,
                available: true,
            })
            .await
            .unwrap();
        self.domain
            .resolve_verified_matrix_session(SessionBinding {
                id: "session".into(),
                engagement_id: self.effect.engagement_id.clone(),
                room_id: "!project:example.test".into(),
                thread_root: Some("$thread".into()),
            })
            .await
            .unwrap();
        self.domain.register_workspace("work".into()).await.unwrap();
        self.domain
            .create_canonical_task(
                "task".into(),
                "session".into(),
                "Exact warm task".into(),
                now(),
            )
            .await
            .unwrap();
        self.domain.enqueue_dispatch(DispatchInput {id:"dispatch".into(),session_id:"session".into(),task_id:Some("task".into()),resources:vec![ResourceLease {id:"work".into(),exclusive:true}],payload:json!({"instruction":"offline task","cwd":"/impostor","model":"impostor","done":true})}).await.unwrap();
        self.domain
            .claim_dispatch("owned_host".into(), now(), capability_ms, capability_ms, 1)
            .await
            .unwrap()
            .unwrap()
    }
    fn count(&self, query: &str) -> u64 {
        rusqlite::Connection::open(self.root.path().join("state/domain.sqlite3"))
            .unwrap()
            .query_row(query, [], |r| r.get(0))
            .unwrap()
    }
    fn state(&self, query: &str) -> String {
        rusqlite::Connection::open(self.root.path().join("state/domain.sqlite3"))
            .unwrap()
            .query_row(query, [], |r| r.get(0))
            .unwrap()
    }
    fn receipt(&self, stage: &str) -> Value {
        serde_json::from_slice(&fs::read(self.work.join(format!("owned-mcp.{stage}"))).unwrap())
            .unwrap()
    }
    fn requests(&self) -> Vec<Value> {
        fs::read_to_string(self.work.join("owned-mcp.requests"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
    async fn entered(&self) {
        tokio::time::timeout(Duration::from_secs(4), async {
            while !self.work.join("owned-mcp.warm-entered").exists() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
    }
    async fn initialized(&self) {
        tokio::time::timeout(Duration::from_secs(4), async {
            loop {
                match fs::read(self.work.join("owned-mcp.warm-initialized")) {
                    Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
                        Ok(value) => {
                            assert!(value["pid"].as_u64().is_some_and(|pid| pid > 1));
                            break;
                        }
                        Err(error) if error.is_eof() => {}
                        Err(_) => panic!("original fixture initialization metadata is malformed"),
                    },
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(_) => panic!("original fixture initialization metadata is unreadable"),
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("original fixture initialization metadata did not finish");
    }
    async fn close(self) {
        self.handle.stop_graceful(Some(Duration::from_secs(1)));
        self.server.await.unwrap();
        self.domain.shutdown().await.unwrap();
        self.custody.shutdown().await.unwrap();
    }
    fn no_task_io(&self) {
        assert_eq!(
            self.requests()
                .iter()
                .filter(|r| r["method"] == "initialize")
                .count(),
            1
        );
        assert!(
            !self
                .requests()
                .iter()
                .any(|r| r["method"] == "thread/start" || r["method"] == "turn/start")
        );
        assert!(!self.work.join("owned-mcp.spawned").exists());
        assert_eq!(
            fs::read_dir(self.root.path().join("contexts"))
                .unwrap()
                .count(),
            0
        );
    }
}

#[tokio::test]
async fn native_warm_owned_runtime_current_owner() {
    let mut f = Fixture::new(false).await;
    fs::write(
        f.work.join("owned-mcp.warm-idle-gate"),
        b"controlled original peer exit",
    )
    .unwrap();
    let mut warm = f.start(None);
    warm.ready().await.unwrap();
    warm.ready().await.unwrap();
    f.initialized().await;
    f.no_task_io();
    let original = f.receipt("warm-initialized")["pid"].clone();
    assert_eq!(original, f.receipt("warm-entered")["pid"]);
    fs::write(
        f.work.join("owned-mcp.warm-idle-exit"),
        b"exit original peer, no numeric signal/adoption",
    )
    .unwrap();
    let failure = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            match warm.ready().await {
                Ok(()) => tokio::time::sleep(Duration::from_millis(5)).await,
                Err(failure) => break failure,
            }
        }
    })
    .await
    .unwrap();
    assert!(
        matches!(failure, Failure::LostAuthority { .. }),
        "{failure:?}"
    );
    assert_eq!(warm.ready().await.err(), Some(failure));
    f.no_task_io();
    assert_eq!(f.count("SELECT COUNT(*) FROM canonical_tasks"), 0);
    assert_eq!(f.count("SELECT COUNT(*) FROM matrix_session_routes"), 0);
    assert_eq!(f.state("SELECT state FROM effects"), "started");
    assert_eq!(f.state("SELECT state FROM engagements"), "reserved");
    assert!(matches!(
        f.scope.claim_warm(),
        Err(hagency_store::Error::Busy)
    ));
    drop(warm);
    f.close().await;
}
#[tokio::test]
async fn native_warm_owned_runtime_observation_custody() {
    for expired in [false, true] {
        let mut f = Fixture::new(false).await;
        let mut warm = f.start(None);
        warm.ready().await.unwrap();
        f.initialized().await;
        let original = f.receipt("warm-initialized")["pid"].clone();
        let lock = rusqlite::Connection::open(f.root.path().join("state/domain.sqlite3")).unwrap();
        lock.execute_batch("BEGIN IMMEDIATE").unwrap();
        // This second Ready wait has already enqueued its original inspection
        // before awaiting its retained receiver. The actual writer is locked.
        assert!(
            tokio::time::timeout(Duration::from_millis(5), warm.ready())
                .await
                .is_err()
        );
        lock.execute_batch("ROLLBACK").unwrap();
        if expired {
            // No new inspection/deadline may replace the admitted old receiver,
            // even when its positive is buffered while the original owner lives.
            tokio::time::sleep(Duration::from_millis(
                warm_limits().initialize.response_ms + 20,
            ))
            .await;
            assert_eq!(warm.ready().await.err(), Some(Failure::Deadline));
            assert_eq!(warm.ready().await.err(), Some(Failure::Deadline));
        } else {
            warm.ready().await.unwrap();
        }
        assert_eq!(original, f.receipt("warm-initialized")["pid"]);
        f.no_task_io();
        assert_eq!(f.count("SELECT COUNT(*) FROM canonical_tasks"), 0);
        assert_eq!(f.state("SELECT state FROM effects"), "started");
        drop(warm);
        f.close().await;
    }
}
#[tokio::test]
async fn native_warm_owned_runtime_unknown_capacity() {
    let mut failed = Fixture::new(false).await;
    let mut other = Fixture::new(false).await;
    let mut third = Fixture::new(false).await;
    let policy = ApprovalHost::new(2, 1, 1000, 2000).unwrap();
    let stalled = failed
        .build_host(Some(policy.clone()))
        .with_guardian_prepare_stall()
        .into_shared();
    let mut warm = WarmRuntime::start(
        failed.domain.clone(),
        failed.scope.clone(),
        failed.home.clone(),
        stalled,
        "work".into(),
        warm_limits(),
    )
    .unwrap();
    assert_eq!(warm.ready().await.err(), Some(Failure::SpawnFailed));
    drop(warm); // joins real late spawn and transitive stop
    assert_eq!(failed.count("SELECT COUNT(*) FROM canonical_tasks"), 0);
    assert_eq!(failed.state("SELECT state FROM effects"), "started");
    assert!(matches!(
        failed.scope.claim_warm(),
        Err(hagency_store::Error::Busy)
    ));
    let mut second = other.start(Some(policy.clone()));
    second.ready().await.unwrap();
    let host = third.host(Some(policy));
    assert_eq!(
        WarmRuntime::start(
            third.domain.clone(),
            third.scope.clone(),
            third.home.clone(),
            host,
            "work".into(),
            warm_limits()
        )
        .err(),
        Some(Failure::ApprovalCapacity)
    );
    assert!(!third.work.join("owned-mcp.requests").exists());
    drop(second);
    third.close().await;
    other.close().await;
    failed.close().await;
}

#[tokio::test]
async fn native_pre_activation_warm_owned_runtime() {
    let mut f = Fixture::new(true).await;
    let mut other = Fixture::new(false).await;
    let policy = ApprovalHost::new(2, 1, 1000, 2000).unwrap();
    let mut warm = f.start(Some(policy.clone()));
    let mut second = other.start(Some(policy));
    warm.ready().await.unwrap();
    second.ready().await.unwrap();
    assert_eq!(f.state("SELECT state FROM engagements"), "reserved");
    assert_eq!(f.state("SELECT state FROM effects"), "started");
    assert_eq!(f.count("SELECT COUNT(*) FROM canonical_tasks"), 0);
    f.no_task_io();
    f.entered().await;
    let original_pid = f.receipt("warm-entered")["pid"].clone();
    tokio::time::sleep(Duration::from_millis(3100)).await;
    assert!(!warm.is_finished());
    let cap = f.activate(60_000).await;
    // Both live slots remain occupied: dispatch must reuse its warm reservation.
    let mut operation = warm
        .dispatch_requiring_workspace(cap.clone(), limits())
        .unwrap();
    let registration = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if let Some(registration) = operation.take_workspace_registration() {
                break registration;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let (binding, ack) = registration.into_parts();
    binding.validate_current(&cap).await.unwrap();
    assert_eq!(f.state("SELECT state FROM runner_dispatches"), "started");
    f.no_task_io();
    ack.registered().unwrap();
    let report = operation.wait().await.unwrap();
    assert_eq!(
        report.protocol,
        Protocol::Completed,
        "failure={:?} observation={:?} helper_spawned={} cached={} ack={} readback={} exit={}",
        report.failure,
        report.runtime_observation(),
        f.work.join("owned-mcp.spawned").exists(),
        f.work.join("owned-mcp.context-cached").exists(),
        f.work.join("owned-mcp.ack").exists(),
        f.work.join("owned-mcp.readback").exists(),
        f.work.join("owned-mcp.receipt").exists()
    );
    assert_eq!(report.canonical_status, Some(TaskState::InProgress));
    assert_eq!(f.receipt("warm-thread")["pid"], original_pid);
    assert_eq!(f.receipt("warm-initialized")["pid"], original_pid);
    assert_eq!(
        f.requests()
            .iter()
            .filter(|r| r["method"] == "initialize")
            .count(),
        1
    );
    assert_eq!(f.receipt("receipt")["helper_exit"], true);
    assert_eq!(f.receipt("readback")["task"]["status"], "in_progress");
    assert!(f.receipt("readback")["task"]["heartbeat_at"].is_u64());
    assert_eq!(f.count("SELECT COUNT(*) FROM final_replies"), 0);
    let hagency_runtime::owned::Cleanup::Observed(cleanup) = report.cleanup else {
        panic!("actual cleanup required");
    };
    if !cleanup.scope.whole_tree_stopped {
        assert_eq!(report.failure, Some(Failure::CleanupUnknown));
        assert_eq!(f.count("SELECT COUNT(*) FROM resource_leases"), 1);
    }
    assert!(matches!(
        f.scope.claim_warm(),
        Err(hagency_store::Error::Busy)
    ));
    drop(report);
    drop(operation);
    drop(binding);
    drop(second);
    other.close().await;
    f.close().await;
}
#[tokio::test]
async fn native_warm_owned_runtime_custody() {
    let mut f = Fixture::new(false).await;
    fs::write(
        f.work.join("owned-mcp.warm-hold"),
        b"offline held initialize",
    )
    .unwrap();
    let mut warm = f.start(None);
    f.entered().await;
    let original_pid = f.receipt("warm-entered")["pid"].clone();
    assert!(
        tokio::time::timeout(Duration::from_millis(50), warm.ready())
            .await
            .is_err()
    );
    assert!(!warm.is_finished());
    assert_eq!(
        f.requests()
            .iter()
            .filter(|r| r["method"] == "initialize")
            .count(),
        1
    );
    fs::write(
        f.work.join("owned-mcp.warm-release"),
        b"release original peer",
    )
    .unwrap();
    warm.ready().await.unwrap();
    let cap = f.activate(60_000).await;
    let mut operation = warm.dispatch(cap, limits()).unwrap();
    let report = operation.wait().await.unwrap();
    assert_eq!(report.protocol, Protocol::Completed);
    assert_eq!(f.receipt("warm-thread")["pid"], original_pid);
    assert_eq!(
        f.requests()
            .iter()
            .filter(|r| r["method"] == "initialize")
            .count(),
        1
    );
    drop(report);
    drop(operation);
    f.close().await;
}
#[tokio::test]
async fn native_warm_owned_runtime_refusals() {
    for case in [
        "revoked_initialize",
        "foreign_dispatch",
        "expired_dispatch",
        "cancelled",
        "scope_mismatch",
        "lost_workspace_ack",
    ] {
        let mut f = Fixture::new(false).await;
        if case == "scope_mismatch" {
            let foreign = Fixture::new(false).await;
            assert!(
                WarmRuntime::start(
                    f.domain.clone(),
                    f.scope.clone(),
                    foreign.home.clone(),
                    f.host(None),
                    "work".into(),
                    warm_limits()
                )
                .is_err()
            );
            assert!(matches!(
                f.scope.claim_warm(),
                Err(hagency_store::Error::Busy)
            ));
            assert!(!f.work.join("owned-mcp.requests").exists());
            foreign.close().await;
            f.close().await;
            continue;
        }
        if case == "revoked_initialize" {
            fs::write(f.work.join("owned-mcp.warm-hold"), b"held").unwrap();
        }
        let mut warm = f.start(None);
        if case == "revoked_initialize" {
            f.entered().await;
            f.domain
                .revoke("revoke".into(), f.effect.engagement_id.clone())
                .await
                .unwrap();
            fs::write(f.work.join("owned-mcp.warm-release"), b"released").unwrap();
            assert!(matches!(
                warm.ready().await.err(),
                Some(Failure::LostAuthority { .. })
            ));
            drop(warm);
            f.no_task_io();
            assert_eq!(f.count("SELECT COUNT(*) FROM canonical_tasks"), 0);
            f.close().await;
            continue;
        }
        warm.ready().await.unwrap();
        let cap = f
            .activate(if case == "expired_dispatch" {
                100
            } else {
                60_000
            })
            .await;
        if case == "lost_workspace_ack" {
            let mut operation = warm.dispatch_requiring_workspace(cap, limits()).unwrap();
            let registration = tokio::time::timeout(Duration::from_secs(3), async {
                loop {
                    if let Some(registration) = operation.take_workspace_registration() {
                        break registration;
                    }
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            })
            .await
            .unwrap();
            let (binding, ack) = registration.into_parts();
            drop(ack);
            drop(binding);
            let report = operation.wait().await.unwrap();
            assert_eq!(report.failure, Some(Failure::Admission));
            assert_eq!(report.protocol, Protocol::NotStarted);
            assert_eq!(
                f.state("SELECT state FROM runner_dispatches"),
                "outcome_unknown"
            );
            assert_eq!(f.count("SELECT COUNT(*) FROM resource_leases"), 1);
            drop(report);
            drop(operation);
            f.no_task_io();
            f.close().await;
            continue;
        }
        if case == "cancelled" {
            warm.cancel();
            assert_eq!(warm.dispatch(cap, limits()).err(), Some(Failure::Cancelled));
        } else {
            let foreign = if case == "foreign_dispatch" {
                Some(Fixture::new(false).await)
            } else {
                None
            };
            let submitted = if let Some(foreign) = &foreign {
                foreign.activate(60_000).await
            } else {
                cap
            };
            if case == "expired_dispatch" {
                tokio::time::sleep(Duration::from_millis(150)).await;
            }
            let mut operation = warm.dispatch(submitted, limits()).unwrap();
            let report = operation.wait().await.unwrap();
            assert_eq!(report.failure, Some(Failure::Admission));
            assert_eq!(report.protocol, Protocol::NotStarted);
            drop(report);
            drop(operation);
            if let Some(foreign) = foreign {
                assert_eq!(
                    foreign.state("SELECT state FROM runner_dispatches"),
                    "leased"
                );
                foreign.close().await;
            }
        }
        f.no_task_io();
        assert_eq!(
            f.state("SELECT state FROM runner_dispatches"),
            if case == "expired_dispatch" {
                "superseded"
            } else {
                "leased"
            }
        );
        assert!(matches!(
            f.scope.claim_warm(),
            Err(hagency_store::Error::Busy)
        ));
        f.close().await;
    }
}

#[cfg(unix)]
fn local_profile(f: &Fixture, seat: &str) -> hagency_execution::LocalCodex {
    let root = f.root.path().canonicalize().unwrap();
    for name in ["provider-home", "provider-codex"] {
        hagency_store::private::directory(&root.join(name)).unwrap();
    }
    hagency_execution::LocalCodex::new(
        "pool".into(),
        seat.into(),
        root.join("provider-home"),
        root.join("provider-codex"),
    )
    .unwrap()
}

#[cfg(unix)]
#[tokio::test]
async fn native_warm_local_codex_custody() {
    use std::os::unix::fs::PermissionsExt;
    for case in ["seat", "managed", "initialize", "idle", "writable"] {
        let mut f = Fixture::new(case == "managed").await;
        let local = local_profile(&f, if case == "seat" { "other_seat" } else { "seat" });
        let codex = f.root.path().canonicalize().unwrap().join("provider-codex");
        fs::write(codex.join("auth.json"), b"opaque offline sentinel").unwrap();
        fs::set_permissions(codex.join("auth.json"), fs::Permissions::from_mode(0o000)).unwrap();
        let host = f.build_host(None).with_local_codex(local);
        if case == "managed" {
            assert!(matches!(host, Err(Failure::Admission)));
            f.close().await;
            continue;
        }
        if case == "initialize" {
            fs::write(f.work.join("owned-mcp.warm-hold"), b"held initialize").unwrap();
        }
        let start = WarmRuntime::start(
            f.domain.clone(),
            f.scope.clone(),
            f.home.clone(),
            host.unwrap().into_shared(),
            "work".into(),
            warm_limits(),
        );
        if case == "seat" {
            assert!(matches!(start, Err(Failure::Admission)));
            assert!(!f.work.join("owned-mcp.requests").exists());
        } else {
            let mut warm = start.unwrap();
            if case == "initialize" {
                f.entered().await;
            } else {
                warm.ready().await.unwrap();
                f.initialized().await;
            }
            if case == "writable" {
                fs::set_permissions(&codex, fs::Permissions::from_mode(0o777)).unwrap();
            } else {
                fs::rename(&codex, codex.with_file_name("original-provider")).unwrap();
                fs::create_dir(&codex).unwrap();
            }
            assert!(matches!(
                tokio::time::timeout(Duration::from_secs(3), warm.ready())
                    .await
                    .unwrap()
                    .err(),
                Some(Failure::LostAuthority { .. })
            ));
            drop(warm);
            assert!(
                !f.requests()
                    .iter()
                    .any(|r| r["method"] == "thread/start" || r["method"] == "turn/start")
            );
        }
        assert_eq!(f.count("SELECT COUNT(*) FROM canonical_tasks"), 0);
        assert_eq!(f.state("SELECT state FROM effects"), "started");
        assert_eq!(f.count("SELECT COUNT(*) FROM managed_accounts"), 0);
        assert!(matches!(
            f.scope.claim_warm(),
            Err(hagency_store::Error::Busy)
        ));
        f.close().await;
    }
}

#[cfg(unix)]
#[tokio::test]
async fn native_warm_local_codex_budgets() {
    for case in ["success", "short_dispatch", "initialize_deadline"] {
        let mut f = Fixture::new(false).await;
        let local = local_profile(&f, "seat");
        let policy = ApprovalHost::new(2, 1, 40_000, 2000).unwrap();
        let host = f
            .build_host(Some(policy))
            .with_local_codex(local)
            .unwrap()
            .into_shared();
        if case == "initialize_deadline" {
            fs::write(f.work.join("owned-mcp.warm-hold"), b"held initialization").unwrap();
        }
        // Equal phase/RPC durations put the original phase deadline (captured
        // before preparation/spawn) before the later initialize RPC deadline.
        let budgets = if case == "initialize_deadline" {
            WarmLimits {
                initialize: Limits {
                    operation_ms: 2000,
                    response_ms: 2000,
                },
                idle_ms: 10_000,
            }
        } else {
            warm_limits()
        };
        let started = tokio::time::Instant::now();
        let mut warm = WarmRuntime::start(
            f.domain.clone(),
            f.scope.clone(),
            f.home.clone(),
            host,
            "work".into(),
            budgets,
        )
        .unwrap();
        if case == "initialize_deadline" {
            assert_eq!(warm.ready().await.err(), Some(Failure::Deadline));
            assert!(started.elapsed() < Duration::from_secs(4));
            drop(warm);
            f.no_task_io();
            assert_eq!(f.count("SELECT COUNT(*) FROM canonical_tasks"), 0);
            f.close().await;
            continue;
        }
        warm.ready().await.unwrap();
        f.initialized().await;
        f.no_task_io();
        let original = f.receipt("warm-initialized")["pid"].clone();
        let cap = f.activate(90_000).await;
        if case == "short_dispatch" {
            assert!(matches!(
                warm.dispatch(cap, limits()),
                Err(Failure::Admission)
            ));
            f.no_task_io();
        } else {
            let mut operation = warm
                .dispatch(
                    cap,
                    Limits {
                        operation_ms: 60_000,
                        response_ms: 2000,
                    },
                )
                .unwrap();
            let report = operation.wait().await.unwrap();
            assert_eq!(report.protocol, Protocol::Completed);
            assert_eq!(f.receipt("warm-thread")["pid"], original);
            let hagency_runtime::owned::Cleanup::Observed(cleanup) = &report.cleanup else {
                panic!("original cleanup required");
            };
            assert!(cleanup.scope.whole_tree_stopped);
            drop(report);
            drop(operation);
        }
        f.close().await;
    }
}
