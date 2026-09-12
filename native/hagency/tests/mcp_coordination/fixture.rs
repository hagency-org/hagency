use crate::common::*;
use hagency::{App, mcp::Session, task_client::Context};
use hagency_core::{messages::*, task_intents::*, tasks::*};
use hagency_store::{DomainRepository, DomainStore, EffectOutcome, Repository, Store};
use rmcp::{RoleClient, ServiceExt, model::CallToolRequestParams, service::RunningService};
use salvo::prelude::*;
use serde_json::{Value, json};
use std::{
    net::SocketAddr,
    process::Stdio,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    process::{Child, Command},
    task::JoinHandle,
};
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
pub fn dispatch(id: &str, session: &str, task: &str) -> DispatchInput {
    DispatchInput {
        id: id.into(),
        session_id: session.into(),
        task_id: Some(task.into()),
        resources: vec![],
        payload: json!({"instruction":"fixture scoped work"}),
    }
}
pub struct Fixture {
    pub root: tempfile::TempDir,
    pub domain: DomainStore,
    pub custody: Store,
    handle: salvo::server::ServerHandle,
    server: JoinHandle<()>,
    pub cap: RunnerCapability,
    pub parent: IntentResult,
    pub agents: Vec<String>,
    pub seq: u64,
    pub address: SocketAddr,
}
impl Fixture {
    pub async fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("state");
        let custody = Store::start(Repository::open(&state).unwrap(), 32).unwrap();
        let mut db = DomainRepository::open(&state).unwrap();
        db.register(&registration()).unwrap();
        let pool = resource("pool", "seat", 1000);
        db.put_resource(&pool).unwrap();
        let mut agents = vec![];
        for (id, name) in [("a", "小白"), ("b", "Edison"), ("c", "Other")] {
            let mut req = request(id, name, &pool, 100);
            if id == "c" {
                req.target_project_id = "other_project".into();
                req.target_room_id = "!other:example.test".into();
            }
            let p = proof(&req);
            let e = db.admit(&p, 1000).unwrap();
            db.approve(&format!("approve_{id}"), &p, 1000).unwrap();
            let effect = db.claim_effect().unwrap().unwrap();
            db.observe_effect(
                &effect.id,
                effect.fence,
                &EffectOutcome::Applied {
                    receipt: format!("fixture_{id}"),
                },
            )
            .unwrap();
            db.register_session(&SessionBinding {
                id: id.into(),
                engagement_id: e.id.clone(),
                room_id: req.target_room_id,
                thread_root: None,
            })
            .unwrap();
            agents.push(e.id);
        }
        let seq = db
            .ingest_message(
                &InboundMessage {
                    server_name: "example.test".into(),
                    room_id: "!project:example.test".into(),
                    event_id: "$root".into(),
                    sender_mxid: "@owner:example.test".into(),
                    thread_root: None,
                    body: "Coordinate verified work".into(),
                    kind: "m.text".into(),
                    origin_ts: now(),
                },
                &[MessageTarget {
                    session_id: "a".into(),
                    wake: true,
                }],
                now(),
            )
            .unwrap()
            .sequence;
        let parent = db
            .create_task_intent(
                &TaskIntent {
                    request_scope: "fixture".into(),
                    request_key: "source".into(),
                    assignee_engagement: agents[0].clone(),
                    root_sequence: seq,
                    input_sequences: vec![seq],
                    definition: TaskDefinition {
                        title: "Canonical parent".into(),
                        ..Default::default()
                    },
                },
                now(),
            )
            .unwrap();
        let c = db.claim_task_notice(now(), 1000).unwrap().unwrap();
        db.deliver_task_notice(
            &c.notice.id,
            &c.token,
            &NoticeDelivery {
                server_name: c.notice.server_name.clone(),
                room_id: c.notice.room_id.clone(),
                transaction_id: c.notice.transaction_id.clone(),
                event_id: "$fixture_parent_notice".into(),
            },
            now(),
        )
        .unwrap();
        db.enqueue_inbox_dispatch(
            &dispatch("creator", &parent.session_id, &parent.task_id),
            &[seq],
        )
        .unwrap();
        let cap = db
            .claim_dispatch("fixture_runner", now(), 120_000, 120_000, 16)
            .unwrap()
            .unwrap();
        db.start_dispatch(&cap, now()).unwrap();
        let domain = DomainStore::start(db, 32).unwrap();
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
            root,
            domain,
            custody,
            handle,
            server,
            cap,
            parent,
            agents,
            seq,
            address,
        }
    }
    pub async fn client(&self) -> Client {
        Client::new(self.address, &self.cap, &self.parent.task_id).await
    }
    pub async fn start(
        &self,
        id: &str,
        session: &str,
        task: &str,
        peer: Option<u64>,
    ) -> RunnerCapability {
        let input = dispatch(id, session, task);
        if let Some(seq) = peer {
            self.domain
                .enqueue_peer_dispatch(input, vec![seq])
                .await
                .unwrap();
        } else {
            self.domain.enqueue_dispatch(input).await.unwrap();
        }
        let cap = self
            .domain
            .claim_dispatch(format!("runner_{id}"), now(), 120_000, 120_000, 16)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(cap.dispatch_id, id);
        self.domain
            .start_dispatch(cap.clone(), now())
            .await
            .unwrap();
        cap
    }
    pub async fn independent(&self, id: &str, session: &str) -> (RunnerCapability, String) {
        let task = format!("task_{id}");
        self.domain
            .create_canonical_task(
                task.clone(),
                session.into(),
                "Independent fixture work".into(),
                now(),
            )
            .await
            .unwrap();
        (self.start(id, session, &task, None).await, task)
    }
    pub async fn close(self) {
        self.handle.stop_graceful(Some(Duration::from_secs(1)));
        self.server.await.unwrap();
        let state = self.root.path().join("state");
        let (result, snapshot) = self.domain.shutdown_observed().await;
        let outcome = format!("{:?}", snapshot.outcome);
        if let Err(error) = &result {
            if crate::stall::timed_out(&outcome) {
                let teardown = crate::stall::two_point_sample(&state);
                eprintln!("[shutdown-stall] {outcome}; snapshot {snapshot:?}; teardown {teardown}");
                crate::stall::record_if_timed_out(
                    &outcome,
                    "mcp_coordination::Fixture::close domain",
                    format!("{snapshot:?}"),
                    Some(teardown),
                );
            }
            panic!("domain shutdown failed ({outcome}): {error:?}; {snapshot:?}");
        }
        let (result, snapshot) = self.custody.shutdown_observed().await;
        let outcome = format!("{:?}", snapshot.outcome);
        if let Err(error) = &result {
            if crate::stall::timed_out(&outcome) {
                let teardown = crate::stall::two_point_sample(&state);
                eprintln!("[shutdown-stall] {outcome}; snapshot {snapshot:?}; teardown {teardown}");
                crate::stall::record_if_timed_out(
                    &outcome,
                    "mcp_coordination::Fixture::close custody",
                    format!("{snapshot:?}"),
                    Some(teardown),
                );
            }
            panic!("custody shutdown failed ({outcome}): {error:?}; {snapshot:?}");
        }
    }
    pub fn count(&self, table: &str) -> u64 {
        rusqlite::Connection::open(self.root.path().join("state/domain.sqlite3"))
            .unwrap()
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    }
}
pub struct Client {
    pub service: RunningService<RoleClient, ()>,
    child: Child,
}
impl Client {
    /// Diagnostic only: attribute a `TransportClosed` to "helper exited"
    /// (with its exit status and everything it wrote to stderr, otherwise
    /// only read at `close()`) rather than "pipe closed while alive".
    /// `try_wait` is non-blocking and does not reap a running child.
    pub async fn exit_evidence(&mut self) -> String {
        match self.child.try_wait() {
            Ok(Some(status)) => {
                let mut stderr = String::new();
                if let Some(mut pipe) = self.child.stderr.take() {
                    use tokio::io::AsyncReadExt;
                    let _ = pipe.read_to_string(&mut stderr).await;
                }
                format!("helper exited {status:?}; stderr={stderr:?}")
            }
            other => format!("helper try_wait={other:?}"),
        }
    }
    pub async fn new(address: SocketAddr, cap: &RunnerCapability, task: &str) -> Self {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_hagency"));
        cmd.arg("mcp")
            .env_clear()
            .env("HAGENCY_RUNNER_API_ADDR", address.to_string())
            .env(
                "HAGENCY_RUNNER_CAPABILITY",
                serde_json::to_string(cap).unwrap(),
            )
            .env("HAGENCY_TASK_ID", task)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(windows)]
        if let Some(root) = std::env::var_os("SystemRoot") {
            cmd.env("SystemRoot", root);
        }
        let mut child = cmd.spawn().unwrap();
        let service =
            ().serve((child.stdout.take().unwrap(), child.stdin.take().unwrap()))
                .await
                .unwrap();
        Self { service, child }
    }
    pub async fn call(&self, name: &str, args: Value) -> Value {
        let response = tokio::time::timeout(
            Duration::from_secs(8),
            self.service.call_tool(
                CallToolRequestParams::new(name.to_owned())
                    .with_arguments(args.as_object().unwrap().clone()),
            ),
        )
        .await
        .unwrap()
        .unwrap();
        serde_json::to_value(response).unwrap()
    }
    pub async fn ok(&self, name: &str, args: Value) -> Value {
        let v = self.call(name, args).await;
        assert_eq!(v["isError"], false, "{name}: {v}");
        v["structuredContent"].clone()
    }
    pub async fn refused(&self, name: &str, args: Value) {
        let v = self.call(name, args).await;
        assert_eq!(v["isError"], true, "{name}: {v}");
    }
    pub async fn close(self) {
        self.service.cancel().await.unwrap();
        let out = tokio::time::timeout(Duration::from_secs(5), self.child.wait_with_output())
            .await
            .unwrap()
            .unwrap();
        assert!(out.status.success(), "{:?}", out.stderr);
        assert!(out.stderr.is_empty());
    }
}
pub async fn direct(address: SocketAddr, cap: RunnerCapability, task: &str) -> Session {
    let mut s = Session::new(Context::new(address, cap, task.into()).unwrap());
    s.handle(&serde_json::to_vec(&json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"fixture","version":"1"}}})).unwrap()).await.unwrap();
    s.handle(br#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
        .await
        .unwrap();
    s
}
pub fn participant(group: &Value, agent: &str) -> String {
    group["participants"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["engagement_id"] == agent)
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .into()
}
pub async fn group(client: &Client, f: &Fixture) -> Value {
    client
        .ok(
            "open_conversation",
            json!({"call_id":"group","label":"协作测试","participant_engagements":[f.agents[1]]}),
        )
        .await["conversation"]
        .clone()
}
