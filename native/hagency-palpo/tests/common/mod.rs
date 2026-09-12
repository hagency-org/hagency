use hagency_core::custody::Lane;
use hagency_palpo::{Adapter, CancellationToken, HostConfig, Limits};
use hagency_store::{
    Repository, Store,
    outbound::{Command, DeliveryView, RegistrationIdentity, Reply, StartedWork},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
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

pub const FLEET: &str = "hf_0123456789abcdef0123456789abcdef";
pub const TOKEN: &str = "synthetic-machine-token-DO-NOT-USE";
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
pub fn limits() -> Limits {
    Limits {
        poll_wait: Duration::ZERO,
        connect: Duration::from_millis(300),
        headers: Duration::from_millis(300),
        request: Duration::from_millis(800),
        body_idle: Duration::from_millis(200),
        retry_min: Duration::from_millis(40),
        retry_max: Duration::from_millis(160),
        idle: Duration::from_millis(20),
        publication_interval: Duration::from_millis(200),
        ..Limits::default()
    }
}
pub fn registration() -> RegistrationIdentity {
    RegistrationIdentity {
        binding: "managed-http".into(),
        registration_generation: 7,
        side_id: "matrix.example.test".into(),
        fleet_id: FLEET.into(),
        registration_fingerprint: "a".repeat(64),
    }
}
pub fn config(endpoint: &str, generation: u64) -> HostConfig {
    HostConfig::new(registration(), endpoint, TOKEN, generation, limits()).unwrap()
}
pub fn store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let store = open(&dir);
    (dir, store)
}
pub fn open(dir: &tempfile::TempDir) -> Store {
    Store::start(Repository::open(&dir.path().join("private")).unwrap(), 32).unwrap()
}
pub fn empty(generation: u64) -> Value {
    json!({"v":2,"generation":generation,"delivery":null})
}
pub fn delivery(id: &str, lane: Lane, generation: u64, token: &str) -> Value {
    let payload = if lane == Lane::Matrix {
        json!({"transactionId":id,"body":{
            "events":[{"event_id":"$synthetic","content":{"score":0.125,"__proto__":{"safe":"opaque"}}}],
            "ephemeral":[{"type":"m.typing","content":{"user_ids":[]}}],
            "device_lists":{"changed":["@fixture:example.test"]},"extension":{"fraction":2.75}
        }})
    } else {
        json!({"requestId":id,"units":0.125,"extension":{"opaque":true}})
    };
    json!({"v":2,"generation":generation,"delivery":{
        "id":id,"lane":lane,"kind":if lane == Lane::Matrix { "transaction" } else { "request" },
        "token":token,"expiresAt":"2099-01-01T00:00:00.000Z","payload":payload
    }})
}
pub async fn view(store: &Store, adapter: &Adapter, lane: Lane, id: &str) -> DeliveryView {
    match store
        .outbound(
            Command::View {
                scope: adapter.scope(),
                lane,
                id: id.into(),
            },
            now(),
        )
        .await
        .unwrap()
    {
        Reply::View(v) => v,
        _ => panic!("view"),
    }
}
pub async fn start(store: &Store, adapter: &Adapter, lane: Lane, id: &str) -> StartedWork {
    let Reply::Claim(Some(ticket)) = store
        .outbound(
            Command::Claim {
                scope: adapter.scope(),
                lane,
                id: id.into(),
                lease_ms: 30000,
            },
            now(),
        )
        .await
        .unwrap()
    else {
        panic!("claim");
    };
    match store.outbound(Command::Start(ticket), now()).await.unwrap() {
        Reply::Started(w) => w,
        _ => panic!("start"),
    }
}
pub async fn complete(store: &Store, work: StartedWork) {
    assert!(matches!(
        store
            .outbound(
                Command::Complete {
                    ticket: work.ticket,
                    result: json!({"hostReceipt":"synthetic"})
                },
                now()
            )
            .await
            .unwrap(),
        Reply::Finished { .. }
    ));
}

pub struct Request {
    pub method: String,
    pub target: String,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
    response: oneshot::Sender<Vec<(Duration, Vec<u8>)>>,
}
impl Request {
    pub fn json(self, status: u16, body: Value) {
        self.raw(response(status, &serde_json::to_vec(&body).unwrap()));
    }
    pub fn raw(self, bytes: Vec<u8>) {
        let _ = self.response.send(vec![(Duration::ZERO, bytes)]);
    }
    pub fn chunks(self, pieces: Vec<(Duration, Vec<u8>)>) {
        let _ = self.response.send(pieces);
    }
    pub fn value(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap()
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
            "{}://{}/api/fleet/v2/{FLEET}",
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
                                // Diagnostic only: name the handshake outcome so a
                                // client-side Timeout can be attributed to a silent
                                // peer drop rather than guessed at.
                                match timeout(Duration::from_secs(1), acceptor.accept(stream)).await {
                                    Ok(Ok(stream)) => serve(stream, tx).await,
                                    Ok(Err(error)) => eprintln!("fixture tls handshake refused: {error:?}"),
                                    Err(_) => eprintln!("fixture tls handshake deadline"),
                                }
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
        timeout(Duration::from_secs(3), self.requests.recv())
            .await
            .unwrap()
            .unwrap()
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
    if let Ok(pieces) = rx.await {
        for (delay, bytes) in pieces {
            tokio::time::sleep(delay).await;
            if stream.write_all(&bytes).await.is_err() {
                return;
            }
            if stream.flush().await.is_err() {
                return;
            }
        }
    }
}
