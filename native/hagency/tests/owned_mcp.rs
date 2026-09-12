#[path = "../../hagency-store/tests/common/mod.rs"]
mod common;
use common::*;
use hagency_core::{replies::*, tasks::*};
use hagency_execution::{Failure, Host, Limits, Operation, Protocol, Settlement};
use hagency_runtime::owned::Cleanup;
use hagency_store::{DomainRepository, DomainStore, EffectOutcome, OwnedObservation};
use salvo::Listener;
use serde_json::json;
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
fn binary() -> PathBuf {
    env!("CARGO_BIN_EXE_hagency-owned-mcp-probe").into()
}
fn limits() -> Limits {
    Limits {
        operation_ms: 30_000,
        response_ms: 2000,
    }
}
struct Fixture {
    root: tempfile::TempDir,
    work: PathBuf,
    domain: DomainStore,
    cap: RunnerCapability,
    custody: hagency_store::Store,
    handle: salvo::server::ServerHandle,
    server: tokio::task::JoinHandle<()>,
    address: std::net::SocketAddr,
}
impl Fixture {
    async fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let work = root.path().join("固定 工作目录");
        hagency_store::private::directory(&work).unwrap();
        let work = work.canonicalize().unwrap();
        let custody = hagency_store::Store::start(
            hagency_store::Repository::open(&root.path().join("state")).unwrap(),
            16,
        )
        .unwrap();
        let mut db = DomainRepository::open(&root.path().join("state")).unwrap();
        db.register(&registration()).unwrap();
        let pool = resource("pool", "seat", 1000);
        db.put_resource(&pool).unwrap();
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
        db.observe_matrix_transport(
            &MatrixTransportObservation {
                engagement_id: e.id.clone(),
                registration_generation: 1,
                generation: 1,
                sender_mxid: "@worker:example.test".into(),
                device_id: "DEVICE".into(),
            },
            now(),
        )
        .unwrap();
        db.observe_matrix_room(
            &MatrixRoomObservation {
                engagement_id: e.id.clone(),
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
            },
            now(),
        )
        .unwrap();
        db.resolve_verified_matrix_session(
            &SessionBinding {
                id: "session".into(),
                engagement_id: e.id.clone(),
                room_id: "!project:example.test".into(),
                thread_root: Some("$thread".into()),
            },
            now(),
        )
        .unwrap();
        db.register_workspace("work").unwrap();
        db.create_canonical_task("task", "session", "Exact frozen task", now())
            .unwrap();
        db.enqueue_dispatch(&DispatchInput { id: "dispatch".into(), session_id: "session".into(), task_id: Some("task".into()), resources: vec![ResourceLease { id:"work".into(), exclusive:true }], payload: json!({"instruction":"do the offline work", "task_id":"impostor", "cwd":"/model/override", "model":"model-override", "done":true}) }).unwrap();
        let cap = db
            .claim_dispatch("owned_host", now(), 60_000, 60_000, 1)
            .unwrap()
            .unwrap();
        let domain = DomainStore::start(db, 16).unwrap();
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
        let server = salvo::Server::new(acceptor);
        let handle = server.handle();
        let server = tokio::spawn(async move {
            server.try_serve(app.router()).await.unwrap();
        });
        Self {
            root,
            work,
            domain,
            cap,
            custody,
            handle,
            server,
            address,
        }
    }
    fn host(&self, mode: &str, reserved: bool) -> Host {
        let mut environment = BTreeMap::from([
            ("PATH".into(), "".into()),
            ("HAGENCY_OFFLINE_MODE".into(), mode.into()),
        ]);
        #[cfg(windows)]
        environment.insert("SystemRoot".into(), std::env::var_os("SystemRoot").unwrap());
        if reserved {
            environment.insert("hagency_runner_capability".into(), "impostor".into());
        }
        Host::new(
            binary(),
            binary(),
            environment,
            BTreeMap::from([("work".into(), self.work.clone())]),
        )
        .unwrap()
    }
    async fn close(self) {
        self.handle.stop_graceful(Some(Duration::from_secs(1)));
        self.server.await.unwrap();
        self.domain.shutdown().await.unwrap();
        self.custody.shutdown().await.unwrap();
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
}

#[tokio::test]
async fn native_owned_mcp_configuration_admission() {
    let f = Fixture::new().await;
    let helper = PathBuf::from(env!("CARGO_BIN_EXE_hagency"));
    for address in [
        "192.0.2.1:1234",
        "0.0.0.0:1234",
        "127.0.0.1:0",
        "[::1%1]:1234",
    ] {
        assert!(
            f.host("heartbeat", false)
                .with_task_helper(helper.clone(), address.parse().unwrap())
                .is_err()
        );
    }
    assert!(
        f.host("heartbeat", true)
            .with_task_helper(helper.clone(), f.address)
            .is_err()
    );
    assert!(
        f.host("heartbeat", false)
            .with_task_helper("relative".into(), f.address)
            .is_err()
    );
    assert!(
        f.host("heartbeat", false)
            .with_task_helper(f.work.clone(), f.address)
            .is_err()
    );
    assert!(
        f.host("heartbeat", false)
            .with_task_helper(f.work.join("absent"), f.address)
            .is_err()
    );
    assert_eq!(f.state(), "leased");
    assert!(!f.work.join("owned-mcp.requests").exists());
    f.close().await;
}
async fn roundtrip(done: bool) {
    let f = Fixture::new().await;
    let host = f
        .host(if done { "done" } else { "heartbeat" }, false)
        .with_task_helper(env!("CARGO_BIN_EXE_hagency").into(), f.address)
        .unwrap();
    let initial_epoch = f
        .domain
        .owned_dispatch_scope(f.cap.clone())
        .await
        .unwrap()
        .task()
        .execution_epoch;
    let mut operation = Operation::start(f.domain.clone(), f.cap.clone(), host, limits()).unwrap();
    let report = operation.wait().await.unwrap();
    let task: Task = serde_json::from_str(
        &f.sql()
            .query_row(
                "SELECT config FROM canonical_tasks WHERE id='task'",
                [],
                |r| r.get::<_, String>(0),
            )
            .unwrap(),
    )
    .unwrap();
    let mut observations = Vec::new();
    // Distinguish "the parent probe never started" from "the helper never
    // reported". `owned-mcp.requests` is written by the parent before it spawns
    // the helper, and `owned-mcp.spawned` is written by the parent the instant
    // the helper is created. Both are checked so an empty stage list names its
    // own cause.
    let parent_started = f.work.join("owned-mcp.requests").is_file();
    let helper_spawned = f.work.join("owned-mcp.spawned").is_file();
    for stage in ["ack", "readback", "receipt"] {
        let path = f.work.join(format!("owned-mcp.{stage}"));
        let content = match fs::read_to_string(path) {
            Ok(value) => Some(value),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => panic!("fixture observation failed: {error}"),
        };
        if let Some(content) = content {
            assert!(!content.contains(&f.cap.secret));
            let value: serde_json::Value = serde_json::from_str(&content).unwrap();
            assert_eq!(value["task"], serde_json::to_value(&task).unwrap());
            if stage == "receipt" {
                assert_eq!(value["helper_exit"], true);
            }
            observations.push(stage);
        }
    }
    if done {
        // Real renewal may stop the old runtime as soon as Done advances its
        // epoch, even before the native helper receives its response. No fake
        // gate delays revocation. Missing acknowledgement/exit remains unknown.
        eprintln!("Done helper stages actually observed: {observations:?}");
    } else {
        assert_eq!(
            observations,
            ["ack", "readback", "receipt"],
            "heartbeat must confirm real helper response, readback and exit \
             (parent_started={parent_started}, helper_spawned={helper_spawned}, \
             protocol={:?}, cleanup={:?})",
            report.protocol,
            report.cleanup
        );
    }
    assert_eq!(task.execution_epoch, initial_epoch + u64::from(done));
    assert_eq!(report.canonical_status, Some(task.status));
    assert_eq!(f.count("SELECT COUNT(*) FROM final_replies"), 0);
    if done {
        assert_eq!(task.status, TaskState::Done);
        assert_eq!(report.failure, Some(Failure::LostAuthority));
        assert_eq!(report.protocol, Protocol::Unknown);
        assert_eq!(
            report.settlement,
            Settlement::Negative(OwnedObservation::Fenced)
        );
        assert_eq!(f.state(), "outcome_unknown");
        assert_eq!(f.count("SELECT fence FROM dispatch_stops WHERE dispatch_id='dispatch' AND reason='owned_runner_failure'"),f.cap.fence);
        assert_eq!(
            f.count("SELECT capability_hash IS NULL FROM runner_dispatches WHERE id='dispatch'"),
            1
        );
        assert!(f.domain.owned_dispatch_scope(f.cap.clone()).await.is_err());
        assert_eq!(
            f.count("SELECT dirty FROM workspace_resources WHERE id='work'"),
            1
        );
        assert_eq!(f.count("SELECT COUNT(*) FROM resource_leases"), 1);
        assert_eq!(
            f.count("SELECT COUNT(*) FROM runner_outputs WHERE accepted=1"),
            0
        );
    } else {
        assert_eq!(task.status, TaskState::InProgress);
        assert!(task.heartbeat_at.is_some());
        assert_eq!(report.protocol, Protocol::Completed);
        let Cleanup::Observed(cleanup) = report.cleanup else {
            panic!("actual cleanup observation required");
        };
        assert!(cleanup.scope.leader_exited);
        if cfg!(target_os = "macos") {
            assert!(!cleanup.scope.whole_tree_stopped);
            assert_eq!(report.failure, Some(Failure::CleanupUnknown));
            assert_eq!(
                report.settlement,
                Settlement::Negative(OwnedObservation::Fenced)
            );
            assert_eq!(f.count("SELECT COUNT(*) FROM resource_leases"), 1);
        } else {
            assert!(cleanup.scope.whole_tree_stopped);
            assert_eq!(report.failure, None);
            assert_eq!(report.settlement, Settlement::Completed);
            assert_eq!(f.count("SELECT COUNT(*) FROM resource_leases"), 0);
        }
    }
    let requests = fs::read_to_string(f.work.join("owned-mcp.requests")).unwrap();
    assert!(!requests.contains(&f.cap.secret));
    assert!(!report.text.as_deref().unwrap_or("").contains(&f.cap.secret));
    let values: Vec<serde_json::Value> = requests
        .lines()
        .map(|v| serde_json::from_str(v).unwrap())
        .collect();
    let thread = values
        .iter()
        .find(|v| v["method"] == "thread/start")
        .unwrap();
    assert_eq!(thread["params"]["model"], "gpt-5.6-sol");
    // The runner is launched in, and reports, the ordinary projection of the
    // retained root (ADR-116 amendment); on Windows that drops the verbatim
    // prefix of the canonical fixture path.
    assert_eq!(
        thread["params"]["cwd"],
        hagency_execution::ordinary_launch_path(&f.work).unwrap()
    );
    assert!(
        thread["params"]["developerInstructions"]
            .as_str()
            .unwrap()
            .contains("task ID is task.")
    );
    let turn = values.iter().find(|v| v["method"] == "turn/start").unwrap();
    assert!(
        turn["params"]["input"][0]["text"]
            .as_str()
            .unwrap()
            .contains("impostor")
    );
    drop(report);
    drop(operation);
    f.close().await;
}
#[tokio::test]
async fn native_owned_mcp_real_heartbeat() {
    roundtrip(false).await;
}
#[tokio::test]
async fn native_owned_mcp_real_done_epoch() {
    roundtrip(true).await;
}

#[tokio::test]
async fn native_owned_mcp_real_finish() {
    let f = Fixture::new().await;
    let host = f
        .host("finish", false)
        .with_task_helper(env!("CARGO_BIN_EXE_hagency").into(), f.address)
        .unwrap();
    let mut operation = Operation::start(f.domain.clone(), f.cap.clone(), host, limits()).unwrap();
    let report = operation.wait().await.unwrap();
    let task: Task = serde_json::from_str(
        &f.sql()
            .query_row(
                "SELECT config FROM canonical_tasks WHERE id='task'",
                [],
                |r| r.get::<_, String>(0),
            )
            .unwrap(),
    )
    .unwrap();
    assert_eq!(task.status, TaskState::Done);
    assert_eq!(task.execution_epoch, 1);
    assert_eq!(report.canonical_status, Some(TaskState::Done));
    assert_eq!(f.count("SELECT COUNT(*) FROM owned_task_completions"), 1);
    assert!(
        f.domain
            .runner_command(f.cap.clone(), RunnerCommand::Check)
            .await
            .is_err()
    );
    assert!(f.domain.owned_dispatch_scope(f.cap.clone()).await.is_err());
    let Cleanup::Observed(cleanup) = report.cleanup else {
        panic!("actual cleanup required")
    };
    assert!(cleanup.scope.leader_exited);
    if cfg!(target_os = "macos") {
        assert!(!cleanup.scope.whole_tree_stopped);
        assert_eq!(report.failure, Some(Failure::CleanupUnknown));
        assert_eq!(f.count("SELECT COUNT(*) FROM final_replies"), 0);
        assert_eq!(f.count("SELECT COUNT(*) FROM resource_leases"), 1);
    } else {
        assert!(cleanup.scope.whole_tree_stopped && cleanup.scope.signals_accepted);
        assert_eq!(report.failure, None);
        assert_eq!(report.settlement, Settlement::CanonicalReplyReady);
        assert_eq!(f.count("SELECT COUNT(*) FROM resource_leases"), 0);
        let claim = f.domain.claim_final_reply(1000).await.unwrap().unwrap();
        let send = f.domain.preview_final_reply(claim).await.unwrap();
        assert_eq!(send.body, "Verified **native MCP final result**");
        assert_eq!(send.route.room_id, "!project:example.test");
        assert_eq!(send.route.thread_root, Some("$thread".into()));
    }
    // Helper ACK/exit may race the real retirement, unlike the deterministic
    // heartbeat fixture. Canonical writer receipt is the independent truth.
    for stage in ["finish-ack", "finish-exit"] {
        if let Ok(value) = fs::read_to_string(f.work.join(format!("owned-mcp.{stage}"))) {
            assert!(!value.contains(&f.cap.secret));
            let value: serde_json::Value = serde_json::from_str(&value).unwrap();
            assert_eq!(value["completion"]["task_id"], "task");
            assert_eq!(value["completion"]["execution_epoch"], 1);
        }
    }
    let requests = fs::read_to_string(f.work.join("owned-mcp.requests")).unwrap();
    assert!(!requests.contains(&f.cap.secret));
    assert!(!requests.contains("Verified **native MCP final result**"));
    drop(report);
    drop(operation);
    f.close().await;
}

async fn local_mcp(f: &Fixture) -> hagency::mcp::Session {
    let c = hagency::task_client::Context::new(f.address, f.cap.clone(), "task".into()).unwrap();
    mcp_context(c).await
}
async fn mcp_context(c: hagency::task_client::Context) -> hagency::mcp::Session {
    let mut session = hagency::mcp::Session::new(c);
    session.handle(&serde_json::to_vec(&json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"completion-test","version":"1"}}})).unwrap()).await.unwrap();
    session
        .handle(br#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
        .await
        .unwrap();
    session
}
async fn mcp_call(
    session: &mut hagency::mcp::Session,
    id: u64,
    name: &str,
    args: serde_json::Value,
) -> serde_json::Value {
    session.handle(&serde_json::to_vec(&json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":args}})).unwrap()).await.unwrap().unwrap()["result"].clone()
}
#[tokio::test]
async fn native_owned_completion_mcp_scope_replay() {
    let f = Fixture::new().await;
    let scope = f.domain.owned_dispatch_scope(f.cap.clone()).await.unwrap();
    f.domain
        .start_owned_dispatch(f.cap.clone(), scope.fingerprint().into())
        .await
        .unwrap();
    let mut mcp = local_mcp(&f).await;
    for (index, args) in [
        json!({"id":"foreign","call_id":"x","body":"result"}),
        json!({"id":"task","call_id":"x","body":"result","room_id":"!foreign:example.test"}),
        json!({"id":"task","call_id":"x","body":""}),
    ]
    .into_iter()
    .enumerate()
    {
        let result = mcp_call(&mut mcp, index as u64 + 1, "complete_task_with_reply", args).await;
        assert_eq!(result["isError"], true);
    }
    assert_eq!(f.count("SELECT COUNT(*) FROM owned_task_completions"), 0);
    // Encoding can exceed the frame bound even when raw body bytes fit. The
    // actual MCP decoder refuses the entire frame; nothing is truncated/saved.
    let mut bounded = local_mcp(&f).await;
    let frame=serde_json::to_vec(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"complete_task_with_reply","arguments":{"id":"task","call_id":"overflow","body":"\"".repeat(17000)}}})).unwrap();
    assert!(frame.len() > hagency::mcp::FRAME_LIMIT);
    assert!(bounded.handle(&frame).await.is_err());
    assert_eq!(f.count("SELECT COUNT(*) FROM owned_task_completions"), 0);
    let args = json!({"id":"task","call_id":"exact_finish","body":"Exact immutable final"});
    let first = mcp_call(&mut mcp, 10, "complete_task_with_reply", args.clone()).await;
    assert_eq!(first["isError"], false);
    assert_eq!(first["structuredContent"]["state"], "held");
    // Receipt-only retry crosses the real HTTP boundary after its capability was
    // retired. Ordinary generic get/task mutation remain refused.
    let retry = mcp_call(&mut mcp, 11, "complete_task_with_reply", args).await;
    assert_eq!(retry["isError"], false);
    assert_eq!(retry["structuredContent"]["replayed"], true);
    assert_eq!(
        retry["structuredContent"]["id"],
        first["structuredContent"]["id"]
    );
    let bad = mcp_call(
        &mut mcp,
        12,
        "complete_task_with_reply",
        json!({"id":"task","call_id":"exact_finish","body":"changed"}),
    )
    .await;
    assert_eq!(bad["isError"], true);
    assert_eq!(
        mcp_call(&mut mcp, 13, "get_task", json!({"id":"task"})).await["isError"],
        true
    );
    assert_eq!(
        mcp_call(
            &mut mcp,
            14,
            "update_task_execution",
            json!({"id":"task","call_id":"later","heartbeat":true})
        )
        .await["isError"],
        true
    );
    assert_eq!(f.count("SELECT COUNT(*) FROM final_replies"), 0);
    assert_eq!(f.count("SELECT COUNT(*) FROM resource_leases"), 1);
    assert_eq!(f.count("SELECT COUNT(*) FROM task_operation_receipts"), 1);
    for field in ["runner", "dispatch", "fence", "secret"] {
        let mut bad = f.cap.clone();
        match field {
            "runner" => bad.runner_id = "foreign".into(),
            "dispatch" => bad.dispatch_id = "foreign".into(),
            "fence" => bad.fence += 1,
            _ => bad.secret = "0".repeat(64),
        }
        let mut other =
            mcp_context(hagency::task_client::Context::new(f.address, bad, "task".into()).unwrap())
                .await;
        let result = mcp_call(
            &mut other,
            1,
            "complete_task_with_reply",
            json!({"id":"task","call_id":"exact_finish","body":"Exact immutable final"}),
        )
        .await;
        assert_eq!(result["isError"], true, "{field}");
    }
    let output = first.to_string();
    assert!(!output.contains(&f.cap.secret));
    assert!(!output.contains("Exact immutable final"));
    assert!(!output.contains("!project"));
    f.close().await;
}
