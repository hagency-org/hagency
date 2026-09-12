use hagency_runtime::codex::transport::{
    Command, Driver, Error, Limits, MAX_EVENT_BYTES, MAX_EVENTS, STDERR_BYTES,
};
use hagency_runtime::codex::{Event, Phase, RequestId};
use serde_json::{Value, json};
use std::{
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWrite, AsyncWriteExt, DuplexStream};
use tokio::time::{Instant, sleep, timeout};

type TestDriver = Driver<DuplexStream, DuplexStream, DuplexStream>;
struct Peer {
    stdin: DuplexStream,
    stdout: DuplexStream,
    stderr: DuplexStream,
}

fn fixture(capacity: usize, limits: Limits) -> (TestDriver, Peer) {
    let (stdin, peer_in) = tokio::io::duplex(capacity);
    let (stdout, peer_out) = tokio::io::duplex(65536);
    let (stderr, peer_err) = tokio::io::duplex(1024);
    (
        Driver::new(stdout, stdin, stderr, limits).unwrap(),
        Peer {
            stdin: peer_in,
            stdout: peer_out,
            stderr: peer_err,
        },
    )
}
fn limits() -> Limits {
    Limits {
        write_timeout_ms: 100,
        event_wait_ms: 1000,
        lifetime_ms: 30_000,
    }
}
fn initialize() -> Command {
    Command::Initialize {
        client_version: "0.1".into(),
        response_timeout_ms: 1000,
    }
}
fn request(length: usize, timeout: u64) -> Command {
    Command::Request {
        method: "turn/start".into(),
        params: json!({ "input": "x".repeat(length) }),
        response_timeout_ms: timeout,
    }
}
fn line(value: Value) -> Vec<u8> {
    let mut bytes = serde_json::to_vec(&value).unwrap();
    bytes.push(b'\n');
    bytes
}
async fn read_line(reader: &mut DuplexStream) -> Vec<u8> {
    timeout(Duration::from_secs(3), async {
        let mut bytes = Vec::new();
        loop {
            let byte = reader.read_u8().await.unwrap();
            bytes.push(byte);
            assert!(bytes.len() <= 1_048_577);
            if byte == b'\n' {
                return bytes;
            }
        }
    })
    .await
    .unwrap()
}
async fn joined<T>(task: tokio::task::JoinHandle<T>) -> T {
    timeout(Duration::from_secs(3), task)
        .await
        .unwrap()
        .unwrap()
}
async fn ready(driver: &mut TestDriver, peer: &mut Peer) {
    let receipt = driver.send(initialize()).await.unwrap();
    let sent = read_line(&mut peer.stdin).await;
    assert_eq!(receipt.bytes, sent.len());
    assert_eq!(receipt.request_id, Some(RequestId::Number(0)));
    peer.stdout.write_all(&line(json!({ "id": 0, "result": {
        "userAgent": "fixture/0.153.4", "platformFamily": "unix", "platformOs": "macos", "codexHome": "/fixture"
    } }))).await.unwrap();
    assert!(matches!(
        driver.next_event().await,
        Ok(Event::Initialized { .. })
    ));
    assert_eq!(
        driver.send(Command::Initialized).await.unwrap().request_id,
        None
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&read_line(&mut peer.stdin).await).unwrap()["method"],
        "initialized"
    );
    assert_eq!(driver.phase(), Phase::Ready);
}

#[tokio::test(start_paused = true)]
async fn native_codex_transport_write_complete_and_early_rpc_response() {
    let (mut driver, mut peer) = fixture(256, limits());
    ready(&mut driver, &mut peer).await;
    let peer_task = tokio::spawn(async move {
        // An upstream response can arrive while only a prefix of stdin was
        // accepted. It cannot make the transport claim a completed write.
        peer.stdout
            .write_all(&line(
                json!({ "id": 1, "result": { "turn": { "id": "fixture" } } }),
            ))
            .await
            .unwrap();
        sleep(Duration::from_millis(10)).await;
        let bytes = read_line(&mut peer.stdin).await;
        (peer, bytes)
    });
    let receipt = driver.send(request(32768, 1000)).await.unwrap();
    let (mut peer, bytes) = joined(peer_task).await;
    assert_eq!(receipt.bytes, bytes.len());
    assert_eq!(receipt.request_id, Some(RequestId::Number(1)));
    assert_eq!(
        serde_json::from_slice::<Value>(&bytes).unwrap()["params"]["input"]
            .as_str()
            .unwrap()
            .len(),
        32768
    );
    assert_eq!(driver.queued_events(), 1);
    assert!(driver.queued_event_bytes() > 64);
    assert!(matches!(
        driver.next_event().await,
        Ok(Event::Response {
            id: RequestId::Number(1),
            result: Ok(_),
            ..
        })
    ));
    assert_eq!(driver.queued_event_bytes(), 0);
    // Completing a transport write leaves the driver live; it asserts neither
    // a turn outcome nor process/task completion.
    peer.stdout.write_all(&line(json!({ "method": "turn/completed", "params": { "threadId": "fixture", "turn": { "id": "fixture", "status": "interrupted", "items": [] } } }))).await.unwrap();
    assert!(matches!(
        driver.next_event().await,
        Ok(Event::Notification { .. })
    ));
    assert!(driver.termination().is_none());
    driver.close();
    assert_eq!(driver.termination().unwrap().cause, Error::HostClosed);
}

#[tokio::test(start_paused = true)]
async fn native_codex_transport_failures_partial_write_timeout_and_cancelled_future() {
    let (mut driver, mut peer) = fixture(8, limits());
    let result = driver.send(initialize()).await;
    assert_eq!(result.err(), Some(Error::Timeout));
    let termination = driver.termination().unwrap();
    assert_eq!(termination.pending_requests, 1);
    let write = termination.unconfirmed_write.as_ref().unwrap();
    assert_eq!(write.accepted_bytes, 8);
    assert!(write.total_bytes > write.accepted_bytes);
    let mut received = Vec::new();
    peer.stdin.read_to_end(&mut received).await.unwrap();
    assert_eq!(received.len(), 8);
    assert_eq!(driver.send(initialize()).await.err(), Some(Error::Closed));
    assert_eq!(driver.next_event().await.err(), Some(Error::Closed));

    let (mut driver, mut peer) = fixture(8, limits());
    assert!(
        timeout(Duration::from_millis(5), driver.send(initialize()))
            .await
            .is_err()
    );
    let termination = driver.termination().unwrap();
    assert_eq!(termination.cause, Error::CancelledOperation);
    assert_eq!(
        termination
            .unconfirmed_write
            .as_ref()
            .unwrap()
            .accepted_bytes,
        8
    );
    peer.stdin.read_to_end(&mut Vec::new()).await.unwrap();
    assert_eq!(driver.phase(), Phase::Closed);
    assert_eq!(driver.send(initialize()).await.err(), Some(Error::Closed));

    let (mut driver, _peer) = fixture(1024, limits());
    driver.send(initialize()).await.unwrap();
    assert!(
        timeout(Duration::from_millis(5), driver.next_event())
            .await
            .is_err()
    );
    assert_eq!(
        driver.termination().unwrap().cause,
        Error::CancelledOperation
    );
    assert_eq!(driver.termination().unwrap().pending_requests, 1);

    let (mut driver, mut peer) = fixture(1024, limits());
    driver.send(initialize()).await.unwrap();
    peer.stdout
        .write_all(&line(json!({ "id": 999, "result": {} })))
        .await
        .unwrap();
    assert_eq!(
        driver.next_event().await.err(),
        Some(Error::Protocol(hagency_runtime::codex::Error::Identity))
    );
    // Protocol failure clears its own pending map; transport reconciliation
    // retains the unresolved count observed before that failing call.
    assert_eq!(driver.termination().unwrap().pending_requests, 1);
}

// The real duplex accepts bytes; only flush completion is deliberately stalled.
struct BlockedFlush(DuplexStream);
impl AsyncWrite for BlockedFlush {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.0).poll_write(cx, bytes)
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Pending
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.0).poll_shutdown(cx)
    }
}

#[tokio::test(start_paused = true)]
async fn native_codex_transport_failures_flush_broken_stream_and_eof() {
    let (stdin, mut peer_in) = tokio::io::duplex(1024);
    let (stdout, _peer_out) = tokio::io::duplex(1024);
    let (stderr, _peer_err) = tokio::io::duplex(1024);
    let mut driver = Driver::new(stdout, BlockedFlush(stdin), stderr, limits()).unwrap();
    assert_eq!(driver.send(initialize()).await.err(), Some(Error::Timeout));
    let write = driver
        .termination()
        .unwrap()
        .unconfirmed_write
        .as_ref()
        .unwrap();
    assert_eq!(write.accepted_bytes, write.total_bytes);
    let mut received = Vec::new();
    peer_in.read_to_end(&mut received).await.unwrap();
    assert_eq!(received.len(), write.total_bytes);

    let (mut driver, peer) = fixture(1024, limits());
    let Peer {
        stdin,
        stdout: _stdout,
        stderr: _stderr,
    } = peer;
    drop(stdin);
    assert_eq!(driver.send(initialize()).await.err(), Some(Error::Io));
    assert_eq!(driver.termination().unwrap().pending_requests, 1);

    let (mut driver, mut peer) = fixture(1024, limits());
    driver.send(initialize()).await.unwrap();
    peer.stdout
        .write_all(b"{\"id\":0,\"result\":")
        .await
        .unwrap();
    peer.stdout.shutdown().await.unwrap();
    assert_eq!(driver.next_event().await.err(), Some(Error::PeerEof));
    assert_eq!(driver.phase(), Phase::Closed);
    assert_eq!(driver.termination().unwrap().pending_requests, 1);

    let (mut driver, mut peer) = fixture(1024, limits());
    ready(&mut driver, &mut peer).await;
    peer.stdout.shutdown().await.unwrap();
    assert_eq!(driver.next_event().await.err(), Some(Error::PeerEof));
    // Even idle frame-boundary EOF is unresolved transport termination, never
    // a clean dispatch or process outcome.
    assert_eq!(driver.termination().unwrap().cause, Error::PeerEof);
}

#[tokio::test(start_paused = true)]
async fn native_codex_transport_deadlines_silence_partial_and_lifetime() {
    let (mut driver, _peer) = fixture(1024, limits());
    driver
        .send(Command::Initialize {
            client_version: "fixture".into(),
            response_timeout_ms: 20,
        })
        .await
        .unwrap();
    let start = Instant::now();
    assert_eq!(driver.next_event().await.err(), Some(Error::Timeout));
    assert_eq!(start.elapsed(), Duration::from_millis(20));

    let (mut driver, mut peer) = fixture(
        1024,
        Limits {
            event_wait_ms: 20_000,
            ..limits()
        },
    );
    ready(&mut driver, &mut peer).await;
    let producer = tokio::spawn(async move {
        peer.stdout.write_all(b"{").await.unwrap();
        for _ in 0..12 {
            sleep(Duration::from_millis(900)).await;
            if peer.stdout.write_all(b" ").await.is_err() {
                break;
            }
        }
        peer
    });
    let start = Instant::now();
    assert_eq!(driver.next_event().await.err(), Some(Error::Timeout));
    assert_eq!(start.elapsed(), Duration::from_secs(10));
    joined(producer).await;

    let (mut driver, mut peer) = fixture(
        1024,
        Limits {
            lifetime_ms: 25,
            ..limits()
        },
    );
    ready(&mut driver, &mut peer).await;
    sleep(Duration::from_millis(24)).await;
    peer.stdout
        .write_all(&line(json!({ "method": "warning", "params": {} })))
        .await
        .unwrap();
    assert!(driver.next_event().await.is_ok());
    assert_eq!(driver.next_event().await.err(), Some(Error::Timeout));
    assert_eq!(driver.termination().unwrap().cause, Error::Timeout);
}

#[tokio::test(start_paused = true)]
async fn native_codex_transport_deadlines_flood_and_cancellation_fairness() {
    let (mut driver, mut peer) = fixture(1024, limits());
    ready(&mut driver, &mut peer).await;
    driver.send(request(0, 20)).await.unwrap();
    read_line(&mut peer.stdin).await;
    let producer = tokio::spawn(async move {
        let bytes = line(
            json!({ "method": "item/agentMessage/delta", "params": { "delta": "still working" } }),
        );
        loop {
            if peer.stdout.write_all(&bytes).await.is_err() {
                break;
            }
            sleep(Duration::from_millis(1)).await;
        }
    });
    let mut received = 0;
    loop {
        match driver.next_event().await {
            Ok(Event::Notification { .. }) => received += 1,
            Err(Error::Timeout) => break,
            _ => panic!("unexpected event"),
        }
    }
    assert!(received >= 10);
    assert_eq!(driver.termination().unwrap().pending_requests, 1);
    joined(producer).await;

    let (mut driver, mut peer) = fixture(1024, limits());
    ready(&mut driver, &mut peer).await;
    let (cancel, cancelled) = tokio::sync::oneshot::channel();
    let producer = tokio::spawn(async move {
        let bytes = line(json!({ "method": "warning", "params": {} }));
        for _ in 0..100 {
            peer.stdout.write_all(&bytes).await.unwrap();
        }
        let _ = cancel.send(());
        // Keep all endpoints alive until the host drops the transport.
        let mut rest = Vec::new();
        peer.stdin.read_to_end(&mut rest).await.unwrap();
    });
    {
        let consume = async {
            loop {
                driver.next_event().await.ok().unwrap();
            }
        };
        tokio::pin!(consume);
        // Poll the consumer before cancellation so this checks a started IO
        // operation, not an unpolled future that never acquired streams.
        tokio::select! { biased; _ = &mut consume => panic!("consumer ended"), _ = cancelled => () }
    }
    assert_eq!(driver.phase(), Phase::Closed);
    assert_eq!(
        driver.termination().unwrap().cause,
        Error::CancelledOperation
    );
    joined(producer).await;
}

#[tokio::test]
async fn native_codex_transport_deadlines_real_timer() {
    let (mut driver, _peer) = fixture(
        1024,
        Limits {
            event_wait_ms: 10,
            ..limits()
        },
    );
    let start = Instant::now();
    assert_eq!(
        timeout(Duration::from_secs(3), driver.next_event())
            .await
            .unwrap()
            .err(),
        Some(Error::Timeout)
    );
    assert!(start.elapsed() >= Duration::from_millis(10));
    let (mut driver, mut peer) = fixture(1024, limits());
    ready(&mut driver, &mut peer).await;
    driver.send(request(0, 20)).await.unwrap();
    read_line(&mut peer.stdin).await;
    let producer = tokio::spawn(async move {
        let bytes =
            line(json!({ "method": "warning", "params": { "message": "continuous output" } }));
        // No sleep: output remains ready until duplex backpressure applies.
        while peer.stdout.write_all(&bytes).await.is_ok() {}
    });
    let mut seen = 0;
    let result = timeout(Duration::from_secs(3), async {
        loop {
            match driver.next_event().await {
                Ok(Event::Notification { .. }) => seen += 1,
                Err(error) => break error,
                _ => panic!("unexpected event"),
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(result, Error::Timeout);
    assert!(seen > 0);
    assert_eq!(driver.termination().unwrap().pending_requests, 1);
    joined(producer).await;
}

#[tokio::test(start_paused = true)]
async fn native_codex_transport_pressure_stderr_tail_during_blocked_write() {
    let (mut driver, mut peer) = fixture(256, limits());
    ready(&mut driver, &mut peer).await;
    let producer = tokio::spawn(async move {
        let mut expected = Vec::new();
        for index in 0..128u8 {
            let bytes = vec![index; 512];
            expected.extend_from_slice(&bytes);
            peer.stderr.write_all(&bytes).await.unwrap();
        }
        (peer, expected)
    });
    assert_eq!(
        driver.send(request(65536, 1000)).await.err(),
        Some(Error::Timeout)
    );
    let (_peer, expected) = joined(producer).await;
    let snapshot = driver.stderr_snapshot();
    assert_eq!(snapshot.total_bytes, expected.len() as u64);
    assert_eq!(snapshot.tail.len(), STDERR_BYTES);
    assert_eq!(snapshot.tail, expected[expected.len() - STDERR_BYTES..]);
    assert_eq!(driver.termination().unwrap().pending_requests, 1);
}

#[tokio::test(start_paused = true)]
async fn native_codex_transport_pressure_event_count_and_bytes() {
    for (count, payload) in [(MAX_EVENTS + 1, 0), (3, 800_000)] {
        let (mut driver, mut peer) = fixture(256, limits());
        ready(&mut driver, &mut peer).await;
        let producer = tokio::spawn(async move {
            for index in 0..count {
                let bytes = line(
                    json!({ "id": format!("approval-{index}"), "method": "item/commandExecution/requestApproval", "params": { "threadId": "fixture", "command": "x".repeat(payload) } }),
                );
                if peer.stdout.write_all(&bytes).await.is_err() {
                    break;
                }
            }
            peer
        });
        assert_eq!(
            driver.send(request(65536, 1000)).await.err(),
            Some(Error::Capacity)
        );
        assert_eq!(driver.termination().unwrap().pending_server_requests, count);
        assert_eq!(driver.termination().unwrap().pending_requests, 1);
        assert_eq!(driver.queued_events(), 0);
        assert_eq!(driver.queued_event_bytes(), 0);
        assert_eq!(driver.next_event().await.err(), Some(Error::Closed));
        joined(producer).await;
    }
    const {
        assert!(3 * 800_000 > MAX_EVENT_BYTES);
    }
}
