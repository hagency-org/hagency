//! ADR174 amendment: a GET whose connection never existed is redialled inside
//! the original deadline and GETs reuse connections; every write stays a single
//! attempt on a fresh connection.
use super::*;
use crate::collector::fixtures as common;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpSocket};

/// A loopback port that refuses connections yet stays reserved for this test:
/// bound, never listening until `listen` is called on the returned socket.
fn reserved() -> (TcpSocket, u16) {
    let socket = TcpSocket::new_v4().unwrap();
    socket.bind("127.0.0.1:0".parse().unwrap()).unwrap();
    let port = socket.local_addr().unwrap().port();
    (socket, port)
}
fn client(port: u16, budget: Duration) -> (common::Fixture, Http) {
    let fixture = common::Fixture::new();
    let mut config = fixture.config(&format!("http://127.0.0.1:{port}/"));
    config.limits.request = budget;
    config.limits.headers = config.limits.headers.min(budget);
    config.limits.connect = config.limits.connect.min(budget);
    config.limits.body_idle = config.limits.body_idle.min(budget);
    let http = Http::new(&config).unwrap();
    (fixture, http)
}

#[derive(Default)]
struct Seen {
    accepted: AtomicUsize,
    heads: Mutex<Vec<String>>,
}
/// Keep-alive HTTP/1.1 peer answering `{}`; it closes only when asked to.
async fn serve(listener: TcpListener, seen: Arc<Seen>) {
    loop {
        let Ok((mut stream, _)) = listener.accept().await else {
            return;
        };
        seen.accepted.fetch_add(1, Ordering::SeqCst);
        let seen = seen.clone();
        tokio::spawn(async move {
            let mut buffer = Vec::new();
            loop {
                let head_end = loop {
                    if let Some(at) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
                        break at + 4;
                    }
                    let mut chunk = [0; 4096];
                    match stream.read(&mut chunk).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => buffer.extend_from_slice(&chunk[..n]),
                    }
                };
                let head = String::from_utf8_lossy(&buffer[..head_end]).to_ascii_lowercase();
                let length = head
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length:"))
                    .map_or(0, |value| value.trim().parse::<usize>().unwrap());
                while buffer.len() < head_end + length {
                    let mut chunk = [0; 4096];
                    match stream.read(&mut chunk).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => buffer.extend_from_slice(&chunk[..n]),
                    }
                }
                buffer.drain(..head_end + length);
                let close = head.contains("connection: close");
                seen.heads.lock().unwrap().push(head);
                let _ = stream
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n{}",
                    )
                    .await;
                if close {
                    return;
                }
            }
        });
    }
}
const PATH: [&str; 4] = ["_matrix", "client", "v3", "sync"];

#[tokio::test]
async fn native_matrix_get_connect_retry_reaches_a_late_peer() {
    let (socket, port) = reserved();
    let seen = Arc::new(Seen::default());
    let (_fixture, http) = client(port, Duration::from_secs(3));
    let cancel = CancellationToken::new();
    let request = http.request(&PATH, None, &cancel);
    tokio::pin!(request);
    // A refused dial returns at once. A single-attempt client would already
    // have failed here; this one is waiting to redial.
    assert!(
        tokio::time::timeout(CONNECT_RETRY / 2, &mut request)
            .await
            .is_err()
    );
    let peer = tokio::spawn(serve(socket.listen(16).unwrap(), seen.clone()));
    assert_eq!(request.await.unwrap().status, 200);
    // The request left exactly once, on a later dial.
    assert_eq!(seen.heads.lock().unwrap().len(), 1);
    peer.abort();
}

#[tokio::test]
async fn native_matrix_get_connect_retry_bounds() {
    // Nothing ever listens: four dials, three growing waits, then the
    // original failure word, which still fences as before.
    let (_socket, port) = reserved();
    let (_fixture, http) = client(port, Duration::from_secs(3));
    let started = std::time::Instant::now();
    let result = http.request(&PATH, None, &CancellationToken::new()).await;
    assert!(matches!(result, Err(Error::Transport)));
    assert!(started.elapsed() >= CONNECT_RETRY * 7);
    assert!(started.elapsed() < Duration::from_secs(3));

    // A wait that cannot fit the original deadline is not started.
    let (_fixture, http) = client(port, Duration::from_millis(250));
    let started = std::time::Instant::now();
    let result = http.request(&PATH, None, &CancellationToken::new()).await;
    assert!(matches!(result, Err(Error::Transport)));
    assert!(started.elapsed() < CONNECT_RETRY * 7);

    // The wait is cancellable.
    let (_fixture, http) = client(port, Duration::from_secs(3));
    let cancel = CancellationToken::new();
    let stop = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(40)).await;
        stop.cancel();
    });
    let result = http.request(&PATH, None, &cancel).await;
    assert!(matches!(result, Err(Error::Cancelled)));
}

#[tokio::test]
async fn native_matrix_write_connect_failure_stays_single_attempt() {
    // The same refusal costs a GET its whole schedule (seven waits' worth, as
    // the bounds test pins). Both writes together must not come near it.
    let (_socket, port) = reserved();
    let (_fixture, http) = client(port, Duration::from_secs(3));
    let cancel = CancellationToken::new();
    let started = std::time::Instant::now();
    assert!(matches!(
        http.post(&PATH, "{}".into(), &cancel).await,
        Err(Error::Transport)
    ));
    assert!(matches!(
        http.put(&PATH, "{}".into(), &cancel).await,
        Err(Error::Transport)
    ));
    assert!(started.elapsed() < CONNECT_RETRY * 7);
}

#[tokio::test]
async fn native_matrix_connect_phase_excludes_tls_verification() {
    // Refused: no connection ever existed.
    let (_socket, port) = reserved();
    let (_fixture, http) = client(port, Duration::from_secs(1));
    let refused = http
        .reader
        .get(format!("http://127.0.0.1:{port}/"))
        .send()
        .await
        .unwrap_err();
    assert!(refused.is_connect() && connect_phase(&refused));

    // An endpoint that fails verification is also "connect" to reqwest, but it
    // is refused, never redialled: the fixture CA is deliberately not trusted.
    let fake = common::Fake::start(true).await;
    let fixture = common::Fixture::new();
    let http = Http::new(&fixture.config(&fake.endpoint)).unwrap();
    let untrusted = http
        .reader
        .get(fake.endpoint.clone())
        .send()
        .await
        .unwrap_err();
    assert!(untrusted.is_connect() && !connect_phase(&untrusted));
    let started = std::time::Instant::now();
    assert!(matches!(
        http.request(&PATH, None, &CancellationToken::new()).await,
        Err(Error::Transport)
    ));
    assert!(started.elapsed() < CONNECT_RETRY * 7);
    fake.close().await;
}

#[tokio::test]
async fn native_matrix_get_reuses_connections_and_writes_do_not() {
    let (socket, port) = reserved();
    let seen = Arc::new(Seen::default());
    let peer = tokio::spawn(serve(socket.listen(16).unwrap(), seen.clone()));
    let (_fixture, http) = client(port, Duration::from_secs(3));
    let cancel = CancellationToken::new();
    for _ in 0..3 {
        assert_eq!(
            http.request(&PATH, None, &cancel).await.unwrap().status,
            200
        );
    }
    assert_eq!(seen.accepted.load(Ordering::SeqCst), 1);
    assert_eq!(
        http.put(&PATH, "{}".into(), &cancel).await.unwrap().status,
        200
    );
    assert_eq!(
        http.post(&PATH, "{}".into(), &cancel).await.unwrap().status,
        200
    );
    // Each write dialled its own connection and asked for it to be closed.
    assert_eq!(seen.accepted.load(Ordering::SeqCst), 3);
    let heads = seen.heads.lock().unwrap();
    assert_eq!(heads.len(), 5);
    for head in heads.iter() {
        assert_eq!(
            head.contains("connection: close"),
            !head.starts_with("get "),
            "{head}"
        );
    }
    peer.abort();
}
