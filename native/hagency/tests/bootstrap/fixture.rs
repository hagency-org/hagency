#[path = "../../../hagency-matrix/tests/common/mod.rs"]
pub mod common;
use hagency_core::{replies::*, tasks::*};
use hagency_store::{DomainRepository, EffectOutcome, private};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    net::SocketAddr,
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Bounds fixture waits that must outlast the operation budget under load —
/// the sibling fixtures (file_service, received_files) name the same value
/// `STARTUP_WATCHDOG`. The bootstrap fixture's own capability-poll bound and
/// the approval scenario's delivery drain both use it, so the two sides can
/// never disagree about how long a loaded host may take.
pub const STARTUP_WATCHDOG: Duration = Duration::from_secs(15);

pub struct Running(Child);
impl Running {
    pub fn from_child(child: Child) -> Self {
        Self(child)
    }
}
#[cfg(unix)]
impl Running {
    pub fn request_shutdown(&self) {
        let result = Command::new("/bin/kill")
            .args(["-TERM", &self.0.id().to_string()])
            .status()
            .unwrap();
        assert!(result.success());
    }
    pub fn still_owned(&mut self) -> bool {
        self.0.try_wait().unwrap().is_none()
    }
    pub async fn exited(&mut self) {
        let until = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = self.0.try_wait().unwrap() {
                assert!(status.success());
                return;
            }
            assert!(
                tokio::time::Instant::now() < until,
                "native shutdown did not finish"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
pub struct Fixture {
    pub root: tempfile::TempDir,
    pub state_dir: PathBuf,
    pub work: PathBuf,
    pub fake: common::Fake,
    pub address: SocketAddr,
    /// Requests the generic responder has answered (`serve_until`).
    #[cfg_attr(not(any(target_os = "linux", target_os = "macos")), allow(dead_code))]
    served: u64,
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
impl Fixture {
    pub async fn new(fenced: bool) -> Self {
        Self::with_account(fenced, false).await
    }
    pub async fn with_account(fenced: bool, managed: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let state_dir = root.path().join("state");
        let init = Command::new(env!("CARGO_BIN_EXE_hagency"))
            .args(["init", "--state-dir"])
            .arg(&state_dir)
            .output()
            .unwrap();
        assert!(
            init.status.success(),
            "init failed: {}",
            String::from_utf8_lossy(&init.stderr)
        );
        let work = root.path().join("工作目录");
        private::directory(&work).unwrap();
        let work = work.canonicalize().unwrap();
        let mut db = DomainRepository::open(&state_dir).unwrap();
        db.register(&common::domain::registration()).unwrap();
        let mut account_id = None;
        let resource = if managed {
            let reserved = db.reserve_account(hagency_store::ACCOUNT_PROFILE).unwrap();
            let choice = db.materialize_account(&reserved.id).unwrap();
            fs::write(
                state_dir.join(&choice.id).join("fixture-account-marker"),
                "bootstrap-selected",
            )
            .unwrap();
            fs::write(work.join("account-probe.required"), b"required").unwrap();
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
            // MA-S2 (ADR-053 amendment): the readiness gate consumes the bound
            // account only when its login fact (MA-S1, migration 028) is
            // observed and unexpired. A managed bootstrap over an unobserved
            // account parks instead of running. Record the fact through the
            // store's own readiness path — exactly what a real deployment
            // writes when its login child exits — never a gate bypass.
            let attempt = db.begin_account_login(choice.id.as_str(), now()).unwrap();
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
            account_id = Some(choice.id);
            db.resource_configuration(&result.resource_id).unwrap()
        } else {
            let resource = common::domain::resource("pool", "seat", 1000);
            db.put_resource(&resource).unwrap();
            resource
        };
        let request = common::domain::request("bootstrap", "Worker", &resource, 100);
        let mut observation = common::domain::observation(&request);
        observation.observed_at_ms = now();
        let proof = hagency_core::authority::verify_request(
            &common::domain::registration(),
            request,
            observation,
        )
        .unwrap();
        let e = db.admit(&proof, now()).unwrap();
        db.approve("approve", &proof, now()).unwrap();
        let effect = db.claim_effect().unwrap().unwrap();
        db.observe_effect(
            &effect.id,
            effect.fence,
            &EffectOutcome::Applied {
                receipt: "fixture provision".into(),
            },
        )
        .unwrap();
        let transport = MatrixTransportObservation {
            engagement_id: e.id.clone(),
            registration_generation: 1,
            generation: 1,
            sender_mxid: "@worker:example.test".into(),
            device_id: "DEVICE_1".into(),
        };
        db.observe_matrix_transport(&transport, now()).unwrap();
        db.observe_matrix_room(
            &MatrixRoomObservation {
                engagement_id: e.id.clone(),
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
            now(),
        )
        .unwrap();
        db.resolve_verified_matrix_session(
            &SessionBinding {
                id: "session".into(),
                engagement_id: e.id.clone(),
                room_id: "!project:example.test".into(),
                thread_root: Some("$task_thread".into()),
            },
            now(),
        )
        .unwrap();
        db.create_canonical_task("task", "session", "Native bootstrap task", now())
            .unwrap();
        db.register_workspace("work").unwrap();
        db.enqueue_dispatch(&DispatchInput{id:"dispatch".into(),session_id:"session".into(),task_id:Some("task".into()),resources:vec![ResourceLease{id:"work".into(),exclusive:true}],payload:json!({"instruction":"Read and heartbeat the assigned task using native MCP."})}).unwrap();
        if fenced {
            db.invalidate_matrix_transport(
                &MatrixTransportInvalidation {
                    expected: transport.clone(),
                    reason: "fixture authentic negative evidence".into(),
                },
                now(),
            )
            .unwrap();
        }
        drop(db); // No issued cap, Started restoration, SQL availability or handoff.
        let fake = common::Fake::start(true).await;
        let executable = PathBuf::from(env!("CARGO_BIN_EXE_hagency-owned-mcp-probe"))
            .canonicalize()
            .unwrap();
        let executable_sha256: String = Sha256::digest(fs::read(&executable).unwrap())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let config = json!({"profile":"codex_app_server_development_v1","managed_account":account_id,"executable":executable,"executable_sha256":executable_sha256,"workspaces":{"work":work},"file_limit":4194304,"operation_ms":10000,"response_ms":1500,
            "matrix":{"origin":fake.endpoint,"server_name":"example.test","registration_fingerprint":"a".repeat(64),"engagement_id":e.id,"registration_generation":1,"transport_generation":1,"sender_mxid":"@worker:example.test","device_id":"DEVICE_1","rooms":[{"id":"!project:example.test","generation":1,"privacy":{"kind":"group"}}]}});
        private::write_new(
            &state_dir.join("development-driver.json"),
            &serde_json::to_vec(&config).unwrap(),
        )
        .unwrap();
        private::write_new(
            &state_dir.join("matrix.access_token"),
            common::TOKEN.as_bytes(),
        )
        .unwrap();
        private::write_new(&state_dir.join("matrix.sdk_key"), &[42; 32]).unwrap();
        private::write_new(
            &state_dir.join("matrix.ca.pem"),
            include_bytes!("../../../hagency-matrix/tests/fixtures/ca.pem"),
        )
        .unwrap();
        let reserve = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = reserve.local_addr().unwrap();
        drop(reserve);
        Self {
            root,
            state_dir,
            work,
            fake,
            address,
            served: 0,
        }
    }
    pub fn command(&self, enabled: bool) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_hagency"));
        command
            .args(["serve", "--state-dir"])
            .arg(&self.state_dir)
            .args(["--listen", &self.address.to_string()]);
        if enabled {
            command.arg("--development-driver");
        }
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        command
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    pub fn configure_continuous(&self) {
        let source = self.state_dir.join("development-driver.json");
        let mut config: Value = serde_json::from_slice(&fs::read(&source).unwrap()).unwrap();
        config["profile"] = json!("codex_app_server_agent_v1");
        private::write_new(
            &self.state_dir.join("agent-driver.json"),
            &serde_json::to_vec(&config).unwrap(),
        )
        .unwrap();
    }
    #[cfg(target_os = "linux")]
    pub fn enqueue_second(&self) {
        let mut db = DomainRepository::open(&self.state_dir).unwrap();
        db.create_canonical_task("task-2", "session", "Second native task", now())
            .unwrap();
        db.enqueue_dispatch(&DispatchInput {
            id: "dispatch-2".into(),
            session_id: "session".into(),
            task_id: Some("task-2".into()),
            resources: vec![ResourceLease {
                id: "work".into(),
                exclusive: true,
            }],
            payload: json!({"instruction":"Run the second task through the retained native host."}),
        })
        .unwrap();
    }
    #[cfg(target_os = "linux")]
    pub fn launch_continuous(&self) -> Running {
        let file = private::open(&self.root.path().join("native.stderr"), true).unwrap();
        let mut command = self.command(false);
        command.arg("--agent-driver");
        Running(command.stderr(Stdio::from(file)).spawn().unwrap())
    }
    pub fn launch(&self, enabled: bool) -> Running {
        let file = private::open(&self.root.path().join("native.stderr"), true).unwrap();
        Running(
            self.command(enabled)
                .stderr(Stdio::from(file))
                .spawn()
                .unwrap(),
        )
    }
    pub fn sql(&self) -> rusqlite::Connection {
        rusqlite::Connection::open(self.state_dir.join("domain.sqlite3")).unwrap()
    }
    pub fn state(&self) -> String {
        self.sql()
            .query_row(
                "SELECT state FROM runner_dispatches WHERE id='dispatch'",
                [],
                |r| r.get(0),
            )
            .unwrap()
    }
    pub fn attempts(&self) -> u64 {
        self.sql()
            .query_row("SELECT COUNT(*) FROM runner_attempts", [], |r| r.get(0))
            .unwrap()
    }
    #[cfg(target_os = "linux")]
    pub async fn wait_attempts(&self, expected: u64) {
        let until = tokio::time::Instant::now() + Duration::from_secs(20);
        while self.attempts() < expected {
            assert!(
                tokio::time::Instant::now() < until,
                "native driver did not reach {expected} attempts"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
    pub async fn capabilities(&self) -> Value {
        let until = tokio::time::Instant::now() + STARTUP_WATCHDOG;
        loop {
            if let Ok(mut stream) = tokio::net::TcpStream::connect(self.address).await {
                let token = String::from_utf8(
                    private::read_secret(&self.state_dir.join("operator.token")).unwrap(),
                )
                .unwrap();
                stream.write_all(format!("GET /api/native/v1/capabilities HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {}\r\nConnection: close\r\n\r\n",self.address,token).as_bytes()).await.unwrap();
                let mut bytes = Vec::new();
                tokio::time::timeout(Duration::from_secs(2), stream.read_to_end(&mut bytes))
                    .await
                    .unwrap()
                    .unwrap();
                let response = String::from_utf8(bytes).unwrap();
                if response.starts_with("HTTP/1.1 200") {
                    return serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1)
                        .unwrap();
                }
            }
            assert!(
                tokio::time::Instant::now() < until,
                "native bootstrap did not serve capabilities: {}",
                {
                    use std::io::Read;
                    let mut bytes = Vec::new();
                    if let Ok(file) = fs::File::open(self.root.path().join("native.stderr")) {
                        file.take(8192).read_to_end(&mut bytes).unwrap();
                    }
                    String::from_utf8_lossy(&bytes).into_owned()
                }
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
    pub async fn wait_result(&self) -> Value {
        let until = tokio::time::Instant::now() + Duration::from_secs(20);
        loop {
            let status = self.capabilities().await["development_execution"].clone();
            if matches!(
                status["state"].as_str(),
                Some("completed" | "unavailable" | "outcome_unknown" | "no_work")
            ) {
                return status;
            }
            assert!(
                tokio::time::Instant::now() < until,
                "native driver did not report its result"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}
/// ADR-182 support: a second session of the same agent, the operator's
/// console access, and a responder that keeps the fake homeserver answering
/// while a continuous worker keeps polling.
#[cfg(any(target_os = "linux", target_os = "macos"))]
impl Fixture {
    fn driver_config(&self) -> Value {
        serde_json::from_slice(&fs::read(self.state_dir.join("development-driver.json")).unwrap())
            .unwrap()
    }
    pub fn engagement(&self) -> String {
        self.driver_config()["matrix"]["engagement_id"]
            .as_str()
            .unwrap()
            .to_owned()
    }
    pub fn second_work(&self) -> PathBuf {
        self.root.path().join("work-2").canonicalize().unwrap()
    }
    /// A second session of the same agent in the same project room (its own
    /// thread), with its own workspace, task and queued dispatch. Called
    /// before `configure_continuous`, which copies the workspace map.
    pub fn seed_second_session(&self) {
        assert!(
            !self.state_dir.join("agent-driver.json").exists(),
            "seed the second session before configuring the continuous driver"
        );
        let work = self.root.path().join("work-2");
        private::directory(&work).unwrap();
        let work = work.canonicalize().unwrap();
        let mut db = DomainRepository::open(&self.state_dir).unwrap();
        db.resolve_verified_matrix_session(
            &SessionBinding {
                id: "session-2".into(),
                engagement_id: self.engagement(),
                room_id: "!project:example.test".into(),
                thread_root: Some("$task_thread_2".into()),
            },
            now(),
        )
        .unwrap();
        db.create_canonical_task("task-3", "session-2", "Third native task", now())
            .unwrap();
        db.register_workspace("work-2").unwrap();
        db.enqueue_dispatch(&DispatchInput {
            id: "dispatch-3".into(),
            session_id: "session-2".into(),
            task_id: Some("task-3".into()),
            resources: vec![ResourceLease {
                id: "work-2".into(),
                exclusive: true,
            }],
            payload: json!({"instruction":"Run the other session's task through the same worker."}),
        })
        .unwrap();
        drop(db);
        let mut config = self.driver_config();
        config["workspaces"]["work-2"] = json!(work);
        fs::write(
            self.state_dir.join("development-driver.json"),
            serde_json::to_vec(&config).unwrap(),
        )
        .unwrap();
    }
    /// An offline console bundle, enough for the operator routes to serve.
    pub fn console_assets(&self) -> PathBuf {
        let assets = self.root.path().join("console");
        private::directory(&assets).unwrap();
        let bytes = b"<!doctype html><html><body>offline console fixture</body></html>";
        let mut manifest = Vec::new();
        for page in ["usage", "engagements"] {
            fs::create_dir(assets.join(page)).unwrap();
            private::write_new(&assets.join(page).join("index.html"), bytes).unwrap();
            manifest.push(json!({"path":format!("{page}/index.html"),"size":bytes.len(),"sha256":format!("{:x}",Sha256::digest(bytes)),"mime":"text/html; charset=utf-8"}));
        }
        private::write_new(
            &assets.join("manifest.json"),
            json!({"version":1,"assets":manifest})
                .to_string()
                .as_bytes(),
        )
        .unwrap();
        assets.canonicalize().unwrap()
    }
    /// The continuous (agent) driver, with the console routes when given assets.
    pub fn launch_agent_driver(&self, assets: Option<&std::path::Path>) -> Running {
        let file = private::open(&self.root.path().join("native.stderr"), true).unwrap();
        let mut command = self.command(false);
        command.arg("--agent-driver");
        if let Some(assets) = assets {
            command.arg("--console-assets").arg(assets);
        }
        Running(command.stderr(Stdio::from(file)).spawn().unwrap())
    }
    /// The operator's console session: a lifecycle ticket exchanged for the
    /// console cookie, exactly as the browser does it.
    pub async fn operator(&self) -> Operator {
        let link = hagency::console::client::lifecycle_access(&self.state_dir, self.address)
            .await
            .unwrap();
        let ticket = link.split_once("#access=").unwrap().1.to_owned();
        let origin = format!("http://{}", self.address);
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        let mut operator = Operator {
            client,
            origin,
            cookie: None,
        };
        let (_, cookie) = operator
            .post("/console/session", json!({"ticket":ticket}))
            .await;
        operator.cookie = Some(cookie.expect("the console session sets its cookie"));
        operator
    }
    /// The one answer the worker needs from the homeserver on every pass:
    /// whoami, a full-state sync and the room state, then an event id for
    /// anything it sends. The fixture is not the peer; it only keeps the
    /// worker polling.
    fn answer(&mut self, request: common::Request) {
        self.served += 1;
        let target = request.target.clone();
        if target.ends_with("/account/whoami") {
            request.json(200, common::who());
        } else if target.starts_with("/_matrix/client/v3/sync?") {
            request.json(200, common::sync(&format!("served-{}", self.served)));
        } else if target.ends_with("/state") {
            request.json(200, common::state());
        } else if request.method == "PUT" && target.contains("/send/") {
            request.json(200, json!({"event_id":format!("$served_{}",self.served)}));
        } else {
            panic!("the fixture has no answer for {} {target}", request.method);
        }
    }
    /// Keep the worker served until `ready(store view, capabilities)` holds.
    /// The harness's patience, not a product budget.
    pub async fn serve_until(&mut self, stage: &str, ready: impl Fn(&Self, &Value) -> bool) {
        let until = tokio::time::Instant::now() + Duration::from_secs(30);
        loop {
            while let Some(request) = self.fake.try_next() {
                self.answer(request);
            }
            let status = self.capabilities().await;
            if ready(self, &status) {
                return;
            }
            assert!(
                tokio::time::Instant::now() < until,
                "timed out during {stage}: {status}"
            );
            tokio::select! {
                request = self.fake.next() => self.answer(request),
                _ = tokio::time::sleep(Duration::from_millis(50)) => {}
            }
        }
    }
    /// Keep the worker served for a while: for asserting that nothing
    /// happened, which no predicate can wait for.
    pub async fn serve_for(&mut self, duration: Duration) {
        let until = tokio::time::Instant::now() + duration;
        while tokio::time::Instant::now() < until {
            tokio::select! {
                request = self.fake.next() => self.answer(request),
                _ = tokio::time::sleep_until(until) => {}
            }
        }
    }
    pub fn count(&self, query: &str) -> u64 {
        self.sql().query_row(query, [], |r| r.get(0)).unwrap()
    }
    pub fn text(&self, query: &str) -> String {
        self.sql().query_row(query, [], |r| r.get(0)).unwrap()
    }
}
/// The operator's authenticated console client.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub struct Operator {
    client: reqwest::Client,
    origin: String,
    cookie: Option<String>,
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
impl Operator {
    pub async fn post(&self, path: &str, body: Value) -> (Value, Option<String>) {
        let mut request = self
            .client
            .post(format!("{}{path}", self.origin))
            .header("Content-Type", "application/json")
            .header("Origin", &self.origin)
            .header("Sec-Fetch-Site", "same-origin")
            .body(serde_json::to_vec(&body).unwrap());
        if let Some(cookie) = &self.cookie {
            request = request.header("Cookie", cookie);
        }
        let response = request.send().await.unwrap();
        let cookie = response
            .headers()
            .get("set-cookie")
            .map(|v| v.to_str().unwrap().split(';').next().unwrap().to_owned());
        let status = response.status();
        let bytes = response.bytes().await.unwrap();
        assert_eq!(
            status,
            reqwest::StatusCode::OK,
            "console request refused: {}",
            String::from_utf8_lossy(&bytes)
        );
        (serde_json::from_slice(&bytes).unwrap(), cookie)
    }
    /// A console call whose refusal is the fact under test.
    pub async fn try_post(&self, path: &str, body: Value) -> (u16, Value) {
        let mut request = self
            .client
            .post(format!("{}{path}", self.origin))
            .header("Content-Type", "application/json")
            .header("Origin", &self.origin)
            .header("Sec-Fetch-Site", "same-origin")
            .body(serde_json::to_vec(&body).unwrap());
        if let Some(cookie) = &self.cookie {
            request = request.header("Cookie", cookie);
        }
        let response = request.send().await.unwrap();
        let status = response.status().as_u16();
        let bytes = response.bytes().await.unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }
    /// The stopped-owner inspection of a dispatch (ADR-162): the operator's
    /// receipt that frees the runner slot (ADR-163) without resolving anything.
    pub async fn inspect(&self, engagement: &str, dispatch: &str) -> Value {
        self.post(
            &format!("/console/api/agents/{engagement}/stopped-dispatches/{dispatch}/inspect"),
            json!({}),
        )
        .await
        .0
    }
    pub async fn resolve(&self, engagement: &str, body: Value) -> Value {
        self.post(
            &format!("/console/api/agents/{engagement}/resolve-stopped-dispatch"),
            body,
        )
        .await
        .0
    }
}
