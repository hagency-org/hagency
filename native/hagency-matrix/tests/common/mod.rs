#![allow(dead_code)]
use hagency_core::replies::*;
use hagency_matrix::{CancellationToken, HostConfig, HostIdentity, HostRoom, Limits};
use hagency_store::{DomainRepository, DomainStore, EffectOutcome};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fmt::Debug, future::Future, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::TcpListener,
    sync::{mpsc, oneshot},
    task::{JoinHandle, JoinSet},
    time::timeout,
};
use tokio_rustls::{
    TlsAcceptor,
    rustls::{
        self,
        pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer},
    },
};
#[path = "../../../hagency-store/tests/common/mod.rs"]
pub mod domain;
pub mod stall;
pub const TOKEN: &str = "synthetic-Matrix-token-not-real";
/// Tight fixture bounds for the transport-bound tests of this crate, which
/// deliberately drive slow headers, idle bodies and deadlines against them.
pub fn limits() -> Limits {
    Limits {
        connect: Duration::from_millis(300),
        headers: Duration::from_millis(400),
        request: Duration::from_millis(900),
        body_idle: Duration::from_millis(200),
        sdk: Duration::from_secs(10),
        ..Limits::default()
    }
}
/// Fixture orchestration bounds for the service-level tests that drive a fake
/// peer through `scripted()` on one current-thread runtime, eight runtimes per
/// process under the whole-package Windows probe. There the tight bounds above
/// were reached by scheduler starvation, not by the product. Every value stays
/// strictly below `hagency_matrix::Limits::default()`, which the production
/// binary runs with, so a real transport refusal still fails a test. The values
/// match the suite-local precedent in `tests/approval_delivery/fixture.rs`.
pub fn load_limits() -> Limits {
    Limits {
        connect: Duration::from_secs(2),
        headers: Duration::from_secs(2),
        request: Duration::from_secs(4),
        body_idle: Duration::from_secs(1),
        sdk: Duration::from_secs(20),
        ..Limits::default()
    }
}
pub struct Fixture {
    pub root: tempfile::TempDir,
    pub store: DomainStore,
    pub identity: HostIdentity,
}
impl Fixture {
    pub fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut db = DomainRepository::open(&root.path().join("domain")).unwrap();
        db.register(&domain::registration()).unwrap();
        let pool = domain::resource("pool", "seat", 1000);
        db.put_resource(&pool).unwrap();
        let p = domain::proof(&domain::request("worker", "Worker", &pool, 100));
        let e = db.admit(&p, 1000).unwrap();
        db.approve("approve", &p, 1000).unwrap();
        let effect = db.claim_effect().unwrap().unwrap();
        db.observe_effect(
            &effect.id,
            effect.fence,
            &EffectOutcome::Applied {
                receipt: "fixture account".into(),
            },
        )
        .unwrap();
        Self {
            root,
            store: DomainStore::start(db, 32).unwrap(),
            identity: HostIdentity {
                server_name: "example.test".into(),
                registration_fingerprint: "a".repeat(64),
                transport: MatrixTransportObservation {
                    engagement_id: e.id,
                    registration_generation: 1,
                    generation: 1,
                    sender_mxid: "@worker:example.test".into(),
                    device_id: "DEVICE_1".into(),
                },
            },
        }
    }
    pub fn config(&self, endpoint: &str) -> HostConfig {
        self.config_with(endpoint, limits())
    }
    /// The same host configuration with the load bounds, for service-level
    /// fixtures that share their runtime with the fake peer.
    pub fn config_under_load(&self, endpoint: &str) -> HostConfig {
        self.config_with(endpoint, load_limits())
    }
    fn config_with(&self, endpoint: &str, limits: Limits) -> HostConfig {
        HostConfig::new(
            self.identity.clone(),
            endpoint,
            TOKEN,
            self.root.path().join("sdk"),
            [42; 32],
            vec![HostRoom {
                room_id: "!direct:example.test".into(),
                generation: 1,
                privacy: RoomPrivacy::Direct {
                    human_mxid: "@owner:example.test".into(),
                },
            }],
            limits,
        )
        .unwrap()
    }
    pub async fn available(&self) -> bool {
        self.store
            .matrix_transport_state(self.identity.transport.engagement_id.clone())
            .await
            .unwrap()
            .is_some_and(|s| s.available)
    }
}
pub fn who() -> Value {
    json!({"user_id":"@worker:example.test","device_id":"DEVICE_1","is_guest":false})
}
pub fn sync(token: &str) -> Value {
    json!({"next_batch":token,"rooms":{"join":{}},"to_device":{"events":[]}})
}
pub fn state() -> Value {
    json!([
     {"type":"m.room.member","state_key":"@worker:example.test","content":{"membership":"join"}},
     {"type":"m.room.member","state_key":"@owner:example.test","content":{"membership":"join"}},
     {"type":"m.room.join_rules","state_key":"","content":{"join_rule":"invite"}},
     {"type":"m.room.encryption","state_key":"","content":{"algorithm":"m.megolm.v1.aes-sha2"}}
    ])
}
pub async fn success(fake: &mut Fake, token: &str) {
    let request = fake.next().await;
    assert_eq!(request.method, "GET");
    assert_eq!(request.target, "/_matrix/client/v3/account/whoami");
    assert_eq!(request.headers["authorization"], format!("Bearer {TOKEN}"));
    assert!(!request.headers.contains_key("x-hagency-generation"));
    assert!(request.body.is_empty());
    request.json(200, who());
    let request = fake.next().await;
    assert!(
        request
            .target
            .starts_with("/_matrix/client/v3/sync?timeout=0&full_state=true&filter=")
    );
    assert!(!request.target.contains(TOKEN));
    request.json(200, sync(token));
    let request = fake.next().await;
    assert!(request.target.starts_with("/_matrix/client/v3/rooms/"));
    assert!(request.target.ends_with("/state"));
    request.json(200, state());
}
/// A script must finish issuing its responses before collection settles. Report
/// an early collector failure directly instead of waiting for an HTTP request
/// that it will never make. Prefer the completed script when both are ready.
pub async fn scripted<C, S>(collector: C, script: S) -> (C::Output, S::Output)
where
    C: Future,
    C::Output: Debug,
    S: Future,
{
    // Diagnostic only: an early collector settlement is reported with its
    // elapsed time against the fixture bounds, so a starved fake peer under
    // whole-package load can be told from a real refusal.
    let started = std::time::Instant::now();
    tokio::pin!(collector, script);
    tokio::select! {
        biased;
        output = &mut script => (collector.await, output),
        output = &mut collector => panic!(
            "collector completed before its HTTP script: {output:?} (after {:?}; fixture request bounds {:?} tight, {:?} load)",
            started.elapsed(),
            limits().request,
            load_limits().request
        ),
    }
}
/// Observe the original domain shutdown once. A failure stays a failure; late
/// worker cleanup or a future reopen cannot replace this Result. A timeout is
/// named as the stall with the `[shutdown-stall]` token and recorded in the
/// process-wide latch so sibling witnesses can attribute their own failures.
pub async fn shutdown_domain(store: &DomainStore, label: &'static str) {
    let (result, snapshot) = store.shutdown_observed().await;
    let outcome = format!("{:?}", snapshot.outcome);
    if let Err(error) = &result {
        if stall::timed_out(&outcome) {
            eprintln!("[shutdown-stall] {outcome} at {label}; snapshot {snapshot:?}");
            stall::record_if_timed_out(&outcome, label, format!("{snapshot:?}"), None);
        }
        panic!("domain shutdown {label} ({outcome}): {error:?}; {snapshot:?}");
    }
}
struct ScriptedResponse {
    pieces: Vec<(Duration, Vec<u8>)>,
    clean: bool,
}
pub struct Request {
    pub method: String,
    pub target: String,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
    response: oneshot::Sender<ScriptedResponse>,
}
impl Request {
    pub fn json(self, status: u16, body: Value) {
        self.raw(response(status, &serde_json::to_vec(&body).unwrap()));
    }
    pub fn raw(self, bytes: Vec<u8>) {
        let _ = self.response.send(ScriptedResponse {
            pieces: vec![(Duration::ZERO, bytes)],
            clean: true,
        });
    }
    /// Deliberately omit TLS close_notify to exercise truncated transport EOF.
    pub fn unclean(self, bytes: Vec<u8>) {
        let _ = self.response.send(ScriptedResponse {
            pieces: vec![(Duration::ZERO, bytes)],
            clean: false,
        });
    }
    pub fn chunks(self, pieces: Vec<(Duration, Vec<u8>)>) {
        let _ = self.response.send(ScriptedResponse {
            pieces,
            clean: true,
        });
    }
}
pub fn response(status: u16, body: &[u8]) -> Vec<u8> {
    let mut wire = format!("HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).into_bytes();
    wire.extend_from_slice(body);
    wire
}
pub struct Fake {
    pub endpoint: String,
    requests: mpsc::Receiver<Request>,
    stop: CancellationToken,
    task: JoinHandle<()>,
}
impl Fake {
    pub async fn start(tls: bool) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!(
            "{}://{}/",
            if tls { "https" } else { "http" },
            listener.local_addr().unwrap()
        );
        let acceptor = tls.then(|| {
            let provider = Arc::new(rustls::crypto::ring::default_provider());
            let config = rustls::ServerConfig::builder_with_provider(provider)
                .with_safe_default_protocol_versions()
                .unwrap()
                .with_no_client_auth()
                .with_single_cert(
                    vec![CertificateDer::from(
                        include_bytes!("../fixtures/server.der").to_vec(),
                    )],
                    PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
                        include_bytes!("../fixtures/public-test-key.der").to_vec(),
                    )),
                )
                .unwrap();
            TlsAcceptor::from(Arc::new(config))
        });
        let (tx, requests) = mpsc::channel(32);
        let stop = CancellationToken::new();
        let token = stop.clone();
        let task = tokio::spawn(async move {
            let mut jobs = JoinSet::new();
            loop {
                tokio::select! {
                    _ = token.cancelled() => break,
                    Some(_) = jobs.join_next(), if !jobs.is_empty() => {},
                    result = listener.accept(), if jobs.len() < 16 => {
                        let (stream,_) = result.unwrap();
                        let tx = tx.clone(); let acceptor = acceptor.clone();
                        jobs.spawn(async move {
                            if let Some(acceptor) = acceptor {
                                if let Ok(Ok(stream)) = timeout(Duration::from_secs(5), acceptor.accept(stream)).await { serve(stream, tx).await; }
                            } else { serve(stream, tx).await; }
                        });
                    }
                }
            }
            jobs.abort_all();
            while jobs.join_next().await.is_some() {}
        });
        Self {
            endpoint,
            requests,
            stop,
            task,
        }
    }
    pub async fn next(&mut self) -> Request {
        self.next_phase(None).await
    }
    pub async fn next_phase(&mut self, phase: Option<&'static str>) -> Request {
        // An SDK bootstrap or committed sync runs between HTTP requests. This
        // is a fixture orchestration bound, not an HTTP/production deadline.
        let result = timeout(
            load_limits().sdk + load_limits().request,
            self.requests.recv(),
        )
        .await;
        if (result.is_err() || matches!(&result, Ok(None)))
            && let Some(phase) = phase
        {
            eprintln!("scripted HTTP phase: {phase}");
        }
        result
            .expect("scripted HTTP request missing after SDK plus HTTP budget")
            .expect("scripted HTTP peer closed")
    }
    pub async fn no_request(&mut self) {
        assert!(
            timeout(Duration::from_millis(80), self.requests.recv())
                .await
                .is_err()
        );
    }
    pub async fn close(self) {
        self.stop.cancel();
        self.task.await.unwrap();
    }
}
async fn serve<S: AsyncRead + AsyncWrite + Unpin>(mut stream: S, tx: mpsc::Sender<Request>) {
    let mut bytes = Vec::new();
    let headers_end = loop {
        if let Some(n) = bytes.windows(4).position(|b| b == b"\r\n\r\n") {
            break n + 4;
        }
        if bytes.len() > 32768 {
            return;
        }
        let mut part = [0; 4096];
        let Ok(Ok(n)) = timeout(Duration::from_secs(2), stream.read(&mut part)).await else {
            return;
        };
        if n == 0 {
            return;
        }
        bytes.extend_from_slice(&part[..n]);
    };
    let text = std::str::from_utf8(&bytes[..headers_end]).unwrap();
    let mut lines = text.split("\r\n");
    let first: Vec<_> = lines.next().unwrap().split(' ').collect();
    let method = first[0].to_owned();
    let target = first[1].to_owned();
    let headers: BTreeMap<_, _> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(k, v)| (k.to_lowercase(), v.trim().to_owned()))
        .collect();
    let length: usize = headers
        .get("content-length")
        .map(|s| s.parse().unwrap())
        .unwrap_or(0);
    if length > 1024 * 1024 {
        return;
    }
    while bytes.len() < headers_end + length {
        let mut part = [0; 4096];
        let Ok(Ok(n)) = timeout(Duration::from_secs(2), stream.read(&mut part)).await else {
            return;
        };
        if n == 0 {
            return;
        }
        bytes.extend_from_slice(&part[..n]);
    }
    let (response, rx) = oneshot::channel();
    let request = Request {
        method,
        target,
        headers,
        body: bytes[headers_end..headers_end + length].to_vec(),
        response,
    };
    if tx.send(request).await.is_err() {
        return;
    }
    if let Ok(ScriptedResponse { pieces, clean }) = rx.await {
        for (delay, bytes) in pieces {
            tokio::time::sleep(delay).await;
            if stream.write_all(&bytes).await.is_err() {
                return;
            }
            if stream.flush().await.is_err() {
                return;
            }
        }
        if clean {
            // TlsStream drop alone does not send close_notify. A bounded clean
            // shutdown lets close-delimited bodies prove actual TLS EOF; the
            // explicit unclean fixture remains a transport failure.
            let _ = timeout(Duration::from_millis(200), stream.shutdown()).await;
        }
    }
}
