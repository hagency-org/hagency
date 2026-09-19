use super::{crypto, matrix};
use hagency_core::{canonical, replies::*, tasks::SessionBinding};
use hagency_store::{DomainRepository, private};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    io::Read,
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
pub const OWNER: &str = "@owner:example.test";
const PROJECT: &str = "!factory_project:example.test";
const ROOT: &str = "!bootstrap:example.test";
const REPRESENTATIVE_TOKEN: &str = "synthetic-separate-representative-token";
const APPROVAL_TOKEN: &str = "synthetic-independent-approval-token";
const HUMAN_TOKEN: &str = "synthetic-independent-owner-token";
const AS_TOKEN: &str = "synthetic-fixed-side-application-service-token";
fn reg() -> hagency_core::authority::Registration {
    matrix::domain::registration()
}
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
pub fn engagement(index: usize) -> String {
    format!(
        "en_{}",
        &format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&[reg().fleet_id, format!("fleet_target_{index}")]).unwrap()
            )
        )[..32]
    )
}
pub fn incoming_bytes(event: &str) -> Vec<u8> {
    format!("independent owner bytes for {event}\0\u{fffd}\n").into_bytes()
}
fn member(user: &str, membership: &str) -> Value {
    json!({"type":"m.room.member","state_key":user,"content":{"membership":membership}})
}
fn rules() -> Vec<Value> {
    vec![
        json!({"type":"m.room.join_rules","state_key":"","content":{"join_rule":"invite"}}),
        json!({"type":"m.room.power_levels","state_key":"","content":{"users":{OWNER:100,reg().representative_mxid:50},"users_default":0,"invite":0}}),
    ]
}
fn encrypted() -> Value {
    json!({"type":"m.room.encryption","state_key":"","content":{"algorithm":"m.megolm.v1.aes-sha2"}})
}
struct Running(Child);
impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
/// One fleet at a time. Each fixture runs a real service process, three
/// encrypted Matrix clients, per-agent guardians and scripted helpers. Cargo
/// runs this binary's tests in parallel, and four of them on a hosted runner's
/// three or four cores starved one another: a 5 s header deadline expired
/// against the LOCAL fake, handoffs lost their warm owner, helpers ran out of
/// patience. A fleet is qualified against its own budgets, not against three
/// other fleets competing for the same cores.
static ONE_FLEET: std::sync::LazyLock<std::sync::Arc<tokio::sync::Mutex<()>>> =
    std::sync::LazyLock::new(Default::default);

pub struct Fixture {
    root: tempfile::TempDir,
    state: PathBuf,
    address: std::net::SocketAddr,
    pub fake: matrix::Fake,
    pub peer: Peer,
    child: Running,
    // Fields drop in declaration order, so this goes last: the next fleet starts
    // only after this one's service, fake and temporary state are gone.
    _one_fleet: tokio::sync::OwnedMutexGuard<()>,
}
impl Fixture {
    pub async fn new(application_service: bool, media: bool) -> Self {
        Self::profile(application_service, media, false).await
    }
    pub async fn profile(application_service: bool, media: bool, local: bool) -> Self {
        Self::configured(application_service, media, local, false, None).await
    }
    pub async fn paced_startup(sdk_ms: Option<u64>) -> Self {
        Self::configured(false, false, true, true, sdk_ms).await
    }
    async fn configured(
        application_service: bool,
        media: bool,
        local: bool,
        paced_startup: bool,
        sdk_ms: Option<u64>,
    ) -> Self {
        let one_fleet = ONE_FLEET.clone().lock_owned().await;
        let root = tempfile::tempdir().unwrap();
        let base = root.path().canonicalize().unwrap();
        let state = base.join("state");
        let initialized = Command::new(env!("CARGO_BIN_EXE_hagency"))
            .args(["init", "--state-dir"])
            .arg(&state)
            .output()
            .unwrap();
        assert!(initialized.status.success());
        for name in ["homes", "source", "root-work"] {
            private::directory(&base.join(name)).unwrap();
        }
        fs::write(
            base.join("source/configured-fleet-probe"),
            b"offline fixture only",
        )
        .unwrap();
        if media {
            fs::write(
                base.join("source/configured-fleet-media"),
                b"offline media fixture only",
            )
            .unwrap();
        }
        fs::write(
            base.join("source/source.txt"),
            b"retained source shared by neither runtime",
        )
        .unwrap();
        let mut db = DomainRepository::open(&state).unwrap();
        db.register(&reg()).unwrap();
        let resource = matrix::domain::resource("pool", "seat", 1000);
        db.put_resource(&resource).unwrap();
        // Only the unrelated coordinator is preexisting fixture state. Both
        // target engagements are admitted/provisioned solely by service intake.
        let mut request = matrix::domain::request("bootstrap", "Coordinator", &resource, 100);
        request.target_project_id = "factory_project".into();
        request.target_room_id = PROJECT.into();
        let proof = matrix::domain::proof(&request);
        let coordinator = db.admit(&proof, 1000).unwrap();
        db.approve("fixture_coordinator", &proof, 1000).unwrap();
        let effect = db.claim_effect().unwrap().unwrap();
        db.observe_effect(
            &effect.id,
            effect.fence,
            &hagency_store::EffectOutcome::Applied {
                receipt: "preexisting coordinator only".into(),
            },
        )
        .unwrap();
        let transport = MatrixTransportObservation {
            engagement_id: coordinator.id.clone(),
            registration_generation: 1,
            generation: 1,
            sender_mxid: crypto::SENDER.into(),
            device_id: crypto::DEVICE.into(),
        };
        db.observe_matrix_transport(&transport, now()).unwrap();
        db.observe_matrix_room(
            &MatrixRoomObservation {
                engagement_id: coordinator.id.clone(),
                registration_generation: 1,
                transport_generation: 1,
                room_id: ROOT.into(),
                generation: 1,
                privacy: RoomPrivacy::Direct {
                    human_mxid: OWNER.into(),
                },
                joined: BTreeSet::from([OWNER.into(), crypto::SENDER.into()]),
                invite_only: true,
                encrypted: true,
            },
            now(),
        )
        .unwrap();
        db.resolve_verified_matrix_session(
            &SessionBinding {
                id: "root".into(),
                engagement_id: coordinator.id.clone(),
                room_id: ROOT.into(),
                thread_root: None,
            },
            now(),
        )
        .unwrap();
        db.register_workspace("root_work").unwrap();
        drop(db);
        let fake = matrix::Fake::start(true).await;
        let mut peer = Peer::new(application_service, media, fake.endpoint.clone()).await;
        peer.provision_targets = !paced_startup;
        let reserve = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = reserve.local_addr().unwrap();
        drop(reserve);
        let executable = PathBuf::from(env!("CARGO_BIN_EXE_hagency-owned-mcp-probe"))
            .canonicalize()
            .unwrap();
        let digest = format!("{:x}", Sha256::digest(fs::read(&executable).unwrap()));
        let fingerprint = canonical::digest(&json!(reg())).unwrap();
        let anchors = json!([{"user_id":OWNER,"master_key":peer.approval.anchor()}]);
        let mut provision = json!({"profile":"registration_token_home_rooms_enrollment_step_v1","peer_masters":anchors,
            "home":{"root":base.join("homes"),"task_client":PathBuf::from(env!("CARGO_BIN_EXE_hagency")).canonicalize().unwrap(),
                "projects":[{"project_id":"factory_project","source":base.join("source"),"mode":"copy"}]}});
        if application_service {
            provision["profile"] = json!("appservice_login_home_rooms_enrollment_step_v1");
            provision["namespace_prefix"] = json!(format!("{}_", reg().fleet_id));
        }
        // The fleet provisions its agents one after another, and no task is
        // posted until all are ready, so the first agent idles warm for as long
        // as the rest take. On a small hosted runner that exceeded a 10 s idle
        // budget (each agent takes about 6 s there): the first warm runtime
        // expired with Deadline and its dispatch handoff was refused. These
        // fixtures do not test idle expiry, so the budget is set where it can
        // never be the limit; warm_runtime covers expiry with its own budget.
        let mut config = json!({"profile":"codex_app_server_agent_v1","executable":executable,"executable_sha256":digest,
            "send_file":media,"receive_file":media,
            "workspaces":{"root_work":base.join("root-work")},"file_limit":4194304,"operation_ms":30_000,"response_ms":2000,
            "intake_sessions":["root"],"factory_service":{"profile":"inline_factory_service_checkpoint_v1","idle_ms":120_000},
            "matrix":{"origin":fake.endpoint,"server_name":"example.test","registration_fingerprint":fingerprint,"engagement_id":coordinator.id,
                "registration_generation":1,"transport_generation":1,"sender_mxid":crypto::SENDER,"device_id":crypto::DEVICE,
                "rooms":[{"id":ROOT,"generation":1,"privacy":{"kind":"direct","human_mxid":OWNER}},
                    {"id":PROJECT,"generation":1,"privacy":{"kind":"group"}}],"token_provisioning":provision},
            "approval":{"origin":fake.endpoint,"server_name":"example.test","registration_fingerprint":fingerprint,"engagement_id":coordinator.id,
                "registration_generation":1,"transport_generation":1,"sender_mxid":reg().approval_bot_mxid,"device_id":"APPROVAL_DEVICE",
                "rooms":[{"id":"!private:example.test","generation":1,"privacy":{"kind":"direct","human_mxid":OWNER}}],"peer_masters":anchors}});
        if local {
            for name in ["provider-home", "provider-codex"] {
                private::directory(&base.join(name)).unwrap();
            }
            private::write_new(
                &base.join("provider-codex/auth.json"),
                b"synthetic opaque provider fixture",
            )
            .unwrap();
            config["local_codex"] = json!({"profile":"provider_owned_codex_v1","preset":"pool","seat":"seat",
                "home":base.join("provider-home"),"codex_home":base.join("provider-codex")});
            config["operation_ms"] = json!(60_000);
            config["approval_owner_wait_ms"] = json!(40_000);
            config["matrix_request_interval_ms"] = json!(25);
        }
        if paced_startup {
            config["matrix_request_interval_ms"] = json!(1000);
        }
        if let Some(ms) = sdk_ms {
            config["matrix_sdk_timeout_ms"] = json!(ms);
        }
        private::write_new(
            &state.join("agent-driver.json"),
            &serde_json::to_vec(&config).unwrap(),
        )
        .unwrap();
        for (name, bytes) in [
            ("matrix.access_token", matrix::TOKEN.as_bytes()),
            ("matrix.sdk_key", &[42; 32]),
            (
                "matrix.ca.pem",
                include_bytes!("../../../hagency-matrix/tests/fixtures/ca.pem").as_slice(),
            ),
            (
                "matrix.registration_token",
                b"synthetic-registration-token".as_slice(),
            ),
            ("matrix.appservice_token", AS_TOKEN.as_bytes()),
            (
                "matrix.representative_token",
                REPRESENTATIVE_TOKEN.as_bytes(),
            ),
            ("matrix.provisioning_key", &[73; 32]),
            ("approval.access_token", APPROVAL_TOKEN.as_bytes()),
            ("approval.sdk_key", &[85; 32]),
            (
                "approval.ca.pem",
                include_bytes!("../../../hagency-matrix/tests/fixtures/ca.pem").as_slice(),
            ),
        ] {
            private::write_new(&state.join(name), bytes).unwrap();
        }
        let diagnostic = private::open(&base.join("native.stderr"), true).unwrap();
        let child = Running(
            Command::new(env!("CARGO_BIN_EXE_hagency"))
                .args(["serve", "--agent-driver", "--state-dir"])
                .arg(&state)
                .args(["--listen", &address.to_string()])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::from(diagnostic))
                .spawn()
                .unwrap(),
        );
        Self {
            root,
            state,
            address,
            fake,
            peer,
            child,
            _one_fleet: one_fleet,
        }
    }
    pub fn work(&self, index: usize) -> PathBuf {
        self.root
            .path()
            .canonicalize()
            .unwrap()
            .join(format!("homes/agents/agent_{}/workdir", engagement(index)))
    }
    pub fn try_receipt(&self, index: usize, stage: &str) -> Option<Value> {
        match fs::read(self.work(index).join(format!("owned-mcp.{stage}"))) {
            Ok(bytes) => Some(serde_json::from_slice(&bytes).unwrap()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => panic!("fixture receipt read: {error:?}"),
        }
    }
    pub fn receipt(&self, index: usize, stage: &str) -> Value {
        self.try_receipt(index, stage).unwrap()
    }
    fn sql(&self) -> rusqlite::Connection {
        rusqlite::Connection::open_with_flags(
            self.state.join("domain.sqlite3"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap()
    }
    pub fn count(&self, query: &str) -> u64 {
        self.sql().query_row(query, [], |r| r.get(0)).unwrap()
    }
    pub fn task_started(&self, task: &str) -> bool {
        self.sql().query_row("SELECT EXISTS(SELECT 1 FROM runner_dispatches WHERE task_id=?1 AND state='started')",[task],|r|r.get(0)).unwrap()
    }
    pub fn task_session(&self, task: &str) -> String {
        self.sql()
            .query_row(
                "SELECT session_id FROM canonical_tasks WHERE id=?1",
                [task],
                |r| r.get(0),
            )
            .unwrap()
    }
    pub fn assert_project_task(&self, index: usize, task: &str) {
        let session = self.task_session(task);
        assert!(session.starts_with(&format!("project_{}_1_", engagement(index))));
        let (encoded,workspace):(String,String)=self.sql().query_row("SELECT f.route,json_extract(d.input,'$.resources[0].id') FROM final_replies f JOIN runner_dispatches d ON d.id=f.source_dispatch_id WHERE d.task_id=?1",[task],|r|Ok((r.get(0)?,r.get(1)?))).unwrap();
        let route: Value = serde_json::from_str(&encoded).unwrap();
        assert_eq!(route["engagement_id"], engagement(index));
        assert_eq!(route["room_id"], PROJECT);
        assert_eq!(route["privacy"], json!({"kind":"group"}));
        assert_eq!(route["encrypted"], false);
        assert_eq!(route["thread_root"], Value::Null);
        assert_eq!(workspace, format!("work_{}", engagement(index)));
    }
    pub fn task_status(&self, task: &str) -> String {
        self.sql()
            .query_row(
                "SELECT json_extract(config,'$.status') FROM canonical_tasks WHERE id=?1",
                [task],
                |r| r.get(0),
            )
            .unwrap()
    }
    pub fn task_reply_delivered(&self, task: &str) -> bool {
        self.sql().query_row("SELECT COUNT(*)=1 FROM owned_task_completions c JOIN runner_dispatches d ON d.id=c.dispatch_id AND d.fence=c.fence JOIN final_replies r ON r.id=c.reply_id AND r.task_id=c.task_id AND r.execution_epoch=c.execution_epoch WHERE c.task_id=?1 AND d.task_id=?1 AND c.state='ready' AND r.state='delivered'",[task],|r|r.get(0)).unwrap()
    }
    pub fn assert_file_delivery(&self, index: usize, round: usize, task: &str) {
        let agent = &self.peer.agents[index];
        assert_eq!(agent.uploads.len(), round + 1);
        let event = &agent.crypto.events[round * 2];
        let content = &event["content"];
        assert_eq!(event["sender"], agent.user);
        assert_eq!(content["msgtype"], "m.file");
        assert_eq!(content["filename"], "任务文件.bin");
        assert_eq!(content["body"], format!("Factory file for {task}"));
        assert!(content.get("url").is_none() && content.get("m.relates_to").is_none());
        assert_eq!(
            content["file"]["url"],
            format!("mxc://example.test/fleet_file_{index}_{}", round + 1)
        );
        let input = format!("$fleet_input_{index}_{}", round + 1);
        let mut expected = format!("factory binary for {task}\0\u{fffd}\n").into_bytes();
        expected.extend(incoming_bytes(&input));
        assert_eq!(content["info"]["size"], expected.len());
        assert_ne!(agent.uploads[round], expected);
        let info = serde_json::from_value(content["file"].clone()).unwrap();
        let mut cursor = std::io::Cursor::new(&agent.uploads[round]);
        let mut plain = Vec::new();
        matrix_sdk_crypto::AttachmentDecryptor::new(&mut cursor, info)
            .unwrap()
            .read_to_end(&mut plain)
            .unwrap();
        assert_eq!(
            plain, expected,
            "independent upstream attachment decoder, original agent/task bytes"
        );
        let count: u64=self.sql().query_row("SELECT COUNT(*) FROM file_deliveries f JOIN runner_dispatches d ON d.id=f.dispatch_id WHERE d.task_id=?1 AND f.event_state='delivered'",[task],|r|r.get(0)).unwrap();
        assert_eq!(count, 1);
        let receipt = self.receipt(index, "fleet-media");
        assert_eq!(receipt["task_id"], task);
        assert_eq!(receipt["task_status"], "in_progress");
        assert_eq!(
            agent.downloads,
            vec![1; round + 1],
            "each original incoming ciphertext fetched once"
        );
        let received = self.receipt(index, "fleet-receive");
        assert_eq!(received["task_id"], task);
        assert_eq!(received["event_id"], input);
        let path = received["path"].as_str().unwrap();
        assert!(
            path.starts_with(".hagency-received-") && path.ends_with(".bin") && !path.contains('/')
        );
        assert_eq!(
            fs::read(self.work(index).join(path)).unwrap(),
            incoming_bytes(&input)
        );
        assert_eq!(
            self.count("SELECT COUNT(*) FROM received_files WHERE state='ready'"),
            (round as u64 + 1) * 2
        );
    }
    pub fn assert_local_provider(&self, value: &Value) {
        let base = self.root.path().canonicalize().unwrap();
        assert_eq!(value["home"], json!(base.join("provider-home")));
        assert_eq!(value["codex_home"], json!(base.join("provider-codex")));
        assert_eq!(value["ambient_key"], false);
        assert_eq!(
            fs::read(base.join("provider-codex/auth.json")).unwrap(),
            b"synthetic opaque provider fixture"
        );
        assert_eq!(fs::read_dir(base.join("provider-home")).unwrap().count(), 0);
        assert_eq!(
            fs::read_dir(base.join("provider-codex")).unwrap().count(),
            1
        );
        assert_eq!(self.count("SELECT COUNT(*) FROM managed_accounts"), 0);
        assert_eq!(
            self.count("SELECT COUNT(*) FROM account_login_observations"),
            0
        );
        assert!(!self.state.join("runtime-home").exists());
    }
    /// Current generation of the shared project's room scope; 0 before any.
    pub fn project_generation(&self) -> u64 {
        self.sql()
            .query_row(
                "SELECT generation FROM matrix_room_scopes WHERE room_id=?1 AND available=1",
                [PROJECT],
                |row| row.get(0),
            )
            .unwrap_or(0)
    }
    pub fn assert_project_scope(&self) {
        let (generation, joined): (u64, String) = self
            .sql()
            .query_row(
                "SELECT generation,joined FROM matrix_room_scopes WHERE room_id=?1 AND available=1",
                [PROJECT],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        // Both physical joins may appear in one authenticated state snapshot.
        // Generations count changed observations, not individual network writes.
        assert!((2..=3).contains(&generation));
        let mut expected = BTreeSet::from([
            OWNER.to_owned(),
            reg().representative_mxid,
            crypto::SENDER.to_owned(),
        ]);
        expected.extend(self.peer.agents.iter().map(|agent| agent.user.clone()));
        assert_eq!(
            serde_json::from_str::<BTreeSet<String>>(&joined).unwrap(),
            expected
        );
    }
    /// The harness's patience per stage, not a product budget: it returns as
    /// soon as the stage is ready. 20 s was enough on a workstation and not on a
    /// small hosted runner, where the two-agent media and executable stages ran
    /// past it after every product step had succeeded.
    pub async fn until(&mut self, stage: &str, ready: impl Fn(&Self) -> bool) {
        let until = tokio::time::Instant::now() + Duration::from_secs(60);
        loop {
            assert!(
                self.child.0.try_wait().unwrap().is_none(),
                "service exited during {stage}: {}",
                self.diagnostic()
            );
            if ready(self) {
                return;
            }
            assert!(
                tokio::time::Instant::now() < until,
                "timed out during {stage}: {}",
                self.diagnostic()
            );
            tokio::select! {request=self.fake.next()=>self.peer.respond(request).await,_=tokio::time::sleep(Duration::from_millis(10))=>{}}
        }
    }
    pub async fn startup_result(&mut self) -> bool {
        let until = tokio::time::Instant::now() + Duration::from_secs(75);
        loop {
            if let Some(status) = self.child.0.try_wait().unwrap() {
                assert!(!status.success());
                assert!(
                    self.diagnostic()
                        .contains("approval startup refused: Matrix operation cancelled"),
                    "{}",
                    self.diagnostic()
                );
                return false;
            }
            if self.diagnostic().contains("native service ready;") {
                return true;
            }
            assert!(
                tokio::time::Instant::now() < until,
                "paced startup observer expired: {}",
                self.diagnostic()
            );
            tokio::select! {request=self.fake.next()=>self.peer.respond(request).await,_=tokio::time::sleep(Duration::from_millis(10))=>{}}
        }
    }
    fn diagnostic(&self) -> String {
        // Only disposable synthetic fixture data. Never used with live state.
        let mut diagnostic = fs::read_to_string(self.root.path().join("native.stderr")).unwrap();
        for index in 0..2 {
            let work = self.work(index);
            let requests: Vec<String> = fs::read_to_string(work.join("owned-mcp.requests"))
                .unwrap_or_default()
                .lines()
                .filter_map(|line| serde_json::from_str::<Value>(line).ok())
                .filter_map(|request| request["method"].as_str().map(str::to_owned))
                .collect();
            let mut stages: Vec<String> = fs::read_dir(&work)
                .into_iter()
                .flatten()
                .filter_map(Result::ok)
                .filter_map(|entry| entry.file_name().into_string().ok())
                .filter(|name| name.starts_with("owned-mcp."))
                .collect();
            stages.sort();
            diagnostic.push_str(&format!(
                "\nsynthetic agent {index}: requests={requests:?}, receipts={stages:?}"
            ));
        }
        diagnostic
    }
    #[cfg(unix)]
    pub fn revoke_local_provider_permissions(&self) {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            self.root.path().join("provider-codex"),
            fs::Permissions::from_mode(0o777),
        )
        .unwrap();
    }
    pub fn handoffs_refused(&self) -> bool {
        self.diagnostic()
            .matches("original dispatch handoff refused; owner retained")
            .count()
            == 2
    }
    pub async fn wait_for_registered_agents(&mut self) {
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(3))
            .build()
            .unwrap();
        let read = async {
            let token = fs::read_to_string(self.state.join("operator.token")).unwrap();
            let until = tokio::time::Instant::now() + Duration::from_secs(10);
            loop {
                let response = client
                    .get(format!(
                        "http://{}/api/native/v1/capabilities",
                        self.address
                    ))
                    .bearer_auth(token.trim())
                    .send()
                    .await
                    .unwrap();
                assert_eq!(response.status(), 200);
                let snapshot: Value =
                    serde_json::from_str(&response.text().await.unwrap()).unwrap();
                assert_eq!(snapshot["factory_service"]["failed"], false);
                if snapshot["factory_service"]["registered_backends"] == 3 {
                    break;
                }
                assert!(tokio::time::Instant::now() < until);
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        };
        tokio::pin!(read);
        loop {
            tokio::select! {_=&mut read=>break,request=self.fake.next()=>self.peer.respond(request).await}
        }
    }
    pub async fn assert_handoff_failures(&mut self) {
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(3))
            .build()
            .unwrap();
        let read = async {
            let response = client
                .get(format!("http://{}/ready", self.address))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), 503);
            let public = response.text().await.unwrap();
            assert!(!public.contains("lost_authority"));
            let token = fs::read_to_string(self.state.join("operator.token")).unwrap();
            let endpoint = format!("http://{}/api/native/v1/capabilities", self.address);
            assert_eq!(client.get(&endpoint).send().await.unwrap().status(), 401);
            for _ in 0..2 {
                let response = client
                    .get(&endpoint)
                    .bearer_auth(token.trim())
                    .send()
                    .await
                    .unwrap();
                assert_eq!(response.status(), 200);
                let snapshot: Value =
                    serde_json::from_str(&response.text().await.unwrap()).unwrap();
                assert_eq!(snapshot["factory_service"]["failed"], true);
                let agents = snapshot["factory_service"]["agents"].as_array().unwrap();
                for index in 0..2 {
                    let status = &agents
                        .iter()
                        .find(|a| a["engagement_id"] == engagement(index))
                        .unwrap()["status"];
                    assert_eq!(status["error"], "worker");
                    assert_eq!(status["owned_failure"], "lost_authority");
                    assert_eq!(status["state"], "outcome_unknown");
                    assert_eq!(status["workspace_registered"], false);
                    for field in ["protocol", "cleanup", "settlement", "runtime"] {
                        assert!(status[field].is_null());
                    }
                }
            }
        };
        tokio::pin!(read);
        loop {
            tokio::select! {_=&mut read=>break,request=self.fake.next()=>self.peer.respond(request).await}
        }
    }
    pub async fn assert_ready(&mut self) {
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(3))
            .build()
            .unwrap();
        let read = async {
            let response = client
                .get(format!("http://{}/ready", self.address))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), 200);
            let value: Value = serde_json::from_str(&response.text().await.unwrap()).unwrap();
            assert_eq!(value["status"], "ok");
            assert!(
                value["components"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|c| c["name"] == "factory_service" && c["state"] == "running")
            );
            assert!(
                value["components"].as_array().unwrap().iter().all(|c| c
                    .as_object()
                    .unwrap()
                    .len()
                    == 2)
            );
            let endpoint = format!("http://{}/api/native/v1/capabilities", self.address);
            assert_eq!(client.get(&endpoint).send().await.unwrap().status(), 401);
            let token = fs::read_to_string(self.state.join("operator.token")).unwrap();
            let response = client
                .get(&endpoint)
                .bearer_auth(token.trim())
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), 200);
            let snapshot: Value = serde_json::from_str(&response.text().await.unwrap()).unwrap();
            let fleet = &snapshot["factory_service"];
            assert_eq!(fleet["registered_backends"], 3);
            assert_eq!(fleet["failed"], false);
            let agents = fleet["agents"].as_array().unwrap();
            assert_eq!(agents.len(), 3);
            for index in 0..2 {
                let agent = agents
                    .iter()
                    .find(|a| a["engagement_id"] == engagement(index))
                    .unwrap();
                assert!(agent["status"]["error"].is_null());
                assert!(serde_json::to_vec(&agent["status"]).unwrap().len() <= 768);
            }
            let text = snapshot.to_string();
            for secret in [
                token.trim(),
                REPRESENTATIVE_TOKEN,
                APPROVAL_TOKEN,
                HUMAN_TOKEN,
                AS_TOKEN,
                "synthetic-registration-token",
            ] {
                assert!(!text.contains(secret));
            }
        };
        tokio::pin!(read);
        loop {
            tokio::select! {_=&mut read=>break,request=self.fake.next()=>self.peer.respond(request).await}
        }
    }
    pub async fn stop(&mut self) {
        assert!(
            Command::new("/bin/kill")
                .args(["-TERM", &self.child.0.id().to_string()])
                .status()
                .unwrap()
                .success()
        );
        tokio::time::timeout(Duration::from_secs(10),async {loop {
            if let Some(status)=self.child.0.try_wait().unwrap() {assert!(status.success(),"shutdown: {}",self.diagnostic());break;}
            tokio::select! {request=self.fake.next()=>self.peer.respond(request).await,_=tokio::time::sleep(Duration::from_millis(10))=>{}}
        }}).await.unwrap();
        for agent in &mut self.peer.agents {
            if let Some(owner) = agent.owner_job.take() {
                owner.await.unwrap();
            }
        }
    }
}

pub struct Agent {
    pub user: String,
    device: String,
    token: String,
    dm: String,
    pub crypto: crypto::Peer,
    uploads: Vec<Vec<u8>>,
    incoming: Vec<Vec<u8>>,
    downloads: Vec<usize>,
    created: bool,
    invited: bool,
    joined: bool,
    owner: bool,
    owner_job: Option<tokio::task::JoinHandle<()>>,
    registered: bool,
    logged: bool,
    pub account_posts: usize,
    pub room_posts: usize,
    sync: u64,
    pending: Option<Value>,
    pub project_events: Vec<Value>,
}
impl Agent {
    fn new(index: usize, crypto: crypto::Peer) -> Self {
        Self {
            user: format!("@{}_{}:example.test", reg().fleet_id, engagement(index)),
            device: format!("DEVICE_{}", engagement(index)),
            token: format!("synthetic-created-agent-token-{index}"),
            dm: format!("!fleet_dm_{index}:example.test"),
            crypto,
            uploads: Vec::new(),
            incoming: Vec::new(),
            downloads: Vec::new(),
            created: false,
            invited: false,
            joined: false,
            owner: false,
            owner_job: None,
            registered: false,
            logged: false,
            account_posts: 0,
            room_posts: 0,
            sync: 0,
            pending: None,
            project_events: Vec::new(),
        }
    }
    fn dm_state(&self) -> Value {
        assert!(self.created);
        json!([member(&self.user,"join"),member(OWNER,if self.owner {"join"} else {"invite"}),encrypted(),
        {"type":"m.room.join_rules","state_key":"","content":{"join_rule":"invite"}},
        {"type":"m.room.create","state_key":"","sender":self.user,"content":{"creator":self.user,"m.federate":false}},
        {"type":"m.room.history_visibility","state_key":"","content":{"history_visibility":"invited"}}])
    }
}
pub struct Peer {
    pub agents: Vec<Agent>,
    pub approval: crypto::Peer,
    application_service: bool,
    media: bool,
    endpoint: String,
    root_sync: u64,
    approval_sync: u64,
    provision_targets: bool,
    interleave: Interleave,
}
/// Forces the order ADR178's superseded-plan amendment is about. A poll resolves
/// its inbox plan, runs intake, then selects. Live, request pacing makes that
/// intake a second wide and the later agent's join always lands inside it; this
/// fake answers in microseconds, so the fleet fixtures never reach it unaided.
/// The later agent's join is held until the first agent's next timeline sync
/// (its intake) arrives; that sync is then held and the join released, and the
/// test releases the sync only after the project generation has advanced
/// underneath it. The coordinator re-reads the room right after a join, so the
/// sync is held for a fraction of a second: a held request gets no response
/// headers, and the collector gives up on those after 5 s.
#[derive(Default)]
struct Interleave {
    armed: bool,
    done: bool,
    sync: Option<matrix::Request>,
    join: Option<matrix::Request>,
}
impl Peer {
    async fn new(application_service: bool, media: bool, endpoint: String) -> Self {
        let mut approval =
            crypto::Peer::for_sender(&reg().approval_bot_mxid, "APPROVAL_DEVICE").await;
        let mut agents = Vec::new();
        for index in 0..2 {
            let peer = approval.additional_sender(
                &format!("@{}_{}:example.test", reg().fleet_id, engagement(index)),
                &format!("DEVICE_{}", engagement(index)),
            );
            agents.push(Agent::new(index, peer));
        }
        Self {
            agents,
            approval,
            application_service,
            media,
            endpoint,
            root_sync: 0,
            approval_sync: 0,
            provision_targets: true,
            interleave: Interleave::default(),
        }
    }
    pub fn interleave_later_join_with_first_agent_poll(&mut self) {
        assert!(
            !self.agents[1].invited,
            "armed after the later agent's invite"
        );
        self.interleave.armed = true;
    }
    pub fn holding_first_agent_poll(&self) -> bool {
        self.interleave.sync.is_some() && self.agents[1].joined
    }
    pub async fn release_first_agent_poll(&mut self) {
        let sync = self.interleave.sync.take().expect("first agent poll held");
        assert!(self.interleave.join.is_none());
        self.interleave.done = true;
        self.respond_now(sync).await;
    }
    fn timeline_sync(request: &matrix::Request) -> bool {
        let Ok(url) = reqwest::Url::parse(&format!("https://fixture.test{}", request.target))
        else {
            return false;
        };
        url.path().ends_with("/sync")
            && url
                .query_pairs()
                .find(|(key, _)| key == "filter")
                .and_then(|(_, value)| serde_json::from_str::<Value>(&value).ok())
                .and_then(|filter| filter["room"]["timeline"]["limit"].as_u64())
                .is_some_and(|limit| limit > 0)
    }
    pub async fn respond(&mut self, request: matrix::Request) {
        if self.interleave.armed && !self.interleave.done {
            let actor = request.headers.get("authorization").cloned();
            let from = |index: usize| actor == Some(format!("Bearer {}", self.agents[index].token));
            if from(1)
                && request.method == "POST"
                && request.target.contains("/join/")
                && self.interleave.sync.is_none()
            {
                assert!(self.interleave.join.is_none());
                self.interleave.join = Some(request);
                return;
            }
            if from(0) && Self::timeline_sync(&request) && self.interleave.join.is_some() {
                let join = self.interleave.join.take().expect("held join");
                self.interleave.sync = Some(request);
                self.respond_now(join).await;
                return;
            }
        }
        self.respond_now(request).await;
    }
    pub async fn queue_owner_round(&mut self, round: u64) {
        for (index, agent) in self.agents.iter_mut().enumerate() {
            assert!(agent.pending.is_none());
            let room: ruma::OwnedRoomId = agent.dm.clone().try_into().unwrap();
            let mut batch = if round == 1 {
                agent.crypto.inbound_room_key(&room).await
            } else {
                json!({"rooms":{},"to_device":{"events":[]}})
            };
            let id = format!("$fleet_input_{index}_{round}");
            let mut content = json!({"msgtype":"m.text","body":format!("Factory {index} complete round {round} through canonical task tools")});
            if self.media {
                let mut plain = std::io::Cursor::new(incoming_bytes(&id));
                let mut encrypted = matrix_sdk_crypto::AttachmentEncryptor::new(&mut plain);
                let mut bytes = Vec::new();
                encrypted.read_to_end(&mut bytes).unwrap();
                let mut descriptor = serde_json::to_value(encrypted.finish()).unwrap();
                descriptor["url"] =
                    json!(format!("mxc://example.test/fleet_incoming_{index}_{round}"));
                content["msgtype"] = json!("m.file");
                content["filename"] = json!("输入文件.bin");
                content["info"] = json!({"size":bytes.len(),"mimetype":"application/octet-stream"});
                content["file"] = descriptor;
                assert_eq!(agent.incoming.len(), round as usize - 1);
                agent.incoming.push(bytes);
                agent.downloads.push(0);
            }
            let mut event = agent.crypto.owner_event(&room, content).await;
            event["event_id"] = json!(id);
            event["origin_server_ts"] = json!(now());
            batch["rooms"] = json!({"join":{&agent.dm:{"timeline":{"events":[event],"limited":false},"state":{"events":[]}}}});
            agent.pending = Some(batch);
        }
    }
    pub fn queue_project_mentions(&mut self, addressed: bool) {
        let events = if addressed {
            self.agents.iter().enumerate().map(|(index,agent)|json!({"event_id":format!("$project_mention_{index}"),"sender":OWNER,"type":"m.room.message","origin_server_ts":now(),
                "content":{"msgtype":"m.text","body":format!("PROJECT_ADDRESSED_{index}"),"m.mentions":{"user_ids":[agent.user]}}})).collect::<Vec<_>>()
        } else {
            vec![
                json!({"event_id":"$project_unaddressed","sender":OWNER,"type":"m.room.message","origin_server_ts":now(),
            "content":{"msgtype":"m.text","body":"PROJECT_UNADDRESSED","m.mentions":{"user_ids":[OWNER]}}}),
            ]
        };
        for agent in &mut self.agents {
            assert!(agent.pending.is_none());
            agent.pending = Some(
                json!({"rooms":{"join":{PROJECT:{"timeline":{"events":events,"limited":false},"state":{"events":[]}}}},"to_device":{"events":[]}}),
            );
        }
    }
    fn project(&self) -> Value {
        let mut events = vec![
            member(OWNER, "join"),
            member(&reg().representative_mxid, "join"),
            member(crypto::SENDER, "join"),
        ];
        events.extend(rules());
        events.push(json!({"type":"com.hagency.project.binding.v1","state_key":"","content":{"v":1,"purpose":"project","authVersion":1,"fleetId":reg().fleet_id,"projectId":"factory_project","ownerMxid":OWNER}}));
        for agent in &self.agents {
            if agent.invited {
                events.push(member(
                    &agent.user,
                    if agent.joined { "join" } else { "invite" },
                ));
            }
        }
        json!(events)
    }
    async fn respond_now(&mut self, request: matrix::Request) {
        let actor = request.headers.get("authorization").cloned();
        if request
            .target
            .starts_with("/_matrix/client/v1/media/download/")
        {
            assert!(self.media);
            assert_eq!(request.method, "GET");
            assert!(request.body.is_empty());
            let index = self
                .agents
                .iter()
                .position(|agent| actor == Some(format!("Bearer {}", agent.token)))
                .unwrap();
            let agent = &mut self.agents[index];
            let round=(1..=agent.incoming.len()).find(|round|request.target==format!("/_matrix/client/v1/media/download/example.test/fleet_incoming_{index}_{round}")).expect("only this agent's original media URI");
            agent.downloads[round - 1] += 1;
            assert_eq!(agent.downloads[round - 1], 1);
            let bytes = &agent.incoming[round - 1];
            let mut response=format!("HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",bytes.len()).into_bytes();
            response.extend_from_slice(bytes);
            request.raw(response);
            return;
        }
        let upload = request.target == "/_matrix/media/v3/upload";
        let body = if request.body.is_empty() || upload {
            Value::Null
        } else {
            serde_json::from_slice(&request.body).unwrap()
        };
        let url =
            reqwest::Url::parse(&format!("https://synthetic.test{}", request.target)).unwrap();
        let path = url.path();
        let segments: Vec<_> = url.path_segments().unwrap().collect();
        let project = self.project();
        let response = if actor == Some(format!("Bearer {APPROVAL_TOKEN}")) {
            if path.ends_with("/whoami") {
                (
                    200,
                    json!({"user_id":reg().approval_bot_mxid,"device_id":"APPROVAL_DEVICE","is_guest":false}),
                )
            } else if path.ends_with("/state") {
                assert!(request.target.contains("private"));
                (
                    200,
                    json!([
                        member(OWNER, "join"),
                        member(&reg().approval_bot_mxid, "join"),
                        encrypted(),
                        rules()[0]
                    ]),
                )
            } else if path.ends_with("/sync") {
                self.approval_sync += 1;
                (
                    200,
                    json!({"next_batch":format!("approval-{}",self.approval_sync),"rooms":{"join":{"!private:example.test":{"timeline":{"events":[],"limited":false},"state":{"events":[]}}}},"to_device":{"events":[]}}),
                )
            } else {
                self.approval
                    .protocol(&request.method, &request.target, &body)
                    .await
                    .expect("original approval SDK protocol")
            }
        } else if actor == Some(format!("Bearer {}", matrix::TOKEN)) {
            if path.ends_with("/whoami") {
                (200, matrix::who())
            } else if path.ends_with("/sync") {
                let mut events = Vec::new();
                if self.provision_targets && self.root_sync == 1 {
                    for index in 0..2 {
                        events.push(json!({"event_id":format!("$fleet_request_{index}"),"sender":OWNER,"type":"m.room.message","origin_server_ts":now(),"content":{"msgtype":"com.hagency.engagement.request.v1","body":json!({"requestId":format!("fleet_target_{index}"),"requester":OWNER,"project":"factory_project","projectRoomId":PROJECT,"role":"coding","requestedTokens":250,"ratePerDay":null,"agent":format!("FleetAgent{index}"),"context":{"agentDefinition":{"resourceId":"resource_27cac5503836765cd10751d2"}}}).to_string()}}));
                        events.push(json!({"event_id":format!("$fleet_approve_{index}"),"sender":reg().representative_mxid,"type":"m.room.message","origin_server_ts":now(),"content":{"msgtype":"com.hagency.engagement.approval.v1","body":json!({"requestId":format!("fleet_target_{index}"),"decision":"approve"}).to_string()}}));
                    }
                }
                self.root_sync += 1;
                (
                    200,
                    json!({"next_batch":format!("root-{}",self.root_sync),"rooms":{"join":{ROOT:{"timeline":{"events":[],"limited":false},"state":{"events":[]}},"!reception:example.test":{"timeline":{"events":events,"limited":false},"state":{"events":[]}}}},"to_device":{"events":[]}}),
                )
            } else {
                assert!(path.ends_with("/state"));
                if request.target.contains("factory_project") {
                    (200, project)
                } else {
                    let mut events = vec![member(OWNER, "join"), member(crypto::SENDER, "join")];
                    events.extend(rules());
                    if request.target.contains("reception") {
                        events.push(member(&reg().representative_mxid, "join"));
                    } else {
                        assert!(request.target.contains("bootstrap"));
                        events.push(encrypted());
                    }
                    (200, json!(events))
                }
            }
        } else if actor.is_none() || actor == Some(format!("Bearer {AS_TOKEN}")) {
            assert_eq!(actor.is_some(), self.application_service);
            if path.ends_with("/whoami") {
                match url
                    .query_pairs()
                    .find(|(k, _)| k == "user_id")
                    .map(|(_, v)| v.into_owned())
                {
                    None => (
                        200,
                        json!({"user_id":reg().representative_mxid,"is_guest":false}),
                    ),
                    Some(user) if self.agents.iter().any(|a| a.user == user && a.registered) => {
                        (200, json!({"user_id":user,"is_guest":false}))
                    }
                    Some(user) => {
                        assert!(user.starts_with("@hagency_namespace_probe_"));
                        (403, json!({"errcode":"M_EXCLUSIVE"}))
                    }
                }
            } else {
                assert_eq!(request.method, "POST");
                let index = self
                    .agents
                    .iter()
                    .position(|a| {
                        body["username"]
                            == a.user.trim_start_matches('@').split_once(':').unwrap().0
                            || body["identifier"]["user"] == a.user
                    })
                    .unwrap();
                let agent = &mut self.agents[index];
                agent.account_posts += 1;
                if path.ends_with("/register") {
                    assert!(!agent.registered);
                    agent.registered = true;
                    if self.application_service {
                        assert_eq!(body["type"], "m.login.application_service");
                        assert_eq!(body["inhibit_login"], true);
                        (200, json!({"user_id":agent.user}))
                    } else {
                        assert_eq!(body["auth"]["type"], "m.login.registration_token");
                        assert_eq!(body["device_id"], agent.device);
                        agent.logged = true;
                        (
                            200,
                            json!({"user_id":agent.user,"device_id":agent.device,"access_token":agent.token}),
                        )
                    }
                } else {
                    assert!(
                        path.ends_with("/login")
                            && self.application_service
                            && agent.registered
                            && !agent.logged
                    );
                    assert_eq!(body["type"], "m.login.application_service");
                    assert_eq!(body["device_id"], agent.device);
                    agent.logged = true;
                    (
                        200,
                        json!({"user_id":agent.user,"device_id":agent.device,"access_token":agent.token}),
                    )
                }
            }
        } else if actor == Some(format!("Bearer {REPRESENTATIVE_TOKEN}")) {
            if path.ends_with("/whoami") {
                (
                    200,
                    json!({"user_id":reg().representative_mxid,"device_id":"REP_DEVICE","is_guest":false}),
                )
            } else if path.ends_with("/state") {
                assert!(request.target.contains("factory_project"));
                (200, project)
            } else {
                assert!(path.ends_with("/invite") && request.method == "POST");
                let agent = self
                    .agents
                    .iter_mut()
                    .find(|a| body["user_id"] == a.user)
                    .unwrap();
                assert!(agent.created && !agent.invited);
                agent.invited = true;
                agent.room_posts += 1;
                (200, json!({}))
            }
        } else if actor == Some(format!("Bearer {HUMAN_TOKEN}")) {
            assert!(path.contains("/join/") && request.method == "POST");
            let index = (0..2)
                .find(|i| request.target.contains(&format!("fleet_dm_{i}")))
                .unwrap();
            let agent = &mut self.agents[index];
            assert!(agent.created && !agent.owner);
            agent.owner = true;
            (200, json!({"room_id":agent.dm}))
        } else {
            let index = self
                .agents
                .iter()
                .position(|a| actor == Some(format!("Bearer {}", a.token)))
                .expect("only exact newly returned account credentials");
            let agent = &mut self.agents[index];
            assert!(agent.logged);
            if upload {
                assert_eq!(request.method, "POST");
                assert_eq!(request.headers["content-type"], "application/octet-stream");
                assert!(
                    agent.uploads.len() < 2
                        && !request.body.is_empty()
                        && request.body.len() < 1024
                );
                agent.uploads.push(request.body.clone());
                (
                    200,
                    json!({"content_uri":format!("mxc://example.test/fleet_file_{index}_{}",agent.uploads.len())}),
                )
            } else if path.ends_with("/whoami") {
                (
                    200,
                    json!({"user_id":agent.user,"device_id":agent.device,"is_guest":false}),
                )
            } else if path.ends_with("/state") {
                if request.target.contains("factory_project") {
                    (200, project)
                } else {
                    assert!(request.target.contains(&format!("fleet_dm_{index}")));
                    (200, agent.dm_state())
                }
            } else if path.ends_with("/createRoom") {
                assert!(!agent.created);
                assert_eq!(body["invite"], json!([OWNER]));
                agent.created = true;
                agent.room_posts += 1;
                (200, json!({"room_id":agent.dm}))
            } else if path.contains("/join/") {
                assert!(agent.invited && !agent.joined);
                agent.joined = true;
                agent.room_posts += 1;
                (200, json!({"room_id":PROJECT}))
            } else if path.ends_with("/sync") {
                // Respect the service's real timeline filter. A zero-limit
                // initial state sync cannot consume an owner's pending input.
                let filter: Value = serde_json::from_str(
                    &url.query_pairs()
                        .find(|(key, _)| key == "filter")
                        .unwrap()
                        .1,
                )
                .unwrap();
                let timeline = filter["room"]["timeline"]["limit"].as_u64().unwrap() > 0;
                agent.sync += 1;
                let mut batch = if timeline { agent.pending.take() } else { None }
                    .unwrap_or_else(|| json!({"rooms":{"join":{}},"to_device":{"events":[]}}));
                batch["next_batch"] = json!(format!("agent-{index}-{}", agent.sync));
                (200, batch)
            } else if request.method == "PUT" && path.contains("/sendToDevice/") {
                agent.crypto.share(body).await;
                (200, json!({}))
            } else if request.method == "PUT" && path.contains("/send/") {
                if request.target.contains("factory_project") {
                    assert!(segments.contains(&"m.room.message"));
                    assert_eq!(body["msgtype"], "m.text");
                    assert!(
                        body["body"]
                            .as_str()
                            .unwrap()
                            .starts_with("Verified factory task ")
                    );
                    agent
                        .project_events
                        .push(json!({"sender":agent.user,"room_id":PROJECT,"content":body}));
                    (
                        200,
                        json!({"event_id":format!("$fleet_project_reply_{index}_{}",agent.project_events.len())}),
                    )
                } else {
                    assert!(
                        request.target.contains(&format!("fleet_dm_{index}"))
                            && segments.contains(&"m.room.encrypted")
                    );
                    let room: ruma::OwnedRoomId = agent.dm.clone().try_into().unwrap();
                    agent.crypto.decrypt(body, &room).await;
                    (
                        200,
                        json!({"event_id":format!("$fleet_reply_{index}_{}",agent.crypto.events.len())}),
                    )
                }
            } else {
                assert!(agent.created && agent.owner && agent.joined);
                agent
                    .crypto
                    .protocol(&request.method, &request.target, &body)
                    .await
                    .expect("original enrolled agent SDK protocol")
            }
        };
        let join = if path.ends_with("/state") {
            (0..2).find(|i| {
                request.target.contains(&format!("fleet_dm_{i}"))
                    && self.agents[*i].owner_job.is_none()
            })
        } else {
            None
        };
        request.json(response.0, response.1);
        if let Some(index) = join {
            let endpoint = self.endpoint.clone();
            let dm = self.agents[index].dm.clone();
            self.agents[index].owner_job = Some(tokio::spawn(async move {
                let client = reqwest::Client::builder()
                    .redirect(reqwest::redirect::Policy::none())
                    .add_root_certificate(
                        reqwest::Certificate::from_pem(include_bytes!(
                            "../../../hagency-matrix/tests/fixtures/ca.pem"
                        ))
                        .unwrap(),
                    )
                    .timeout(Duration::from_secs(4))
                    .build()
                    .unwrap();
                let mut url = reqwest::Url::parse(&endpoint).unwrap();
                url.path_segments_mut()
                    .unwrap()
                    .extend(["_matrix", "client", "v3", "join", &dm]);
                assert_eq!(
                    client
                        .post(url)
                        .bearer_auth(HUMAN_TOKEN)
                        .header("content-type", "application/json")
                        .body("{}")
                        .send()
                        .await
                        .unwrap()
                        .status(),
                    200
                );
            }));
        }
    }
}
