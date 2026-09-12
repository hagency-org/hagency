use super::peer::{self, Fake, Request};
use hagency_core::{authority::Registration, project::Resource};
use hagency_store::{DomainRepository, Repository, private};
use serde_json::{Value, json};
use std::{
    fs,
    net::SocketAddr,
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub fn registration() -> Registration {
    Registration {
        fleet_id: peer::FLEET.into(),
        generation: 7,
        server_name: "matrix.example.test".into(),
        representative_mxid: format!("@{}_representative:matrix.example.test", peer::FLEET),
        approval_bot_mxid: "@approval:matrix.example.test".into(),
        reception_room_id: "!reception:matrix.example.test".into(),
    }
}
pub fn resource() -> Resource {
    serde_json::from_value(resource_body(true)).unwrap()
}
pub fn resource_body(published: bool) -> Value {
    // Public mutation accepts provider input, never Resource's derived roles cache.
    json!({"presetId":"private_preset","seatId":"private_seat",
        "framework":"codex","model":"gpt-5.6-sol","reasoning":"medium",
        "ceiling":{"tokens":1000,"period":"monthly"},"published":published})
}
pub struct Running(Child);
impl Running {
    pub async fn refused(&mut self) {
        let until = tokio::time::Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(status) = self.0.try_wait().unwrap() {
                assert!(!status.success());
                return;
            }
            assert!(
                tokio::time::Instant::now() < until,
                "original invalid-config child did not exit"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
    #[cfg(unix)]
    pub async fn graceful(&mut self) {
        assert!(
            Command::new("/bin/kill")
                .args(["-TERM", &self.0.id().to_string()])
                .status()
                .unwrap()
                .success()
        );
        let until = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = self.0.try_wait().unwrap() {
                assert!(status.success());
                return;
            }
            assert!(
                tokio::time::Instant::now() < until,
                "original service did not shut down"
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
    pub state: PathBuf,
    pub address: SocketAddr,
    pub fake: Fake,
}
impl Fixture {
    pub async fn new(registered: bool, published: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("state");
        let init = Command::new(env!("CARGO_BIN_EXE_hagency"))
            .args(["init", "--state-dir"])
            .arg(&state)
            .output()
            .unwrap();
        assert!(init.status.success(), "actual native init refused");
        let mut db = DomainRepository::open(&state).unwrap();
        if registered {
            db.register(&registration()).unwrap();
        }
        if published {
            db.put_resource(&resource()).unwrap();
        }
        drop(db);
        let fake = Fake::start(true).await;
        let config = json!({"profile":"palpo_v2_resources_v1", "endpoint":fake.endpoint,
            "registration":registration(),"machine_generation":31});
        private::write_new(
            &state.join("palpo-transport.json"),
            &serde_json::to_vec(&config).unwrap(),
        )
        .unwrap();
        private::write_new(&state.join("palpo.machine_token"), peer::TOKEN.as_bytes()).unwrap();
        private::write_new(
            &state.join("palpo.ca.pem"),
            include_bytes!("../../../hagency-palpo/tests/fixtures/ca.pem"),
        )
        .unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        Self {
            root,
            state,
            address,
            fake,
        }
    }
    pub fn launch(&self, enabled: bool) -> Running {
        let mut command = Command::new(env!("CARGO_BIN_EXE_hagency"));
        command.env_clear();
        if let Some(system) = std::env::var_os("SystemRoot") {
            command.env("SystemRoot", system);
        }
        command
            .args(["serve", "--state-dir"])
            .arg(&self.state)
            .args(["--listen", &self.address.to_string()]);
        if enabled {
            command.arg("--palpo-transport");
        }
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(
                private::open(&self.root.path().join("native.stderr"), true).unwrap(),
            ));
        Running(command.spawn().unwrap())
    }
    pub async fn request(&self, method: &str, path: &str, value: Option<Value>) -> Value {
        let until = tokio::time::Instant::now() + Duration::from_secs(15);
        let mut stream = loop {
            if let Ok(stream) = tokio::net::TcpStream::connect(self.address).await {
                break stream;
            }
            assert!(
                tokio::time::Instant::now() < until,
                "original service did not listen"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        };
        let token =
            String::from_utf8(private::read_secret(&self.state.join("operator.token")).unwrap())
                .unwrap();
        let body = value.map_or_else(String::new, |v| v.to_string());
        stream.write_all(format!("{method} /api/native/v1/{path} HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",self.address,body.len()).as_bytes()).await.unwrap();
        let mut bytes = Vec::new();
        tokio::time::timeout(
            Duration::from_secs(2),
            stream.take(65537).read_to_end(&mut bytes),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(bytes.len() <= 65536);
        let response = std::str::from_utf8(&bytes).unwrap();
        assert_eq!(
            response
                .split_whitespace()
                .nth(1)
                .and_then(|s| s.parse::<u16>().ok()),
            Some(200),
            "native API refused fixed fixture request"
        );
        serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap()
    }
    pub async fn capabilities(&self) -> Value {
        self.request("GET", "capabilities", None).await
    }
    pub async fn terminal(&self) -> Value {
        let until = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            let value = self.capabilities().await;
            if matches!(
                value["palpo_publication"]["state"].as_str(),
                Some("unavailable" | "outcome_unknown" | "stopped")
            ) {
                return value;
            }
            assert!(
                tokio::time::Instant::now() < until,
                "original worker did not report terminal state"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
    pub async fn publication(&mut self) -> Request {
        // Defaults publish every 15s. These are finite fixture observation
        // bounds; the production interval, retry and HTTP deadlines are unchanged.
        let until = tokio::time::Instant::now() + Duration::from_secs(25);
        for _ in 0..768 {
            assert!(
                tokio::time::Instant::now() < until,
                "original publication not observed"
            );
            let request = self.fake.next().await;
            if request.target.contains("/poll?") {
                request.json(200, peer::empty(31));
            } else {
                assert!(request.target.ends_with("/updates"));
                return request;
            }
        }
        panic!("original fixture request observation capacity exhausted");
    }
    pub fn sql(&self, name: &str) -> rusqlite::Connection {
        rusqlite::Connection::open_with_flags(
            self.state.join(name),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap()
    }
    pub fn pending(&self) -> (u64, String, String, String) {
        self.sql("custody.sqlite3").query_row(
            "SELECT sequence,digest,body,state FROM outbound_publications WHERE binding='native-palpo-v2'", [],
            |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).unwrap()
    }
    pub fn untouched_registration(&self, expected: bool) {
        let encoded: Option<String> = self
            .sql("domain.sqlite3")
            .query_row(
                "SELECT config FROM registrations WHERE fleet_id=?1",
                [peer::FLEET],
                |r| r.get(0),
            )
            .optional()
            .unwrap();
        assert_eq!(
            encoded.map(|v| serde_json::from_str::<Registration>(&v).unwrap()),
            expected.then(registration)
        );
        let active: u64 = self
            .sql("custody.sqlite3")
            .query_row("SELECT COUNT(*) FROM outbound_transports", [], |r| r.get(0))
            .unwrap();
        assert_eq!(active, 0);
    }
    pub fn reopen(&self) {
        drop(DomainRepository::open(&self.state).unwrap());
        drop(Repository::open(&self.state).unwrap());
    }
    pub fn no_runner(&self) {
        assert!(!self.state.join("runtime-home").exists());
        assert!(!self.state.join("sdk").exists());
        assert_eq!(
            self.sql("domain.sqlite3")
                .query_row("SELECT COUNT(*) FROM runner_attempts", [], |r| r
                    .get::<_, u64>(0))
                .unwrap(),
            0
        );
    }
    pub fn rewrite(&self, change: impl FnOnce(&mut Value)) {
        let path = self.state.join("palpo-transport.json");
        let mut value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        change(&mut value);
        fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
    }
}
use rusqlite::OptionalExtension;

pub fn check(request: &Request, sequence: u64) -> Value {
    assert_eq!(request.method, "POST");
    assert_eq!(
        request.headers["authorization"],
        format!("Bearer {}", peer::TOKEN)
    );
    assert_eq!(request.headers["x-hagency-generation"], "31");
    let value = request.value();
    assert_eq!(value["generation"], 31);
    assert_eq!(value["sequence"], sequence);
    assert_eq!(value["capabilities"]["fleetId"], peer::FLEET);
    assert_eq!(
        value["capabilities"]["representativeMxid"],
        registration().representative_mxid
    );
    for private in [
        peer::TOKEN,
        "private_preset",
        "private_seat",
        "receptionRoomId",
    ] {
        assert!(!value.to_string().contains(private));
    }
    value
}
pub fn offers(value: &Value) -> &[Value] {
    value["capabilities"]["offers"].as_array().unwrap()
}
