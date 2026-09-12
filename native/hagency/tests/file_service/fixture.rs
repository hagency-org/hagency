//! Actual native service setup. Domain writes provision legitimate work only;
//! Collector refresh, compatible claim, Started binding, source registration and
//! runtime launch occur exclusively inside the real service process.
#[path = "../../../hagency-matrix/tests/common/mod.rs"]
pub mod common;
#[path = "../fixtures/matrix_crypto_peer.rs"]
pub mod crypto;
use hagency_core::{replies::*, tasks::*};
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
pub const DATA: &[u8] = b"original native file bytes\0\xff\x80\n";
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

const HELPER_RECEIPTS: [&str; 5] = ["admission", "read-error", "delivery", "task", "receipt"];
const HELPER_RECEIPT_BYTES: u64 = 8192;
#[derive(Clone, Copy)]
enum ReceiptBaseline {
    Absent,
    Present,
    Unavailable,
}
struct HelperObservation {
    root: PathBuf,
    baseline: [ReceiptBaseline; 5],
}
impl HelperObservation {
    fn new(root: PathBuf) -> Self {
        let baseline = HELPER_RECEIPTS.map(|name| {
            match fs::symlink_metadata(root.join(format!("file-mcp.{name}"))) {
                Ok(_) => ReceiptBaseline::Present,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    ReceiptBaseline::Absent
                }
                Err(_) => ReceiptBaseline::Unavailable,
            }
        });
        Self { root, baseline }
    }
    fn snapshot(&self) -> Value {
        let mut result = serde_json::Map::new();
        for (index, name) in HELPER_RECEIPTS.iter().enumerate() {
            result.insert((*name).into(), json!(self.receipt(index, name)));
        }
        Value::Object(result)
    }
    fn receipt(&self, index: usize, name: &str) -> &'static str {
        match self.baseline[index] {
            ReceiptBaseline::Present => return "preexisting",
            ReceiptBaseline::Unavailable => return "baseline_unavailable",
            ReceiptBaseline::Absent => {}
        }
        let path = self.root.join(format!("file-mcp.{name}"));
        match fs::symlink_metadata(&path) {
            Ok(metadata) if !metadata.file_type().is_file() => return "unsupported",
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return "absent",
            Err(_) => return "read_failed",
        }
        let mut bytes = Vec::new();
        if fs::File::open(path)
            .and_then(|file| file.take(HELPER_RECEIPT_BYTES + 1).read_to_end(&mut bytes))
            .is_err()
        {
            return "read_failed";
        }
        if bytes.len() > HELPER_RECEIPT_BYTES as usize {
            return "oversized";
        }
        let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
            return "malformed";
        };
        match name {
            "admission" | "read-error" => match value.get("isError").and_then(Value::as_bool) {
                Some(false) => "accepted",
                Some(true) => "rejected",
                None => "malformed",
            },
            "delivery" => match value.get("status").and_then(Value::as_str) {
                Some("queued") => "queued",
                Some("delivered") => "delivered",
                Some("failed") => "failed",
                Some("outcome_unknown") => "outcome_unknown",
                Some(_) => "unsupported",
                None => "malformed",
            },
            "task" => match value.get("status").and_then(Value::as_str) {
                Some("in_progress") => "in_progress",
                Some("done") => "done",
                Some(_) => "unsupported",
                None => "malformed",
            },
            "receipt" => match value.get("helper_exit").and_then(Value::as_bool) {
                Some(true) => "helper_exited",
                Some(false) => "exit_unconfirmed",
                None => "malformed",
            },
            _ => "unsupported",
        }
    }
}

pub struct Running {
    child: Option<Child>,
    observation: Rc<Cell<Observation>>,
    // Independent read offset, opened before this original child is spawned.
    // A later fixture rename/relaunch cannot substitute another child's output.
    stderr: fs::File,
    helper: HelperObservation,
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
            "helper":self.helper.snapshot(),"child":child,"stderr":stderr,"stderr_bytes":bytes.len().min(8192),"stderr_truncated":bytes.len()>8192,
            "bootstrap_phase":boundary_phase(&bytes, b"native startup boundary: ", &[
                "runtime_entered", "runtime_ready", "bootstrap_entered", "configuration_entered",
                "executable_verify_entered", "executable_hash_entered", "executable_hash_completed",
                "executable_verify_completed", "custody_entered",
                "domain_entered", "shared_entered", "files_entered", "app_entered",
                "bootstrap_ready", "bind_entered", "server_poll_entered", "driver_entered", "serving"]),
            "media_phase":boundary_phase(&bytes, b"native media boundary: ", &[
                "create_entered", "created", "existing", "create_refused", "private_entered",
                "private_policy_refused", "private_other_refused", "directory_open_entered",
                "store_entered", "store_refused", "store_ready"]),
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
fn boundary_phase(bytes: &[u8], marker: &[u8], labels: &[&'static str]) -> &'static str {
    // Closed vocabulary only. Preserve separate bootstrap and worker phases;
    // concurrent worker logging cannot overwrite the other operation's phase.
    bytes
        .split(|byte| *byte == b'\n')
        .filter_map(|line| {
            let start = line.windows(marker.len()).position(|part| part == marker)?;
            let value = &line[start + marker.len()..];
            labels.iter().copied().find(|label| {
                value
                    .strip_prefix(label.as_bytes())
                    .is_some_and(|rest| rest.iter().all(u8::is_ascii_whitespace))
            })
        })
        .next_back()
        .unwrap_or("unobserved")
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
pub struct Fixture {
    observation: Rc<Cell<Observation>>,
    pub root: tempfile::TempDir,
    pub state_dir: PathBuf,
    pub work: PathBuf,
    pub fake: common::Fake,
    pub address: SocketAddr,
    pub peer: crypto::Peer,
    pub room: String,
    pub ciphertext: Option<Vec<u8>>,
    pub uploads: usize,
    pub event_puts: usize,
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
impl Fixture {
    pub(super) fn phase(&self, phase: &'static str) {
        let mut observation = self.observation.get();
        observation.phase = phase;
        self.observation.set(observation);
    }
    pub(super) async fn next(&mut self, phase: &'static str) -> common::Request {
        self.phase(phase);
        let request = self.fake.next_phase(Some(phase)).await;
        let mut observation = self.observation.get();
        observation.requests = observation.requests.saturating_add(1);
        observation.last_http = match (request.method.as_str(), request.target.as_str()) {
            ("GET", "/_matrix/client/v3/account/whoami") => "whoami",
            ("GET", "/_matrix/client/versions") => "versions",
            ("GET", value) if value.starts_with("/_matrix/client/v3/sync?") => "sync",
            ("GET", value)
                if value.starts_with("/_matrix/client/v3/rooms/") && value.ends_with("/state") =>
            {
                "room_state"
            }
            ("POST", "/_matrix/client/v3/keys/query") => "keys_query",
            ("POST", "/_matrix/client/v3/keys/upload") => "keys_upload",
            ("POST", "/_matrix/client/v3/keys/device_signing/upload") => "device_signing",
            ("POST", "/_matrix/client/v3/keys/signatures/upload") => "signatures",
            ("POST", "/_matrix/client/v3/keys/claim") => "keys_claim",
            ("POST", "/_matrix/media/v3/upload") => "media_upload",
            ("PUT", value) if value.starts_with("/_matrix/client/v3/sendToDevice/") => "to_device",
            ("PUT", value)
                if value.starts_with("/_matrix/client/v3/rooms/") && value.contains("/send/") =>
            {
                "room_event"
            }
            _ => "other",
        };
        self.observation.set(observation);
        request
    }
    /// An actual new native task client inherits the original disposable
    /// caller's context. This grants no new capability or current authority.
    pub(super) async fn historical_file(&self, delivery_id: &str) -> Value {
        self.phase("historical.mcp");
        let bytes = private::read_secret(&self.work.join("file-mcp.context")).unwrap();
        assert!(bytes.len() <= 8192);
        let inherited: std::collections::BTreeMap<String, String> =
            serde_json::from_slice(&bytes).unwrap();
        assert_eq!(inherited.len(), 4);
        let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_hagency"));
        command
            .arg("mcp")
            .env_clear()
            .kill_on_drop(true)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for key in [
            "HAGENCY_RUNNER_API_ADDR",
            "HAGENCY_RUNNER_CAPABILITY",
            "HAGENCY_TASK_ID",
            "HAGENCY_FILE_TOOLS",
        ] {
            command.env(
                key,
                inherited.get(key).expect("original fixed context field"),
            );
        }
        #[cfg(windows)]
        command.env("SystemRoot", std::env::var_os("SystemRoot").unwrap());
        let mut child = command.spawn().unwrap();
        let mut input = child.stdin.take().unwrap();
        let mut output = child.stdout.take().unwrap();
        let mut errors = child.stderr.take().unwrap();
        for message in [
            json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"historical-file-fixture","version":"1"}}}),
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"get_file_delivery","arguments":{"delivery_id":delivery_id}}}),
        ] {
            let mut bytes = serde_json::to_vec(&message).unwrap();
            bytes.push(b'\n');
            input.write_all(&bytes).await.unwrap();
        }
        drop(input);
        let status = match tokio::time::timeout(Duration::from_secs(5), child.wait()).await {
            Ok(status) => status.unwrap(),
            Err(_) => {
                child.kill().await.unwrap();
                child.wait().await.unwrap();
                panic!("original-context historical MCP did not finish");
            }
        };
        assert!(status.success());
        let mut stdout = Vec::new();
        (&mut output)
            .take(16385)
            .read_to_end(&mut stdout)
            .await
            .unwrap();
        assert!(stdout.len() <= 16384);
        let mut stderr = Vec::new();
        (&mut errors)
            .take(4097)
            .read_to_end(&mut stderr)
            .await
            .unwrap();
        assert!(stderr.is_empty());
        let replies = stdout
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| serde_json::from_slice::<Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(replies.len(), 2);
        assert_eq!(replies[0]["id"], 0);
        assert!(replies[0].get("error").is_none());
        assert_eq!(replies[1]["id"], 1);
        assert!(replies[1].get("error").is_none());
        replies[1]["result"].clone()
    }

    pub async fn deliver(&mut self) -> Value {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(40);
        let mut requests = 0usize;
        loop {
            if let Ok(bytes) = fs::read(self.work.join("file-mcp.receipt")) {
                return serde_json::from_slice(&bytes).unwrap();
            }
            if let Ok(request) =
                tokio::time::timeout(Duration::from_millis(50), self.next("delivery.http")).await
            {
                requests += 1;
                assert!(requests <= 96, "unbounded native protocol loop");
                self.respond(request).await;
                continue;
            }
            // The model can atomically finish its receipt during the request
            // wait. Runtime/canonical settlement is a separate later boundary.
            if let Ok(bytes) = fs::read(self.work.join("file-mcp.receipt")) {
                return serde_json::from_slice(&bytes).unwrap();
            }
            let status = self.capabilities().await["development_execution"].clone();
            assert!(
                !matches!(
                    status["state"].as_str(),
                    Some("unavailable" | "outcome_unknown" | "no_work")
                ),
                "actual file workflow refused; this is not a positive delivery: {status}; key writes={}, claims={}, shares={}, upload={}",
                self.peer.writes.len(),
                self.peer.claims,
                self.peer.shares,
                self.ciphertext.is_some()
            );
            assert!(
                tokio::time::Instant::now() < deadline,
                "actual file workflow did not deliver: {status}"
            );
        }
    }

    pub(super) async fn respond(&mut self, request: common::Request) {
        self.phase("response.apply");
        assert_eq!(
            request.headers.get("authorization"),
            Some(&format!("Bearer {}", common::TOKEN))
        );
        assert!(!request.target.contains(common::TOKEN));
        match (request.method.as_str(), request.target.as_str()) {
            ("GET", "/_matrix/client/v3/account/whoami") => request.json(200, common::who()),
            ("GET", target) if target.starts_with("/_matrix/client/v3/sync?") => {
                request.json(200, common::sync("native-file"))
            }
            ("GET", target)
                if target.starts_with("/_matrix/client/v3/rooms/")
                    && target.ends_with("/state") =>
            {
                request.json(200, common::state())
            }
            ("POST", "/_matrix/media/v3/upload") => {
                self.observe_upload(&request);
                request.json(200, json!({"content_uri":"mxc://example.test/native-file"}));
            }
            ("PUT", target)
                if target.starts_with("/_matrix/client/v3/sendToDevice/m.room.encrypted/") =>
            {
                self.peer
                    .share(serde_json::from_slice(&request.body).unwrap())
                    .await;
                request.json(200, json!({}));
            }
            ("PUT", target)
                if target.starts_with("/_matrix/client/v3/rooms/")
                    && target.contains("/send/m.room.encrypted/") =>
            {
                self.observe_event(&request).await;
                request.json(200, json!({"event_id":"$native_file_accepted"}));
            }
            _ => {
                let body = if request.body.is_empty() {
                    Value::Null
                } else {
                    serde_json::from_slice(&request.body).unwrap()
                };
                let (status, response) = self
                    .peer
                    .protocol(&request.method, &request.target, &body)
                    .await
                    .unwrap_or_else(|| {
                        panic!(
                            "unhandled actual Matrix route {} {}",
                            request.method, request.target
                        )
                    });
                request.json(status, response);
            }
        }
    }

    pub(super) fn observe_upload(&mut self, request: &common::Request) {
        assert_eq!(
            request.headers.get("authorization"),
            Some(&format!("Bearer {}", common::TOKEN))
        );
        assert_eq!(request.method, "POST");
        assert_eq!(request.target, "/_matrix/media/v3/upload");
        assert!(self.ciphertext.is_none(), "duplicate media POST");
        assert_eq!(request.headers["content-type"], "application/octet-stream");
        assert_eq!(request.body.len(), DATA.len());
        assert_ne!(request.body, DATA);
        self.uploads += 1;
        assert_eq!(self.uploads, 1);
        self.ciphertext = Some(request.body.clone());
    }
    pub(super) async fn observe_event(&mut self, request: &common::Request) {
        assert_eq!(
            request.headers.get("authorization"),
            Some(&format!("Bearer {}", common::TOKEN))
        );
        assert_eq!(request.method, "PUT");
        assert!(
            request.target.starts_with("/_matrix/client/v3/rooms/")
                && request.target.contains("/send/m.room.encrypted/")
        );
        assert!(self.ciphertext.is_some());
        self.event_puts += 1;
        assert_eq!(self.event_puts, 1, "duplicate encrypted event PUT");
        self.peer
            .decrypt(
                serde_json::from_slice(&request.body).unwrap(),
                ruma::RoomId::parse(&self.room).unwrap().as_ref(),
            )
            .await;
        // Even actual recipient decryption cannot replace the event ACK.
        assert_eq!(self.delivered_count(), 0);
    }
    /// Retain the actual server response sender while the original service
    /// awaits it. This pauses no other task and supplies no synthetic outcome.
    pub(super) async fn pause_write(&mut self, event: bool) -> common::Request {
        let until = tokio::time::Instant::now() + Duration::from_secs(20);
        for _ in 0..96 {
            let request = tokio::time::timeout_at(
                until,
                self.next(if event {
                    "uncertainty.event"
                } else {
                    "uncertainty.upload"
                }),
            )
            .await
            .expect("actual native write was not reached");
            let selected = if event {
                request.method == "PUT"
                    && request.target.starts_with("/_matrix/client/v3/rooms/")
                    && request.target.contains("/send/m.room.encrypted/")
            } else {
                request.method == "POST" && request.target == "/_matrix/media/v3/upload"
            };
            if selected {
                if event {
                    self.observe_event(&request).await;
                } else {
                    self.observe_upload(&request);
                }
                return request;
            }
            self.respond(request).await;
        }
        panic!("bounded original write request count exceeded");
    }

    pub(super) fn delivered_count(&self) -> u64 {
        self.sql()
            .query_row(
                "SELECT COUNT(*) FROM file_deliveries WHERE event_state='delivered'",
                [],
                |row| row.get(0),
            )
            .unwrap()
    }
    pub async fn new(direct: bool) -> Self {
        let peer = crypto::Peer::new().await;
        let room = if direct {
            "!direct:example.test"
        } else {
            "!project:example.test"
        };
        let privacy = if direct {
            RoomPrivacy::Direct {
                human_mxid: "@owner:example.test".into(),
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
                room_id: room.into(),
                generation: 1,
                privacy: privacy.clone(),
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
                room_id: room.into(),
                thread_root: (!direct).then(|| "$task_thread".into()),
            },
            now(),
        )
        .unwrap();
        db.create_canonical_task("task", "session", "Native bootstrap task", now())
            .unwrap();
        db.register_workspace("work").unwrap();
        db.enqueue_dispatch(&DispatchInput{id:"dispatch".into(),session_id:"session".into(),task_id:Some("task".into()),resources:vec![ResourceLease{id:"work".into(),exclusive:true}],payload:json!({"instruction":"Send sample.bin using native MCP and inspect its delivery status. Leave the canonical task in progress."})}).unwrap();
        drop(db); // No issued cap, Started restoration, SQL availability or handoff.
        let fake = common::Fake::start(true).await;
        let executable = PathBuf::from(env!("CARGO_BIN_EXE_hagency-file-mcp-probe"))
            .canonicalize()
            .unwrap();
        let executable_sha256: String = Sha256::digest(fs::read(&executable).unwrap())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let config = json!({"profile":"codex_app_server_development_v1","send_file":true,"executable":executable,"executable_sha256":executable_sha256,"workspaces":{"work":work},"file_limit":4194304,"operation_ms":10000,"response_ms":1500,
            "matrix":{"origin":fake.endpoint,"server_name":"example.test","registration_fingerprint":"a".repeat(64),"engagement_id":e.id,"registration_generation":1,"transport_generation":1,"sender_mxid":"@worker:example.test","device_id":"DEVICE_1","crypto_enrollment":{"profile":"fresh_own_account_v1","peer_masters":[{"user_id":"@owner:example.test","master_key":peer.anchor()}]},"rooms":[{"id":room,"generation":1,"privacy":privacy}]}});
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
        private::write_new(&work.join("sample.bin"), DATA).unwrap();
        Self {
            observation: Rc::new(Cell::new(Observation::new("unlaunched"))),
            peer,
            room: room.into(),
            ciphertext: None,
            uploads: 0,
            event_puts: 0,
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
            .env("RUST_LOG", "info,hagency_startup_observation=trace")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        command
    }
    pub fn launch(&mut self, enabled: bool, variant: &'static str) -> Running {
        // A restarted child gets a distinct observation; it cannot change the
        // original child's retained evidence even if their fixture is shared.
        self.observation = Rc::new(Cell::new(Observation::new(variant)));
        let stderr = self.root.path().join("native.stderr");
        let file = private::open(&stderr, true).unwrap();
        let stderr = fs::File::open(&stderr).unwrap();
        let helper = HelperObservation::new(self.work.clone());
        Running {
            helper,
            child: Some(
                self.command(enabled)
                    .stderr(Stdio::from(file))
                    .spawn()
                    .unwrap(),
            ),
            observation: self.observation.clone(),
            stderr,
        }
    }
    pub(super) fn sql(&self) -> rusqlite::Connection {
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
                    let value: Value =
                        serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
                    let mut observation = self.observation.get();
                    observation.status = match value["development_execution"]["state"].as_str() {
                        Some("disabled") => "disabled",
                        Some("configured") => "configured",
                        Some("refreshing") => "refreshing",
                        Some("enrolling") => "enrolling",
                        Some("claiming") => "claiming",
                        Some("registering") => "registering",
                        Some("running") => "running",
                        Some("completed") => "completed",
                        Some("unavailable") => "unavailable",
                        Some("outcome_unknown") => "outcome_unknown",
                        Some("no_work") => "no_work",
                        Some("closed") => "closed",
                        _ => "other",
                    };
                    self.observation.set(observation);
                    return value;
                }
            }
            assert!(
                tokio::time::Instant::now() < until,
                "native bootstrap did not serve capabilities; original child evidence follows on panic cleanup"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
    pub async fn wait_result(&self) -> Value {
        self.phase("driver.result");
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

#[tokio::test]
async fn native_file_service_media_startup_observation() {
    let mut f = Fixture::new(false).await;
    let media = f.state_dir.join("file-media");
    private::directory(&media).unwrap();
    let mut child = f.launch(true, "observation.media_refused");
    // Bound the actual peer requests, not the idle polls: a slow hosted
    // enrollment must not exhaust the loop while the child is still working.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    let mut status = Value::Null;
    let mut requests = 0;
    loop {
        assert!(
            tokio::time::Instant::now() < deadline,
            "media refusal was not reported; last state {}",
            status["state"]
        );
        if let Ok(request) =
            tokio::time::timeout(Duration::from_millis(50), f.next("observation.media_setup")).await
        {
            requests += 1;
            assert!(requests <= 96);
            f.respond(request).await;
        } else {
            status = f.capabilities().await["development_execution"].clone();
            if status["state"] == "outcome_unknown" {
                break;
            }
            assert_ne!(status["state"], "unavailable");
        }
    }
    assert_eq!(status["state"], "outcome_unknown");
    assert_eq!(f.attempts(), 0);
    assert!(!media.join("media.journal").exists());
    assert_eq!(f.uploads, 0);
    let snapshot = child.snapshot();
    assert_eq!(snapshot["bootstrap_phase"], "serving");
    assert_eq!(snapshot["media_phase"], "store_refused");
    assert_eq!(snapshot["child"]["state"], "running");
    assert_eq!(snapshot["stderr_truncated"], false);
    assert!(serde_json::to_vec(&snapshot).unwrap().len() <= 1024);
    f.fake.no_request().await;
    child.stop_and_reap();
    f.fake.close().await;
}

#[tokio::test]
async fn native_file_service_original_observation() {
    let mut f = Fixture::new(false).await;
    let mut child = f.launch(true, "observation.live");
    let request = f.next("observation.whoami").await;
    assert_eq!(request.target, "/_matrix/client/v3/account/whoami");
    // Holding the real response keeps the original service running. Merely
    // waiting for another request must not fabricate a process exit.
    let live = child.snapshot();
    assert_eq!(live["child"]["state"], "running");
    assert_eq!(live["variant"], "observation.live");
    assert_eq!(live["phase"], "observation.whoami");
    assert_eq!(live["last_http"], "whoami");
    assert_eq!(live["requests"], 1);
    assert!(
        live["helper"]
            .as_object()
            .unwrap()
            .values()
            .all(|state| state == "absent")
    );
    // The spawned Driver can issue its first request before the parent logs
    // serving. Both fixed phases follow original bind and initial server poll.
    assert!(matches!(
        live["bootstrap_phase"].as_str(),
        Some("driver_entered" | "serving")
    ));
    assert!(serde_json::to_vec(&live).unwrap().len() <= 1024);
    let original = child.observation.clone();
    child.stop_and_reap();
    drop(request);

    fs::rename(
        f.root.path().join("native.stderr"),
        f.root.path().join("native-observation-first.stderr"),
    )
    .unwrap();
    // This is a real startup refusal, not an injected process/status result.
    fs::write(f.state_dir.join("development-driver.json"), b"{}").unwrap();
    let mut child = f.launch(true, "observation.refused");
    assert!(!Rc::ptr_eq(&original, &child.observation));
    f.phase("observation.startup");
    // Reuse the existing fixture startup watchdog; no original deadline changes.
    tokio::time::timeout(STARTUP_WATCHDOG, async {
        while child.child.as_mut().unwrap().try_wait().unwrap().is_none() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("actual invalid-configuration child must exit");
    let exited = child.snapshot();
    assert_eq!(exited["child"]["state"], "exited");
    assert_eq!(exited["child"]["success"], false);
    assert_eq!(exited["stderr"], "config");
    assert_eq!(exited["requests"], 0);
    assert_eq!(exited["last_http"], "none");
    assert_eq!(exited["bootstrap_phase"], "configuration_entered");
    assert_eq!(exited["media_phase"], "unobserved");
    assert!(
        exited["helper"]
            .as_object()
            .unwrap()
            .values()
            .all(|state| state == "absent")
    );
    fs::rename(
        f.root.path().join("native.stderr"),
        f.root.path().join("native-observation-refused.stderr"),
    )
    .unwrap();
    private::write_new(&f.root.path().join("native.stderr"), b"Error: Startup\n").unwrap();
    assert_eq!(child.snapshot()["stderr"], "config");
    assert_eq!(child.snapshot()["bootstrap_phase"], "configuration_entered");
    assert_eq!(original.get().variant, "observation.live");
    assert_eq!(original.get().phase, "observation.whoami");
    let rendered = serde_json::to_string(&exited).unwrap();
    assert!(rendered.len() <= 1024);
    for private in [common::TOKEN, f.state_dir.to_str().unwrap(), &f.room] {
        assert!(!rendered.contains(private));
    }
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let _original_child = child;
        panic!("fixture original failure retained");
    }))
    .unwrap_err();
    assert_eq!(
        panic.downcast_ref::<&'static str>(),
        Some(&"fixture original failure retained")
    );
    f.fake.no_request().await;
    f.fake.close().await;
}

#[test]
fn native_file_service_helper_observation_bounds() {
    let root = tempfile::tempdir().unwrap();
    let original = HelperObservation::new(root.path().into());
    assert!(
        original
            .snapshot()
            .as_object()
            .unwrap()
            .values()
            .all(|v| v == "absent")
    );
    fs::write(
        root.path().join("file-mcp.admission"),
        br#"{"isError":false,"private":"DO_NOT_PROJECT_SECRET"}"#,
    )
    .unwrap();
    fs::write(
        root.path().join("file-mcp.read-error"),
        br#"{"isError":true}"#,
    )
    .unwrap();
    fs::write(
        root.path().join("file-mcp.delivery"),
        br#"{"status":"delivered","delivery_id":"PRIVATE_ID"}"#,
    )
    .unwrap();
    fs::write(
        root.path().join("file-mcp.task"),
        br#"{"status":"in_progress","id":"PRIVATE_TASK"}"#,
    )
    .unwrap();
    fs::write(
        root.path().join("file-mcp.receipt"),
        br#"{"helper_exit":true}"#,
    )
    .unwrap();
    let snapshot = original.snapshot();
    assert_eq!(
        snapshot,
        json!({"admission":"accepted","read-error":"rejected","delivery":"delivered","task":"in_progress","receipt":"helper_exited"})
    );
    let rendered = serde_json::to_string(&snapshot).unwrap();
    assert!(rendered.len() < 256 && !rendered.contains("PRIVATE") && !rendered.contains("SECRET"));
    // A new launch cannot relabel old helper receipts as its own observations.
    let later = HelperObservation::new(root.path().into());
    assert!(
        later
            .snapshot()
            .as_object()
            .unwrap()
            .values()
            .all(|v| v == "preexisting")
    );
    for (bytes, expected) in [
        (b"{".to_vec(), "malformed"),
        (
            br#"{"status":"future_state_PRIVATE"}"#.to_vec(),
            "unsupported",
        ),
        (br#"{"status":null}"#.to_vec(), "malformed"),
        (vec![b'x'; HELPER_RECEIPT_BYTES as usize + 1], "oversized"),
    ] {
        fs::write(root.path().join("file-mcp.delivery"), bytes).unwrap();
        assert_eq!(original.snapshot()["delivery"], expected);
    }
    fs::remove_file(root.path().join("file-mcp.delivery")).unwrap();
    fs::create_dir(root.path().join("file-mcp.delivery")).unwrap();
    assert_eq!(original.snapshot()["delivery"], "unsupported");
    let unavailable = HelperObservation {
        root: root.path().into(),
        baseline: [ReceiptBaseline::Unavailable; 5],
    };
    assert!(
        unavailable
            .snapshot()
            .as_object()
            .unwrap()
            .values()
            .all(|v| v == "baseline_unavailable")
    );
}
