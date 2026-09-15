use hagency_core::{
    ingress::{VerifiedNoticeClaim, VerifiedTaskRequest},
    replies::*,
    task_intents::{IntentResult, TaskDefinition},
    tasks::*,
};
use hagency_execution::Host;
use hagency_matrix::{CancellationToken, Collector, HostConfig, HostIntakePlan, HostRoom};
use salvo::Listener;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[path = "../../../hagency-matrix/tests/common/mod.rs"]
pub mod common;
pub const ROOM: &str = "!project:example.test";
pub const INPUT: &str = "Please verify the native workflow. 中文 body stays intact.";
pub const FINAL: &str = "Verified **native MCP final result**";

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
pub fn room(encrypted: bool) -> Value {
    let mut state = common::state();
    if !encrypted {
        state
            .as_array_mut()
            .unwrap()
            .retain(|event| event["type"] != "m.room.encryption");
    }
    state
}
pub fn event() -> Value {
    json!({"event_id":"$question","sender":"@owner:example.test","type":"m.room.message","origin_server_ts":now(),"content":{"msgtype":"m.text","body":INPUT,"m.mentions":{"user_ids":["@worker:example.test"]}}})
}
pub fn sync(event: Value) -> Value {
    json!({"next_batch":"question","rooms":{"join":{ROOM:{"timeline":{"limited":false,"events":[event]},"state":{"events":[]}}}},"to_device":{"events":[]}})
}
pub struct Workflow {
    pub f: common::Fixture,
    pub collector: Collector,
    pub work: PathBuf,
}
impl Workflow {
    pub async fn ready(private: bool) -> (Self, common::Fake) {
        let f = common::Fixture::new();
        let mut fake = common::Fake::start(true).await;
        let work = f.root.path().join("固定 工作目录");
        hagency_store::private::directory(&work).unwrap();
        let work = work.canonicalize().unwrap();
        f.store.register_workspace("work".into()).await.unwrap();
        let config = HostConfig::new(
            f.identity.clone(),
            &fake.endpoint,
            common::TOKEN,
            f.root.path().join("sdk"),
            [42; 32],
            vec![HostRoom {
                room_id: ROOM.into(),
                generation: 1,
                privacy: if private {
                    RoomPrivacy::Direct {
                        human_mxid: "@owner:example.test".into(),
                    }
                } else {
                    RoomPrivacy::Group {}
                },
            }],
            common::load_limits(),
        )
        .unwrap()
        .with_root_pem(include_bytes!(
            "../../../hagency-matrix/tests/fixtures/ca.pem"
        ))
        .unwrap();
        let collector = Collector::new(config, f.store.clone()).unwrap();
        let cancel = CancellationToken::new();
        let (result, ()) = common::scripted(collector.collect(&cancel), async {
            fake.next().await.json(200, common::who());
            fake.next().await.json(200, common::sync("boot"));
            fake.next().await.json(200, room(private));
        })
        .await;
        result.unwrap();
        f.store
            .resolve_verified_matrix_session(SessionBinding {
                id: "root".into(),
                engagement_id: f.identity.transport.engagement_id.clone(),
                room_id: ROOM.into(),
                thread_root: None,
            })
            .await
            .unwrap();
        (Self { f, collector, work }, fake)
    }
    pub async fn pending(
        &self,
        fake: &mut common::Fake,
    ) -> (IntentResult, VerifiedNoticeClaim, u64) {
        let cancel = CancellationToken::new();
        let (result, ()) = common::scripted(
            self.collector
                .intake(HostIntakePlan::new(vec!["root".into()]).unwrap(), &cancel),
            async {
                fake.next().await.json(200, common::who());
                fake.next().await.json(200, sync(event()));
                fake.next().await.json(200, room(false));
            },
        )
        .await;
        assert_eq!(result.unwrap().admitted, 1);
        let inbox = self
            .f
            .store
            .inbox("root".into(), 0, 10, None)
            .await
            .unwrap();
        assert_eq!(inbox.len(), 1);
        assert!(inbox[0].wake);
        assert_eq!(inbox[0].message.event_id, "$question");
        assert_eq!(inbox[0].message.sender_mxid, "@owner:example.test");
        assert_eq!(inbox[0].message.body, INPUT);
        assert_eq!(inbox[0].message.thread_root, None);
        let seq = inbox[0].message.sequence;
        let scope = self
            .f
            .store
            .matrix_ingress_scope("root".into())
            .await
            .unwrap();
        let intent = self
            .f
            .store
            .create_verified_task_intent(VerifiedTaskRequest {
                scope,
                request_key: "workflow_request".into(),
                source_sequence: seq,
                definition: TaskDefinition {
                    title: "Verify native workflow".into(),
                    ..Default::default()
                },
            })
            .await
            .unwrap();
        assert_eq!(intent.activation, "pending");
        assert_ne!(intent.session_id, "root");
        self.assert_inactive(&intent, seq).await;
        let claim = self
            .f
            .store
            .claim_verified_task_notice(60_000)
            .await
            .unwrap()
            .unwrap();
        (intent, claim, seq)
    }
    pub fn input(&self, intent: &IntentResult) -> DispatchInput {
        DispatchInput {
            id: "workflow_dispatch".into(),
            session_id: intent.session_id.clone(),
            task_id: Some(intent.task_id.clone()),
            resources: vec![ResourceLease {
                id: "work".into(),
                exclusive: true,
            }],
            payload: json!({"instruction":"Use the authenticated frozen inbox as task data."}),
        }
    }
    pub async fn assert_inactive(&self, intent: &IntentResult, seq: u64) {
        assert_eq!(self.intent_state(&intent.task_id), ("pending".into(), None));
        assert!(
            self.f
                .store
                .inbox(intent.session_id.clone(), 0, 10, None)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            self.f
                .store
                .enqueue_inbox_dispatch(self.input(intent), vec![seq])
                .await
                .is_err()
        );
        assert!(
            self.f
                .store
                .enqueue_dispatch(self.input(intent))
                .await
                .is_err()
        );
        assert!(
            self.f
                .store
                .claim_dispatch("owned_host".into(), now(), 60_000, 60_000, 1)
                .await
                .unwrap()
                .is_none()
        );
        self.assert_no_execution();
    }
    pub fn assert_no_execution(&self) {
        assert_eq!(self.count("SELECT COUNT(*) FROM runner_dispatches"), 0);
        assert_eq!(self.count("SELECT COUNT(*) FROM owned_task_completions"), 0);
        assert_eq!(self.count("SELECT COUNT(*) FROM final_replies"), 0);
        assert_eq!(self.count("SELECT COUNT(*) FROM resource_leases"), 0);
        assert!(!self.work.join("owned-mcp.requests").exists());
    }
    pub fn sql(&self) -> rusqlite::Connection {
        rusqlite::Connection::open_with_flags(
            self.f.root.path().join("domain/domain.sqlite3"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap()
    }
    pub fn count(&self, query: &str) -> u64 {
        self.sql().query_row(query, [], |r| r.get(0)).unwrap()
    }
    pub fn intent_state(&self, task: &str) -> (String, Option<String>) {
        self.sql()
            .query_row(
                "SELECT state,anchor_event_id FROM task_intents WHERE task_id=?1",
                [task],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap()
    }
    pub fn task(&self, id: &str) -> Task {
        serde_json::from_str(
            &self
                .sql()
                .query_row(
                    "SELECT config FROM canonical_tasks WHERE id=?1",
                    [id],
                    |r| r.get::<_, String>(0),
                )
                .unwrap(),
        )
        .unwrap()
    }
    pub fn reply_state(&self, id: &str) -> String {
        self.sql()
            .query_row("SELECT state FROM final_replies WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .unwrap()
    }
    pub fn host(&self, address: std::net::SocketAddr) -> Host {
        #[allow(unused_mut)]
        let mut environment = BTreeMap::from([
            ("PATH".into(), "".into()),
            ("HAGENCY_OFFLINE_MODE".into(), "finish".into()),
        ]);
        #[cfg(windows)]
        environment.insert("SystemRoot".into(), std::env::var_os("SystemRoot").unwrap());
        let binary = PathBuf::from(env!("CARGO_BIN_EXE_hagency-owned-mcp-probe"));
        Host::new(
            binary.clone(),
            binary,
            environment,
            BTreeMap::from([("work".into(), self.work.clone())]),
        )
        .unwrap()
        .with_task_helper(env!("CARGO_BIN_EXE_hagency").into(), address)
        .unwrap()
    }
    pub async fn close(self) {
        self.collector.close().await.unwrap();
        let state = self.f.root.path().join("state");
        let (result, snapshot) = self.f.store.shutdown_observed().await;
        let outcome = format!("{:?}", snapshot.outcome);
        if let Err(error) = &result {
            if common::stall::timed_out(&outcome) {
                let teardown = common::stall::two_point_sample(&state);
                eprintln!("[shutdown-stall] {outcome}; snapshot {snapshot:?}; teardown {teardown}");
                common::stall::record_if_timed_out(
                    &outcome,
                    "owned_matrix::Workflow::close domain",
                    format!("{snapshot:?}"),
                    Some(teardown),
                );
            }
            panic!("domain shutdown failed ({outcome}): {error:?}; {snapshot:?}");
        }
    }
}
pub async fn outgoing(fake: &mut common::Fake, txn: &str) -> common::Request {
    for _ in 0..2 {
        let r = fake.next().await;
        assert_eq!(r.method, "GET");
        assert_eq!(r.target, "/_matrix/client/v3/account/whoami");
        assert_eq!(
            r.headers["authorization"],
            format!("Bearer {}", common::TOKEN)
        );
        r.json(200, common::who());
        let r = fake.next().await;
        assert_eq!(r.target, format!("/_matrix/client/v3/rooms/{ROOM}/state"));
        r.json(200, room(false));
    }
    let r = fake.next().await;
    assert_eq!(r.method, "PUT");
    assert_eq!(
        r.target,
        format!("/_matrix/client/v3/rooms/{ROOM}/send/m.room.message/{txn}")
    );
    assert_eq!(
        r.headers["authorization"],
        format!("Bearer {}", common::TOKEN)
    );
    r
}
pub struct RunnerApi {
    custody: hagency_store::Store,
    handle: salvo::server::ServerHandle,
    server: tokio::task::JoinHandle<()>,
    pub address: std::net::SocketAddr,
}
impl RunnerApi {
    pub async fn start(w: &Workflow) -> Self {
        let custody = hagency_store::Store::start(
            hagency_store::Repository::open(&w.f.root.path().join("domain")).unwrap(),
            16,
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
        .with_domain(w.f.store.clone());
        let server = salvo::Server::new(acceptor);
        let handle = server.handle();
        let server = tokio::spawn(async move {
            server.try_serve(app.router()).await.unwrap();
        });
        Self {
            custody,
            handle,
            server,
            address,
        }
    }
    pub async fn close(self) {
        self.handle.stop_graceful(Some(Duration::from_secs(1)));
        self.server.await.unwrap();
        let (result, snapshot) = self.custody.shutdown_observed().await;
        let outcome = format!("{:?}", snapshot.outcome);
        if let Err(error) = &result {
            if common::stall::timed_out(&outcome) {
                eprintln!("[shutdown-stall] {outcome}; snapshot {snapshot:?}");
                common::stall::record_if_timed_out(
                    &outcome,
                    "owned_matrix::RunnerApi::close custody",
                    format!("{snapshot:?}"),
                    None,
                );
            }
            panic!("custody shutdown failed ({outcome}): {error:?}; {snapshot:?}");
        }
    }
}
