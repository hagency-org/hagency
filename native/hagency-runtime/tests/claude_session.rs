use hagency_runtime::claude::{
    EventKind, Message,
    session::{Error, Limits, Phase, SessionDriver},
};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream};

struct FlushProbe {
    fail: bool,
}
impl tokio::io::AsyncWrite for FlushProbe {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
        bytes: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::task::Poll::Ready(Ok(bytes.len()))
    }
    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        if self.fail {
            std::task::Poll::Ready(Err(std::io::Error::other("private provider sentinel")))
        } else {
            std::task::Poll::Pending
        }
    }
    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
    }
}

type Driver = SessionDriver<DuplexStream, DuplexStream, DuplexStream>;
struct Peer {
    input: BufReader<DuplexStream>,
    output: DuplexStream,
    stderr: DuplexStream,
}
fn pair(capacity: usize, limits: Limits) -> (Driver, Peer) {
    let (stdin, input) = tokio::io::duplex(capacity);
    let (stdout, output) = tokio::io::duplex(capacity);
    let (stderr, error) = tokio::io::duplex(capacity);
    (
        Driver::new(stdout, stdin, stderr, limits).unwrap(),
        Peer {
            input: BufReader::new(input),
            output,
            stderr: error,
        },
    )
}
fn limits() -> Limits {
    Limits {
        write_timeout_ms: 500,
        event_wait_ms: 1000,
        lifetime_ms: 30_000,
    }
}
async fn read(peer: &mut Peer) -> Value {
    let mut line = String::new();
    peer.input.read_line(&mut line).await.unwrap();
    serde_json::from_str(&line).unwrap()
}
fn encoded(value: Value) -> Vec<u8> {
    let mut bytes = serde_json::to_vec(&value).unwrap();
    bytes.push(b'\n');
    bytes
}
async fn initialize(driver: &mut Driver, peer: &mut Peer) {
    let (result, ()) = tokio::join!(driver.initialize(), async {
        let request = read(peer).await;
        assert_eq!(
            request["request"],
            json!({"subtype":"initialize","hooks":null})
        );
        peer.output
            .write_all(&encoded(json!({"type":"control_response","response":{
            "subtype":"success","request_id":request["request_id"],"response":{}}})))
            .await
            .unwrap();
    });
    result.unwrap();
    assert_eq!(driver.phase(), Phase::Ready);
}
async fn prompt(driver: &mut Driver, peer: &mut Peer) {
    let (result, value) = tokio::join!(
        driver.prompt("literal $(not a shell) 中文\nnext"),
        read(peer)
    );
    let receipt = result.unwrap();
    assert_eq!(receipt.accepted_bytes, receipt.total_bytes);
    assert!(receipt.flushed);
    assert_eq!(
        value["message"]["content"],
        "literal $(not a shell) 中文\nnext"
    );
    assert_eq!(value["session_id"], "");
    assert!(value["parent_tool_use_id"].is_null());
}
async fn event(driver: &mut Driver, peer: &mut Peer, value: Value) -> Result<Message, Error> {
    let bytes = encoded(value);
    let (result, write) = tokio::join!(driver.next_message(), peer.output.write_all(&bytes));
    write.unwrap();
    result
}
fn init() -> Value {
    json!({"type":"system","subtype":"init","session_id":"session-one"})
}
fn permission(id: &str) -> Value {
    json!({"type":"control_request","request_id":id,
    "request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"private sentinel"}}})
}
async fn running() -> (Driver, Peer) {
    let (mut driver, mut peer) = pair(4096, limits());
    initialize(&mut driver, &mut peer).await;
    prompt(&mut driver, &mut peer).await;
    event(&mut driver, &mut peer, init()).await.unwrap();
    (driver, peer)
}

#[tokio::test]
async fn native_claude_session_identity_and_result() {
    let (mut driver, mut peer) = running().await;
    assert_eq!(driver.session_id(), Some("session-one"));
    assert!(matches!(
        event(&mut driver, &mut peer, permission("permission-one"))
            .await
            .unwrap(),
        Message::Permission { .. }
    ));
    event(
        &mut driver,
        &mut peer,
        json!({"type":"control_cancel_request","request_id":"permission-one"}),
    )
    .await
    .unwrap();
    let result = event(
        &mut driver,
        &mut peer,
        json!({"type":"result","session_id":"session-one",
        "subtype":"success","is_error":true,"result":"not canonical completion"}),
    )
    .await
    .unwrap();
    assert!(
        matches!(result,Message::Event {kind:EventKind::Result,payload,..} if payload["is_error"]==true)
    );
    assert_eq!(driver.phase(), Phase::ResultObserved);
    assert!(driver.termination().is_none());
    assert!(matches!(
        driver.prompt("cannot reuse").await,
        Err(Error::State)
    ));

    for value in [
        init(),
        json!({"type":"assistant","session_id":"other","message":{}}),
        json!({"type":"control_response","response":{"subtype":"success","request_id":"unsolicited","response":{}}}),
        json!({"type":"control_cancel_request","request_id":"unknown"}),
    ] {
        let (mut driver, mut peer) = running().await;
        assert!(matches!(
            event(&mut driver, &mut peer, value).await,
            Err(Error::Identity)
        ));
        assert_eq!(driver.phase(), Phase::Closed);
    }
    for early in [
        permission("early"),
        json!({"type":"assistant","session_id":"session-one","message":{}}),
    ] {
        let (mut driver, mut peer) = pair(4096, limits());
        initialize(&mut driver, &mut peer).await;
        prompt(&mut driver, &mut peer).await;
        assert!(matches!(
            event(&mut driver, &mut peer, early).await,
            Err(Error::Identity)
        ));
    }
    for cancelled in [false, true] {
        let (mut driver, mut peer) = running().await;
        event(&mut driver, &mut peer, permission("duplicate"))
            .await
            .unwrap();
        if cancelled {
            event(
                &mut driver,
                &mut peer,
                json!({"type":"control_cancel_request","request_id":"duplicate"}),
            )
            .await
            .unwrap();
        }
        assert!(matches!(
            event(&mut driver, &mut peer, permission("duplicate")).await,
            Err(Error::Identity)
        ));
    }
    let (mut driver, mut peer) = running().await;
    for n in 0..16 {
        event(&mut driver, &mut peer, permission(&format!("p-{n}")))
            .await
            .unwrap();
    }
    assert!(matches!(
        event(&mut driver, &mut peer, permission("over-cap")).await,
        Err(Error::Capacity)
    ));
    for refused in [false, true] {
        let (mut driver, mut peer) = pair(4096, limits());
        let (result, ()) = tokio::join!(driver.initialize(), async {
            let request = read(&mut peer).await;
            let response = if refused {
                json!({"subtype":"error","request_id":request["request_id"],"error":"private error"})
            } else {
                json!({"subtype":"success","request_id":"wrong","response":{}})
            };
            peer.output
                .write_all(&encoded(
                    json!({"type":"control_response","response":response}),
                ))
                .await
                .unwrap();
        });
        assert_eq!(
            result,
            Err(if refused {
                Error::Refused
            } else {
                Error::Identity
            })
        );
        assert_eq!(driver.phase(), Phase::Closed);
    }
    let (mut driver, mut peer) = pair(4096, limits());
    let (result, ()) = tokio::join!(driver.initialize(), async {
        let request = read(&mut peer).await;
        let mut bytes = encoded(json!({"type":"control_response","response":{
            "subtype":"success","request_id":request["request_id"],"response":{}}}));
        bytes.extend(encoded(init()));
        peer.output.write_all(&bytes).await.unwrap();
    });
    result.unwrap();
    // Already-observed pre-prompt bytes cannot be relabeled as this prompt's
    // session. This does not claim causal proof for unread peer bytes.
    assert_eq!(
        driver.prompt("must refuse buffered pre-prompt input").await,
        Err(Error::Identity)
    );
}

#[tokio::test(start_paused = true)]
async fn native_claude_session_deadlines_and_cancellation() {
    for fail in [false, true] {
        let (stdout, _output) = tokio::io::duplex(64);
        let (stderr, _error) = tokio::io::duplex(64);
        let mut driver = SessionDriver::new(stdout, FlushProbe { fail }, stderr, limits()).unwrap();
        assert_eq!(
            driver.initialize().await,
            Err(if fail {
                Error::Io("stdin flush")
            } else {
                Error::Timeout
            })
        );
        let progress = driver.termination().unwrap().unconfirmed_write.unwrap();
        assert_eq!(progress.accepted_bytes, progress.total_bytes);
        assert!(!progress.flushed);
        assert!(!format!("{:?}", driver.termination()).contains("private provider sentinel"));
    }
    let (mut driver, _peer) = pair(8, limits());
    assert_eq!(driver.initialize().await, Err(Error::Timeout));
    let progress = driver.termination().unwrap().unconfirmed_write.unwrap();
    assert_eq!(progress.accepted_bytes, 8);
    assert!(progress.total_bytes > 8);
    assert!(!progress.flushed);
    assert_eq!(driver.initialize().await, Err(Error::Closed));

    let (mut driver, _peer) = pair(8, limits());
    drop(driver.initialize());
    assert_eq!(driver.phase(), Phase::New);
    assert!(
        tokio::time::timeout(Duration::from_millis(20), driver.initialize())
            .await
            .is_err()
    );
    assert_eq!(driver.termination().unwrap().cause, Error::Cancelled);
    assert_eq!(
        driver
            .termination()
            .unwrap()
            .unconfirmed_write
            .unwrap()
            .accepted_bytes,
        8
    );
    assert_eq!(driver.initialize().await, Err(Error::Closed));

    let (mut driver, mut peer) = pair(512, limits());
    initialize(&mut driver, &mut peer).await;
    assert!(
        tokio::time::timeout(
            Duration::from_millis(20),
            driver.prompt(&"x".repeat(64 * 1024))
        )
        .await
        .is_err()
    );
    assert_eq!(driver.termination().unwrap().cause, Error::Cancelled);
    let progress = driver.termination().unwrap().unconfirmed_write.unwrap();
    assert!(progress.accepted_bytes > 0 && progress.accepted_bytes < progress.total_bytes);
    assert_eq!(driver.phase(), Phase::Closed);

    let (mut driver, mut peer) = pair(
        4096,
        Limits {
            event_wait_ms: 20_000,
            ..limits()
        },
    );
    let started = tokio::time::Instant::now();
    let (result, ()) = tokio::join!(driver.initialize(), async {
        read(&mut peer).await;
        peer.output.write_all(b"{").await.unwrap();
    });
    assert_eq!(
        result,
        Err(Error::Protocol(hagency_runtime::claude::Error::Timeout))
    );
    assert_eq!(started.elapsed(), Duration::from_secs(10));

    let (mut driver, mut peer) = pair(
        4096,
        Limits {
            event_wait_ms: 20_000,
            lifetime_ms: 100,
            ..limits()
        },
    );
    let (result, ()) = tokio::join!(driver.initialize(), async {
        read(&mut peer).await;
    });
    assert_eq!(result, Err(Error::Timeout));
    assert!(driver.termination().unwrap().unconfirmed_write.is_none());

    let (mut driver, mut peer) = running().await;
    assert!(
        tokio::time::timeout(Duration::from_millis(20), driver.next_message())
            .await
            .is_err()
    );
    assert_eq!(driver.termination().unwrap().cause, Error::Cancelled);
    assert!(peer.output.write_all(b"closed").await.is_err());
    for partial in [false, true] {
        let (mut driver, mut peer) = running().await;
        if partial {
            peer.output.write_all(b"{").await.unwrap();
        }
        drop(peer.output);
        let result = driver.next_message().await;
        let expected = if partial {
            Error::Protocol(hagency_runtime::claude::Error::UnexpectedEof)
        } else {
            Error::PeerEof
        };
        assert!(matches!(result,Err(error) if error==expected));
    }
}

#[tokio::test]
async fn native_claude_session_backpressure_and_capacity() {
    let (mut driver, mut peer) = pair(512, limits());
    initialize(&mut driver, &mut peer).await;
    let text = "x".repeat(64 * 1024);
    let (result, ()) = tokio::join!(driver.prompt(&text), async {
        // Peer cannot drain stdin until BOTH output pipes are drained by host.
        peer.stderr.write_all(&vec![b'e'; 64 * 1024]).await.unwrap();
        peer.output.write_all(&encoded(init())).await.unwrap();
        assert_eq!(read(&mut peer).await["message"]["content"], text);
    });
    assert!(result.unwrap().flushed);
    assert!(matches!(
        driver.next_message().await.unwrap(),
        Message::Event {
            kind: EventKind::System,
            ..
        }
    ));
    let stderr = driver.stderr_snapshot();
    assert_eq!(stderr.total_bytes, 64 * 1024);
    assert_eq!(stderr.tail, vec![b'e'; 16 * 1024]);

    for large in [false, true] {
        let (mut driver, mut peer) = pair(512, limits());
        initialize(&mut driver, &mut peer).await;
        let bytes = if large {
            encoded(
                json!({"type":"assistant","session_id":"session-one","message":{"text":"x".repeat(800*1024)}}),
            )
        } else {
            encoded(init())
        };
        let (result, _) = tokio::join!(driver.prompt(&text), async {
            for _ in 0..if large { 3 } else { 17 } {
                peer.output.write_all(&bytes).await?;
            }
            Ok::<(), std::io::Error>(())
        });
        assert_eq!(result, Err(Error::Capacity));
        let progress = driver.termination().unwrap().unconfirmed_write.unwrap();
        assert!(progress.accepted_bytes < progress.total_bytes);
        assert!(!progress.flushed);
    }
}
