//! Actual incoming native service setup. Canonical writes provision work only;
//! Collector refresh, compatible claim, Started binding, source registration and
//! runtime launch occur exclusively inside the real service process.
#[path = "../../../hagency-matrix/tests/common/mod.rs"]
pub mod common;
#[path = "../fixtures/matrix_crypto_peer.rs"]
pub mod crypto;
use hagency_core::replies::*;
use hagency_store::{DomainRepository, EffectOutcome, private};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    cell::Cell,
    collections::BTreeSet,
    fs,
    io::{Read, Seek},
    net::SocketAddr,
    path::PathBuf,
    process::{Child, Command, Stdio},
    rc::Rc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
pub const DATA: &[u8] = b"independent incoming native bytes\0\xff\x80\n";
const STARTUP_WATCHDOG: Duration = Duration::from_secs(15);

#[derive(Clone, Copy, Debug)]
struct Observation {
    variant: &'static str,
    phase: &'static str,
    last_http: &'static str,
    requests: u16,
    status: &'static str,
    started: Instant,
}
impl Observation {
    fn new(variant: &'static str) -> Self {
        Self {
            variant,
            phase: "launch",
            last_http: "none",
            requests: 0,
            status: "unobserved",
            started: Instant::now(),
        }
    }
}

pub struct Running {
    child: Option<Child>,
    observation: Rc<Cell<Observation>>,
    // Independent read offset, opened before this original child is spawned.
    // A later fixture rename/relaunch cannot substitute another child's output.
    stderr: fs::File,
}
impl Running {
    fn snapshot(&mut self) -> Value {
        let observation = self.observation.get();
        let child = match self.child.as_mut().map(Child::try_wait) {
            Some(Ok(Some(status))) => {
                json!({"state":"exited","success":status.success(),"code":status.code()})
            }
            Some(Ok(None)) => json!({"state":"running"}),
            Some(Err(_)) => json!({"state":"inspection_failed"}),
            None => json!({"state":"reaped"}),
        };
        let mut bytes = Vec::new();
        let stderr = match self
            .stderr
            .rewind()
            .and_then(|()| (&mut self.stderr).take(8193).read_to_end(&mut bytes))
        {
            Ok(_) => stderr_category(&bytes),
            Err(_) => "read_failed",
        };
        // The original output is never interpolated. Categories, bounded counts,
        // OS exit facts and fixture-owned static labels are the entire report.
        json!({"variant":observation.variant,"phase":observation.phase,
            "last_http":observation.last_http,"requests":observation.requests,
            "status":observation.status,"elapsed_ms":observation.started.elapsed().as_millis().min(u64::MAX as u128) as u64,
            "child":child,"stderr":stderr,"stderr_bytes":bytes.len().min(8192),"stderr_truncated":bytes.len()>8192,
            "service_ready_logged":bytes.windows(b"native service ready; production Agent execution remains unavailable".len())
                .any(|value|value==b"native service ready; production Agent execution remains unavailable")})
    }
    /// Check only the original service PID. This is not process-tree cleanup.
    pub fn stop_and_reap(mut self) {
        let child = self.child.as_mut().unwrap();
        if child
            .try_wait()
            .expect("inspect original native child")
            .is_none()
        {
            child.kill().expect("terminate original native child");
        }
        child.wait().expect("reap original native child");
        self.child = None;
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        if std::thread::panicking() {
            use std::io::Write;
            let _ = writeln!(
                std::io::stderr().lock(),
                "native file original process: {}",
                self.snapshot()
            );
        }
        // Best-effort panic cleanup; successful tests use checked stop_and_reap.
        if let Some(child) = &mut self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
fn stderr_category(bytes: &[u8]) -> &'static str {
    if bytes.is_empty() {
        return "empty";
    }
    for line in bytes.split(|byte| *byte == b'\n') {
        match line.strip_suffix(b"\r").unwrap_or(line) {
            b"Error: Config" => return "config",
            b"Error: Startup" => return "startup",
            b"Error: Server" => return "server",
            b"Error: Worker" => return "worker",
            _ => {}
        }
    }
    "other"
}

struct Sender {
    machine: matrix_sdk_crypto::OlmMachine,
    server: crypto::Peer,
    one_time: std::collections::BTreeMap<String, Value>,
}
impl Sender {
    async fn new() -> Self {
        use matrix_sdk_crypto::types::requests::AnyOutgoingRequest;
        let machine = matrix_sdk_crypto::OlmMachine::new(
            ruma::user_id!("@owner:example.test"),
            ruma::device_id!("HUMAN"),
        )
        .await;
        let bootstrap = machine.bootstrap_cross_signing(false).await.unwrap();
        let AnyOutgoingRequest::KeysUpload(upload) =
            bootstrap.upload_keys_req.as_ref().unwrap().request()
        else {
            panic!("actual sender key upload missing")
        };
        let one_time = upload
            .one_time_keys
            .iter()
            .map(|(id, key)| (id.to_string(), serde_json::to_value(key).unwrap()))
            .collect();
        let mut server = crypto::Peer::new().await;
        // This is the fake homeserver's public key table for the independent
        // sender. No private service key or service SDK handle is available.
        server.query = json!({"device_keys":{crypto::HUMAN:{crypto::HUMAN_DEVICE:upload.device_keys.as_ref().unwrap()}},"master_keys":{crypto::HUMAN:bootstrap.upload_signing_keys_req.master_key.unwrap()},"self_signing_keys":{crypto::HUMAN:bootstrap.upload_signing_keys_req.self_signing_key.unwrap()},"user_signing_keys":{crypto::HUMAN:bootstrap.upload_signing_keys_req.user_signing_key.unwrap()},"failures":{}});
        let signed = serde_json::to_value(bootstrap.upload_signatures_req.signed_keys).unwrap();
        for (id, value) in signed[crypto::HUMAN].as_object().unwrap() {
            let current = if id == crypto::HUMAN_DEVICE {
                &mut server.query["device_keys"][crypto::HUMAN][id]
            } else {
                let table = ["master_keys", "self_signing_keys", "user_signing_keys"]
                    .into_iter()
                    .find(|table| {
                        server.query[*table][crypto::HUMAN]["keys"]
                            .as_object()
                            .is_some_and(|keys| keys.values().any(|key| key.as_str() == Some(id)))
                    })
                    .expect("original sender signature identifies its real key");
                &mut server.query[table][crypto::HUMAN]
            };
            // SDK signature uploads extend the original signed object. Retain
            // its device self-signature while adding actual cross-signatures.
            for (signer, signatures) in value["signatures"].as_object().unwrap() {
                if current["signatures"].get(signer).is_none() {
                    current["signatures"][signer] = json!({});
                }
                for (id, signature) in signatures.as_object().unwrap() {
                    current["signatures"][signer][id] = signature.clone();
                }
            }
        }
        let (id, _) = machine.query_keys_for_users([machine.user_id()]);
        machine
            .mark_request_as_sent(&id, &query(&server.query))
            .await
            .unwrap();
        assert!(
            machine
                .get_identity(machine.user_id(), None)
                .await
                .unwrap()
                .unwrap()
                .is_verified()
        );
        Self {
            machine,
            server,
            one_time,
        }
    }
    async fn protocol(&mut self, method: &str, target: &str, body: &Value) -> Option<(u16, Value)> {
        if method == "POST" && target == "/_matrix/client/v3/keys/claim" {
            assert_eq!(self.server.writes.len(), 4);
            assert_eq!(self.server.claims, 0);
            assert_eq!(
                body["one_time_keys"],
                json!({crypto::HUMAN:{crypto::HUMAN_DEVICE:"signed_curve25519"}})
            );
            let (id, key) = self.one_time.pop_first().unwrap();
            self.server.claims += 1;
            self.server.writes.push((target.into(), body.clone()));
            return Some((
                200,
                json!({"one_time_keys":{crypto::HUMAN:{crypto::HUMAN_DEVICE:{id:key}}},"failures":{}}),
            ));
        }
        self.server.protocol(method, target, body).await
    }
    async fn packet(&self, room: &str, direct: bool, descriptor: &Value) -> Value {
        use matrix_sdk_crypto::EncryptionSettings;
        use ruma::{api::client::keys::claim_keys, serde::Raw};
        assert_eq!(self.server.writes.len(), 5);
        let human = &self.machine;
        let (id, _) =
            human.query_keys_for_users([human.user_id(), ruma::user_id!("@worker:example.test")]);
        human
            .mark_request_as_sent(&id, &query(&self.server.query))
            .await
            .unwrap();
        let (key, value) = self.server.writes[0].1["one_time_keys"]
            .as_object()
            .unwrap()
            .iter()
            .next()
            .unwrap();
        let mut claim = claim_keys::v3::Response::new(Default::default());
        claim.one_time_keys =
            serde_json::from_value(json!({crypto::SENDER:{crypto::DEVICE:{key:value}}})).unwrap();
        let (id, _) = human
            .get_missing_sessions([ruma::user_id!("@worker:example.test")].into_iter())
            .await
            .unwrap()
            .unwrap();
        human.mark_request_as_sent(&id, &claim).await.unwrap();
        let room = ruma::RoomId::parse(room).unwrap();
        let shares = human
            .share_room_key(
                &room,
                [ruma::user_id!("@worker:example.test")].into_iter(),
                EncryptionSettings::default(),
            )
            .await
            .unwrap();
        let mut to_device = Vec::new();
        for share in shares {
            assert_eq!(share.messages[crypto::SENDER].len(), 1);
            let raw = share.messages[crypto::SENDER].values().next().unwrap();
            let content: Value = serde_json::from_str(raw.json().get()).unwrap();
            to_device
                .push(json!({"sender":crypto::HUMAN,"type":share.event_type,"content":content}));
        }
        assert!(!to_device.is_empty());
        let mut file = json!({"msgtype":"m.file","body":"原始文件.bin","filename":"原始文件.bin","info":{"mimetype":"application/octet-stream","size":DATA.len()},"file":descriptor});
        if !direct {
            file["m.relates_to"] = json!({"rel_type":"m.thread","event_id":"$task_thread"});
        }
        let mut inputs = vec![("$incoming", file)];
        if !direct {
            inputs.push(("$wake",json!({"msgtype":"m.text","body":"Read the attached user data; leave this task in progress.","m.mentions":{"user_ids":[crypto::SENDER]},"m.relates_to":{"rel_type":"m.thread","event_id":"$task_thread"}})));
        }
        let mut events = Vec::new();
        for (id, content) in inputs {
            let encrypted = human
                .encrypt_room_event_raw(
                    &room,
                    "m.room.message",
                    &Raw::from_json_string(content.to_string()).unwrap(),
                )
                .await
                .unwrap();
            events.push(json!({"event_id":id,"origin_server_ts":now(),"sender":crypto::HUMAN,"type":"m.room.encrypted","content":encrypted.content}));
        }
        let mut state = common::state();
        for (index, event) in state.as_array_mut().unwrap().iter_mut().enumerate() {
            event["event_id"] = json!(format!("$state_{index}"));
            event["sender"] = json!(crypto::SENDER);
            event["origin_server_ts"] = json!(now());
        }
        json!({"next_batch":"native_incoming_once","rooms":{"join":{room.as_str():{"state":{"events":state},"timeline":{"limited":false,"events":events}}}},"to_device":{"events":to_device}})
    }
}
fn query(value: &Value) -> ruma::api::client::keys::get_keys::v3::Response {
    let mut result = ruma::api::client::keys::get_keys::v3::Response::new();
    result.device_keys = serde_json::from_value(value["device_keys"].clone()).unwrap();
    result.master_keys = serde_json::from_value(value["master_keys"].clone()).unwrap();
    result.self_signing_keys = serde_json::from_value(value["self_signing_keys"].clone()).unwrap();
    result.user_signing_keys = serde_json::from_value(value["user_signing_keys"].clone()).unwrap();
    result
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
pub struct Fixture {
    observation: Rc<Cell<Observation>>,
    pub root: tempfile::TempDir,
    pub state_dir: PathBuf,
    pub work: PathBuf,
    pub fake: common::Fake,
    pub address: SocketAddr,
    sender: Sender,
    pub room: String,
    pub direct: bool,
    pub gets: usize,
    pub intakes: usize,
    encrypted: hagency_media::Encrypted,
    descriptor: Value,
    pub negative: bool,
}
impl Fixture {
    pub async fn new(direct: bool, mode: &str) -> Self {
        let sender = Sender::new().await;
        let room = if direct {
            "!direct:example.test"
        } else {
            "!project:example.test"
        };
        let privacy = if direct {
            RoomPrivacy::Direct {
                human_mxid: crypto::HUMAN.into(),
            }
        } else {
            RoomPrivacy::Group {}
        };
        let root = tempfile::tempdir().unwrap();
        let state_dir = root.path().join("state");
        let init = Command::new(env!("CARGO_BIN_EXE_hagency"))
            .args(["init", "--state-dir"])
            .arg(&state_dir)
            .output()
            .unwrap();
        assert!(init.status.success(), "fresh native init failed");
        let work = root.path().join("接收工作目录");
        private::directory(&work).unwrap();
        let work = work.canonicalize().unwrap();
        let mut db = DomainRepository::open(&state_dir).unwrap();
        db.register(&common::domain::registration()).unwrap();
        let resource = common::domain::resource("pool", "seat", 1000);
        db.put_resource(&resource).unwrap();
        let request = common::domain::request("bootstrap", "Worker", &resource, 100);
        let mut observation = common::domain::observation(&request);
        observation.observed_at_ms = now();
        let proof = hagency_core::authority::verify_request(
            &common::domain::registration(),
            request,
            observation,
        )
        .unwrap();
        let engagement = db.admit(&proof, now()).unwrap();
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
            engagement_id: engagement.id.clone(),
            registration_generation: 1,
            generation: 1,
            sender_mxid: crypto::SENDER.into(),
            device_id: crypto::DEVICE.into(),
        };
        db.observe_matrix_transport(&transport, now()).unwrap();
        db.observe_matrix_room(
            &MatrixRoomObservation {
                engagement_id: engagement.id.clone(),
                registration_generation: 1,
                transport_generation: 1,
                room_id: room.into(),
                generation: 1,
                privacy: privacy.clone(),
                joined: BTreeSet::from([crypto::SENDER.into(), crypto::HUMAN.into()]),
                invite_only: true,
                encrypted: true,
            },
            now(),
        )
        .unwrap();
        db.resolve_verified_matrix_session(
            &hagency_core::tasks::SessionBinding {
                id: "session".into(),
                engagement_id: engagement.id.clone(),
                room_id: room.into(),
                thread_root: (!direct).then(|| "$task_thread".into()),
            },
            now(),
        )
        .unwrap();
        db.create_canonical_task("task", "session", "Read original encrypted input", now())
            .unwrap();
        db.register_workspace("work").unwrap();
        // No enqueue, input, attachment, capability or Started fixture write.
        drop(db);
        let source = root.path().join("independent-sender");
        private::directory(&source).unwrap();
        private::write_new(&source.join("original.bin"), DATA).unwrap();
        let workspace = hagency_files::Workspace::from_directory(
            cap_std::fs::Dir::open_ambient_dir(&source, cap_std::ambient_authority()).unwrap(),
            hagency_files::Limits::new(4096, 1).unwrap(),
        )
        .unwrap();
        let snapshot = workspace
            .snapshot(&hagency_files::RelativeFile::new("original.bin").unwrap())
            .unwrap();
        let encrypted = hagency_media::Codec::new(hagency_media::Limits::new(4096, 1).unwrap())
            .encrypt(snapshot)
            .unwrap();
        let mut descriptor: Value =
            serde_json::from_slice(encrypted.descriptor().private_event_json()).unwrap();
        descriptor["url"] = json!("mxc://remote.media/native_incoming");
        let fake = common::Fake::start(true).await;
        let executable = PathBuf::from(env!("CARGO_BIN_EXE_hagency-receive-mcp-probe"))
            .canonicalize()
            .unwrap();
        let executable_sha256 = format!("{:x}", Sha256::digest(fs::read(&executable).unwrap()));
        let config = json!({"profile":"codex_app_server_development_v1","receive_file":true,"receive_inbox":{"dispatch_id":"dispatch","session_id":"session","task_id":"task","workspace_id":"work"},"executable":executable,"executable_sha256":executable_sha256,"workspaces":{"work":work},"file_limit":4096,"operation_ms":30000,"response_ms":1500,"matrix":{"origin":fake.endpoint,"server_name":"example.test","registration_fingerprint":"a".repeat(64),"engagement_id":engagement.id,"registration_generation":1,"transport_generation":1,"sender_mxid":crypto::SENDER,"device_id":crypto::DEVICE,"crypto_enrollment":{"profile":"fresh_own_account_v1","peer_masters":[{"user_id":crypto::HUMAN,"master_key":sender.server.anchor()}]},"rooms":[{"id":room,"generation":1,"privacy":privacy}]}});
        for (name, bytes) in [
            (
                "development-driver.json",
                serde_json::to_vec(&config).unwrap(),
            ),
            ("matrix.access_token", common::TOKEN.as_bytes().to_vec()),
            ("matrix.sdk_key", vec![42; 32]),
            (
                "matrix.ca.pem",
                include_bytes!("../../../hagency-matrix/tests/fixtures/ca.pem").to_vec(),
            ),
        ] {
            private::write_new(&state_dir.join(name), &bytes).unwrap();
        }
        private::write_new(&work.join("receive-fixture.mode"), mode.as_bytes()).unwrap();
        let reserve = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = reserve.local_addr().unwrap();
        drop(reserve);
        Self {
            observation: Rc::new(Cell::new(Observation::new("unlaunched"))),
            root,
            state_dir,
            work,
            fake,
            address,
            sender,
            room: room.into(),
            direct,
            gets: 0,
            intakes: 0,
            encrypted,
            descriptor,
            negative: mode == "negative",
        }
    }
    pub fn launch(&mut self, variant: &'static str) -> Running {
        self.observation = Rc::new(Cell::new(Observation::new(variant)));
        let stderr = self.root.path().join(format!("{variant}.stderr"));
        let file = private::open(&stderr, true).unwrap();
        let stderr = fs::File::open(stderr).unwrap();
        let child = Command::new(env!("CARGO_BIN_EXE_hagency"))
            .args(["serve", "--state-dir"])
            .arg(&self.state_dir)
            .args([
                "--listen",
                &self.address.to_string(),
                "--development-driver",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(file))
            .spawn()
            .unwrap();
        Running {
            child: Some(child),
            observation: self.observation.clone(),
            stderr,
        }
    }
    pub fn sql(&self) -> rusqlite::Connection {
        rusqlite::Connection::open(self.state_dir.join("domain.sqlite3")).unwrap()
    }
    pub fn count(&self, table: &str) -> u64 {
        assert!(
            [
                "runner_attempts",
                "runner_dispatches",
                "matrix_attachments",
                "received_files"
            ]
            .contains(&table)
        );
        self.sql()
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    }
    pub fn receipt(&self, name: &str) -> Option<Value> {
        fs::read(self.work.join(format!("receive-mcp.{name}")))
            .ok()
            .map(|bytes| serde_json::from_slice(&bytes).unwrap())
    }
    pub async fn respond(&mut self, request: common::Request) {
        let mut observation = self.observation.get();
        observation.requests += 1;
        observation.phase = "matrix.request";
        observation.last_http = match request.target.as_str() {
            "/_matrix/client/v3/account/whoami" => "whoami",
            "/_matrix/client/v1/media/download/remote.media/native_incoming" => "media.get",
            target if target.starts_with("/_matrix/client/v3/sync?") => "sync",
            target if target.starts_with("/_matrix/client/v3/keys/") => "keys",
            target if target.ends_with("/state") => "room.state",
            _ => "other",
        };
        self.observation.set(observation);
        assert_eq!(
            request.headers.get("authorization"),
            Some(&format!("Bearer {}", common::TOKEN))
        );
        assert!(!request.target.contains(common::TOKEN));
        match (request.method.as_str(), request.target.as_str()) {
            ("GET", "/_matrix/client/v3/account/whoami") => request.json(200, common::who()),
            ("GET", target) if target.starts_with("/_matrix/client/v3/sync?") => {
                let value = if self.sender.server.writes.len() == 5 && self.intakes == 0 {
                    self.intakes += 1;
                    self.sender
                        .packet(&self.room, self.direct, &self.descriptor)
                        .await
                } else {
                    common::sync("bootstrap")
                };
                request.json(200, value);
            }
            ("GET", target)
                if target.starts_with("/_matrix/client/v3/rooms/")
                    && target.ends_with("/state") =>
            {
                request.json(200, common::state())
            }
            ("GET", "/_matrix/client/v1/media/download/remote.media/native_incoming") => {
                self.gets += 1;
                assert_eq!(self.gets, 1, "exact replay cannot redownload");
                assert!(request.body.is_empty());
                if self.negative {
                    request.unclean(b"HTTP/1.1 200 OK\r\nContent-Length: 500\r\nConnection: close\r\n\r\ntruncated".to_vec());
                } else {
                    let body = self.encrypted.ciphertext();
                    let mut response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    )
                    .into_bytes();
                    response.extend_from_slice(body);
                    request.raw(response);
                }
            }
            _ => {
                let body = if request.body.is_empty() {
                    Value::Null
                } else {
                    serde_json::from_slice(&request.body).unwrap()
                };
                let (status, value) = self
                    .sender
                    .protocol(&request.method, &request.target, &body)
                    .await
                    .unwrap_or_else(|| {
                        panic!(
                            "unexpected actual Matrix route {} {}",
                            request.method, request.target
                        )
                    });
                request.json(status, value);
            }
        }
    }
    pub async fn drive(&mut self) -> Value {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(35);
        for _ in 0..800 {
            if let Some(value) = self.receipt("receipt") {
                return value;
            }
            if let Ok(request) =
                tokio::time::timeout(Duration::from_millis(50), self.fake.next()).await
            {
                self.respond(request).await;
                continue;
            }
            // The actual runtime can publish its receipt during the HTTP wait.
            // Preserve its independently checked file result before observing
            // the later, separately qualified process cleanup outcome.
            if let Some(value) = self.receipt("receipt") {
                return value;
            }
            let status = self.capabilities().await["development_execution"].clone();
            assert!(
                !matches!(
                    status["state"].as_str(),
                    Some("unavailable" | "outcome_unknown" | "no_work")
                ),
                "actual incoming workflow refused: {status}; intake={}, GET={}, keys={}, runtime_entry={:?}, helper_phase={:?}, sent={:?}, stage={:?}, list={:?}, first={:?}",
                self.intakes,
                self.gets,
                self.sender.server.writes.len(),
                self.receipt("entry"),
                self.receipt("phase"),
                self.receipt("sent"),
                self.receipt("stage"),
                self.receipt("list"),
                self.receipt("first")
            );
            assert!(
                tokio::time::Instant::now() < deadline,
                "incoming executable did not complete: {status}"
            );
        }
        panic!("bounded incoming fixture loop exhausted")
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
                tokio::time::timeout(
                    Duration::from_secs(2),
                    (&mut stream).take(16385).read_to_end(&mut bytes),
                )
                .await
                .unwrap()
                .unwrap();
                assert!(bytes.len() <= 16384);
                let response = String::from_utf8(bytes).unwrap();
                if response.starts_with("HTTP/1.1 200") {
                    let value: Value =
                        serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
                    let mut observation = self.observation.get();
                    observation.status = match value["development_execution"]["state"].as_str() {
                        Some("running") => "running",
                        Some("completed") => "completed",
                        Some("outcome_unknown") => "outcome_unknown",
                        Some("unavailable") => "unavailable",
                        Some("no_work") => "no_work",
                        _ => "other",
                    };
                    self.observation.set(observation);
                    return value;
                }
            }
            assert!(
                tokio::time::Instant::now() < until,
                "original native receive server unavailable"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}
