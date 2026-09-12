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
    pub fn launch(&self, enabled: bool) -> Running {
        let file = private::open(&self.root.path().join("native.stderr"), true).unwrap();
        Running(
            self.command(enabled)
                .stderr(Stdio::from(file))
                .spawn()
                .unwrap(),
        )
    }
    fn sql(&self) -> rusqlite::Connection {
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
    pub async fn capabilities(&self) -> Value {
        let until = tokio::time::Instant::now() + Duration::from_secs(15);
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
