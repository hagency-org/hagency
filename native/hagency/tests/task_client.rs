#[path = "../../hagency-store/tests/common/mod.rs"]
mod common;
use common::*;
use hagency::{
    App,
    task_client::{self, Command, Context, Error},
};
use hagency_core::tasks::*;
use hagency_store::{DomainRepository, DomainStore, EffectOutcome, Repository, Store};
use salvo::prelude::*;
use serde_json::json;
use std::{
    net::SocketAddr,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    task::JoinHandle,
};

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
fn capability() -> RunnerCapability {
    RunnerCapability {
        dispatch_id: "dispatch".into(),
        runner_id: "runner".into(),
        fence: 1,
        secret: "a".repeat(64),
    }
}
struct Fixture {
    _root: tempfile::TempDir,
    domain: DomainStore,
    custody: Store,
    handle: salvo::server::ServerHandle,
    server: JoinHandle<()>,
    cap: RunnerCapability,
    address: SocketAddr,
}
impl Fixture {
    async fn new(parked: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("state");
        let custody = Store::start(Repository::open(&state).unwrap(), 16).unwrap();
        let mut db = DomainRepository::open(&state).unwrap();
        db.register(&registration()).unwrap();
        let pool = resource("pool", "seat", 1000);
        db.put_resource(&pool).unwrap();
        let proof = proof(&request("request", "小白", &pool, 100));
        let e = db.admit(&proof, 1000).unwrap();
        db.approve("approve", &proof, 1000).unwrap();
        let effect = db.claim_effect().unwrap().unwrap();
        db.observe_effect(
            &effect.id,
            effect.fence,
            &EffectOutcome::Applied {
                receipt: "fixture_identity".into(),
            },
        )
        .unwrap();
        db.register_session(&SessionBinding {
            id: "session".into(),
            engagement_id: e.id,
            room_id: "!project:example.test".into(),
            thread_root: Some("$source".into()),
        })
        .unwrap();
        db.create_canonical_task("task", "session", "维护当前任务", now())
            .unwrap();
        db.create_canonical_task("other", "session", "Must not mutate", now())
            .unwrap();
        db.enqueue_dispatch(&DispatchInput {
            id: "dispatch".into(),
            session_id: "session".into(),
            task_id: Some("task".into()),
            resources: vec![],
            payload: json!({"instruction":"maintain"}),
        })
        .unwrap();
        let cap = db
            .claim_dispatch("runner", now(), 120_000, 120_000, 8)
            .unwrap()
            .unwrap();
        db.start_dispatch(&cap, now()).unwrap();
        if parked {
            db.park_dispatch(&cap, true, now()).unwrap();
        }
        let domain = DomainStore::start(db, 16).unwrap();
        let acceptor = TcpListener::new("127.0.0.1:0").try_bind().await.unwrap();
        let address = acceptor.local_addr().unwrap();
        let app = App::new(
            custody.clone(),
            b"fixture_operator_token_32_bytes_minimum",
            address,
        )
        .unwrap()
        .with_domain(domain.clone());
        let server = Server::new(acceptor);
        let handle = server.handle();
        let server = tokio::spawn(async move {
            server.try_serve(app.router()).await.unwrap();
        });
        Self {
            _root: root,
            domain,
            custody,
            handle,
            server,
            cap,
            address,
        }
    }
    fn context(&self) -> Context {
        Context::new(self.address, self.cap.clone(), "task".into()).unwrap()
    }
    async fn close(self) {
        self.handle.stop_graceful(Some(Duration::from_secs(1)));
        self.server.await.unwrap();
        self.domain.shutdown().await.unwrap();
        self.custody.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn native_task_client_lifecycle() {
    let f = Fixture::new(false).await;
    let c = f.context();
    let limit = Duration::from_secs(2);
    assert_eq!(
        task_client::run(&c, &Command::Get, None, limit)
            .await
            .unwrap()
            .task
            .status,
        TaskState::InProgress
    );
    let original = task_client::run(&c, &Command::Heartbeat, Some("heartbeat"), limit)
        .await
        .unwrap();
    assert!(original.task.heartbeat_at.is_some());
    assert!(!original.replayed);
    let replay = task_client::run(&c, &Command::Heartbeat, Some("heartbeat"), limit)
        .await
        .unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.task.heartbeat_at, original.task.heartbeat_at);
    assert!(matches!(
        task_client::run(
            &c,
            &Command::Comment {
                text: "different".into()
            },
            Some("heartbeat"),
            limit
        )
        .await,
        Err(Error::Refused(409))
    ));
    let waiting = task_client::run(
        &c,
        &Command::Wait {
            reason: "waiting for source".into(),
            until: "2030-01-01T00:00:00Z".into(),
        },
        Some("wait"),
        limit,
    )
    .await
    .unwrap();
    assert_eq!(waiting.task.status, TaskState::Blocked);
    let resumed = task_client::run(&c, &Command::Resume, Some("resume"), limit)
        .await
        .unwrap();
    assert_eq!(resumed.task.status, TaskState::InProgress);
    assert!(resumed.task.waiting_reason.is_none());
    task_client::run(
        &c,
        &Command::Comment {
            text: "检查已通过".into(),
        },
        Some("comment"),
        limit,
    )
    .await
    .unwrap();
    let done = task_client::run(&c, &Command::Done, Some("done"), limit)
        .await
        .unwrap();
    assert_eq!(done.task.status, TaskState::Done);
    let replay = task_client::run(&c, &Command::Done, Some("done"), limit)
        .await
        .unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.task.execution_epoch, done.task.execution_epoch);
    assert!(matches!(
        task_client::run(&c, &Command::Heartbeat, Some("late"), limit).await,
        Err(Error::Refused(409))
    ));
    let stored = f
        .domain
        .runner_command(f.cap.clone(), RunnerCommand::Task { id: "task".into() })
        .await
        .unwrap();
    assert_eq!(stored["status"], "done");
    f.close().await;
}

#[tokio::test]
async fn native_task_client_scope() {
    let f = Fixture::new(false).await;
    for cap in [
        RunnerCapability {
            secret: "b".repeat(64),
            ..f.cap.clone()
        },
        RunnerCapability {
            fence: f.cap.fence + 1,
            ..f.cap.clone()
        },
        RunnerCapability {
            dispatch_id: "other".into(),
            ..f.cap.clone()
        },
    ] {
        let c = Context::new(f.address, cap, "task".into()).unwrap();
        assert!(matches!(
            task_client::run(&c, &Command::Done, Some("forged"), Duration::from_secs(2)).await,
            Err(Error::Refused(401))
        ));
    }
    let c = Context::new(f.address, f.cap.clone(), "other".into()).unwrap();
    assert!(matches!(
        task_client::run(&c, &Command::Done, Some("other"), Duration::from_secs(2)).await,
        Err(Error::Refused(403))
    ));
    assert!(matches!(
        task_client::run(&f.context(), &Command::Done, None, Duration::from_secs(2)).await,
        Err(Error::Invalid)
    ));
    for address in ["192.0.2.1:13300", "0.0.0.0:13300", "127.0.0.1:0"] {
        assert!(Context::new(address.parse().unwrap(), capability(), "task".into()).is_err());
    }
    assert!(Context::new(f.address, capability(), "../other".into()).is_err());
    f.close().await;
    let f = Fixture::new(true).await;
    assert!(matches!(
        task_client::run(
            &f.context(),
            &Command::Done,
            Some("parked"),
            Duration::from_secs(2)
        )
        .await,
        Err(Error::Refused(401))
    ));
    f.close().await;
}

async fn fake(response: Option<String>, hold_open: bool) -> (Context, JoinHandle<Vec<u8>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut buffer = [0; 1024];
        loop {
            let n = stream.read(&mut buffer).await.unwrap();
            if n == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..n]);
            assert!(request.len() <= 32 * 1024);
            if let Some(end) = request.windows(4).position(|v| v == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&request[..end]).to_ascii_lowercase();
                let length = headers
                    .lines()
                    .find_map(|line| {
                        line.strip_prefix("content-length:")
                            .and_then(|v| v.trim().parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                if request.len() >= end + 4 + length {
                    break;
                }
            }
        }
        if let Some(response) = response {
            let _ = stream.write_all(response.as_bytes()).await;
        }
        if hold_open {
            assert_eq!(
                tokio::time::timeout(Duration::from_secs(2), stream.read(&mut buffer))
                    .await
                    .unwrap()
                    .unwrap(),
                0
            );
        }
        // No redirect, error or unknown response causes an automatic reconnect.
        assert!(
            tokio::time::timeout(Duration::from_millis(30), listener.accept())
                .await
                .is_err()
        );
        request
    });
    (
        Context::new(address, capability(), "task".into()).unwrap(),
        server,
    )
}

#[tokio::test]
async fn native_task_client_transport() {
    for (response,expected) in [
        (Some("HTTP/1.1 307 Temporary Redirect\r\nLocation: http://127.0.0.1:1/steal\r\nContent-Length: 0\r\n\r\n".into()),Error::Refused(307)),
        (Some("HTTP/1.1 500 Error\r\nContent-Length: 14\r\n\r\nprivate_canary".into()),Error::Unknown),
        (Some("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 999999\r\n\r\n".into()),Error::Unknown),
        (Some("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 20\r\n\r\n{}".into()),Error::Unknown),
        (Some("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n{}".into()),Error::Unknown),
        (None,Error::Unknown),
    ] {
        let hold=response.is_none();
        let(c,server)=fake(response,hold).await;
        let error=task_client::run(&c,&Command::Heartbeat,Some("stable"),Duration::from_millis(150)).await.err().unwrap();
        assert_eq!(error,expected);assert!(!error.to_string().contains("private_canary"));assert!(!error.to_string().contains(&"a".repeat(64)));
        let request=server.await.unwrap();let request=String::from_utf8(request).unwrap();
        assert!(request.contains("x-hagency-dispatch: dispatch"));assert!(request.contains("\"call_id\":\"stable\""));
    }
    let chunk = format!("4000\r\n{}\r\n", "a".repeat(16 * 1024));
    for (response, hold_open) in [
        (
            format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n{}0\r\n\r\n",
                chunk.repeat(5)
            ),
            false,
        ),
        (
            format!(
                "HTTP/1.1 200 OK\r\n{}Content-Length: 0\r\n\r\n",
                "X-Extra: data\r\n".repeat(40)
            ),
            false,
        ),
        (
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 100\r\n\r\n{"
                .into(),
            true,
        ),
    ] {
        let (context, server) = fake(Some(response), hold_open).await;
        assert!(matches!(
            task_client::run(
                &context,
                &Command::Done,
                Some("bounded"),
                Duration::from_millis(150)
            )
            .await,
            Err(Error::Unknown)
        ));
        server.await.unwrap();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn native_task_client_cli() {
    let f = Fixture::new(false).await;
    let address = f.address.to_string();
    let capability = serde_json::to_string(&f.cap).unwrap();
    let output = tokio::task::spawn_blocking(move || {
        isolated_cli()
            .env("HAGENCY_RUNNER_API_ADDR", address)
            .env("HAGENCY_RUNNER_CAPABILITY", capability)
            .env("HAGENCY_TASK_ID", "task")
            .args(["task", "--call-id", "cli-heartbeat", "heartbeat"])
            .output()
            .unwrap()
    })
    .await
    .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["task"]["id"], "task");
    assert!(value["task"]["heartbeat_at"].is_number());
    assert!(!String::from_utf8_lossy(&output.stdout).contains(&f.cap.secret));
    let output = tokio::task::spawn_blocking(|| {
        isolated_cli()
            .env("HAGENCY_RUNNER_CAPABILITY", "private_context_canary")
            .args(["task", "get"])
            .output()
            .unwrap()
    })
    .await
    .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(!String::from_utf8_lossy(&output.stderr).contains("private_context_canary"));
    f.close().await;
}

fn isolated_cli() -> std::process::Command {
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_hagency"));
    command.env_clear();
    // Windows socket initialization needs its system directory, as do our
    // existing owned-process fixtures. No proxy, PATH or user credentials pass.
    #[cfg(windows)]
    command.env(
        "SystemRoot",
        std::env::var_os("SystemRoot").expect("Windows system root"),
    );
    command
}

#[path = "task_client/mcp.rs"]
mod mcp;
