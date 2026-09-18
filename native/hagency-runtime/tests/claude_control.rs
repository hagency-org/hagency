use hagency_runtime::claude::{
    Message,
    session::{
        ApprovalControlPolicy, ControlUpdate, Error, Limits, PermissionDecision, Phase,
        PreparedUpdate, SessionDriver,
    },
};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, DuplexStream};

type Driver = SessionDriver<DuplexStream, DuplexStream, DuplexStream>;
struct Peer {
    input: BufReader<DuplexStream>,
    output: DuplexStream,
    _stderr: DuplexStream,
}
fn limits() -> Limits {
    Limits {
        write_timeout_ms: 500,
        event_wait_ms: 2000,
        lifetime_ms: 30_000,
    }
}
fn policy() -> ApprovalControlPolicy {
    ApprovalControlPolicy {
        owner_wait_ms: 1000,
        response_reserve_ms: 1000,
    }
}
fn frame(value: Value) -> Vec<u8> {
    let mut bytes = serde_json::to_vec(&value).unwrap();
    bytes.push(b'\n');
    bytes
}
async fn read(peer: &mut Peer) -> Value {
    let mut line = String::new();
    assert!(peer.input.read_line(&mut line).await.unwrap() > 0);
    serde_json::from_str(&line).unwrap()
}
fn request(id: &str, input: Value) -> Value {
    json!({"type":"control_request","request_id":id,
    "request":{"subtype":"can_use_tool","tool_name":"Bash","tool_use_id":"tool-one","input":input}})
}
fn notice() -> Value {
    json!({"type":"system","subtype":"status","session_id":"session-one"})
}
fn cancel(id: &str) -> Value {
    json!({"type":"control_cancel_request","request_id":id})
}
fn result() -> Value {
    json!({"type":"result","subtype":"success","session_id":"session-one","is_error":false,"result":"not Done"})
}
async fn event(driver: &mut Driver, peer: &mut Peer, value: Value) -> Result<Message, Error> {
    let bytes = frame(value);
    let (received, written) = tokio::join!(driver.next_message(), peer.output.write_all(&bytes));
    written.unwrap();
    received
}
async fn running(capacity: usize, enabled: bool) -> (Driver, Peer) {
    let (stdin, input) = tokio::io::duplex(capacity);
    let (stdout, output) = tokio::io::duplex(capacity);
    let (stderr, error) = tokio::io::duplex(capacity);
    let mut driver = Driver::new(stdout, stdin, stderr, limits()).unwrap();
    let mut peer = Peer {
        input: BufReader::new(input),
        output,
        _stderr: error,
    };
    let (initialized, ()) = tokio::join!(driver.initialize(), async {
        let value = read(&mut peer).await;
        peer.output
            .write_all(&frame(json!({"type":"control_response","response":{
            "subtype":"success","request_id":value["request_id"],"response":{}}})))
            .await
            .unwrap();
    });
    initialized.unwrap();
    let (prompted, _) = tokio::join!(driver.prompt("offline"), read(&mut peer));
    prompted.unwrap();
    event(
        &mut driver,
        &mut peer,
        json!({"type":"system","subtype":"init","session_id":"session-one"}),
    )
    .await
    .unwrap();
    if enabled {
        driver.enable_approval_control(policy()).unwrap();
    }
    (driver, peer)
}
async fn callback(driver: &mut Driver, peer: &mut Peer, id: &str, input: Value) {
    assert!(matches!(
        event(driver, peer, request(id, input)).await.unwrap(),
        Message::Permission { .. }
    ));
}

#[tokio::test]
async fn native_claude_permission_exact_response() {
    for decision in [PermissionDecision::Allow, PermissionDecision::Deny] {
        let (mut driver, mut peer) = running(4096, true).await;
        let original = json!({"command":"printf '%s' 'literal 中文'","nested":{"x":[1,null,true]}});
        let mut observed = event(
            &mut driver,
            &mut peer,
            request("request-one", original.clone()),
        )
        .await
        .unwrap();
        if let Message::Permission { input, .. } = &mut observed {
            *input = json!({"command":"substituted input"});
        }
        let mut prepared = driver.prepare_approval("request-one", decision).unwrap();
        assert_eq!(prepared.request_id(), "request-one");
        assert!(prepared.response_deadline() > driver.approval_deadline("request-one").unwrap());
        let (sent, response) = tokio::join!(
            driver.send_prepared_approval(&mut prepared),
            read(&mut peer)
        );
        assert!(
            matches!(sent.unwrap(),PreparedUpdate::WriteAccepted(progress) if progress.flushed && progress.accepted_bytes==progress.total_bytes)
        );
        let expected = match decision {
            PermissionDecision::Allow => json!({"behavior":"allow","updatedInput":original}),
            PermissionDecision::Deny => {
                json!({"behavior":"deny","message":"Permission denied by Hagency.","interrupt":true})
            }
        };
        assert_eq!(
            response,
            json!({"type":"control_response","response":{
            "subtype":"success","request_id":"request-one","response":expected}})
        );
        // A flushed response still leaves the session running; no application
        // acknowledgement, task completion or cleanup was invented.
        assert_eq!(driver.phase(), Phase::Running);
        assert!(driver.termination().is_none());
        assert!(matches!(
            driver.send_prepared_approval(&mut prepared).await,
            Err(Error::PermissionUnavailable)
        ));
    }
    let (mut first, mut peer) = running(4096, true).await;
    callback(&mut first, &mut peer, "same-id", json!({"command":"first"})).await;
    let mut prepared = first
        .prepare_approval("same-id", PermissionDecision::Allow)
        .unwrap();
    let (mut foreign, mut other) = running(4096, true).await;
    callback(
        &mut foreign,
        &mut other,
        "same-id",
        json!({"command":"other"}),
    )
    .await;
    assert!(matches!(
        foreign.send_prepared_approval(&mut prepared).await,
        Err(Error::Identity)
    ));
    let (sent, response) =
        tokio::join!(first.send_prepared_approval(&mut prepared), read(&mut peer));
    assert!(matches!(sent.unwrap(), PreparedUpdate::WriteAccepted(_)));
    assert_eq!(
        response["response"]["response"]["updatedInput"],
        json!({"command":"first"})
    );

    let (mut driver, mut peer) = running(4096, true).await;
    callback(&mut driver, &mut peer, "once", json!({})).await;
    let _original = driver
        .prepare_approval("once", PermissionDecision::Deny)
        .unwrap();
    assert!(matches!(
        driver.prepare_approval("once", PermissionDecision::Allow),
        Err(Error::PermissionUnavailable)
    ));
}

#[tokio::test(start_paused = true)]
async fn native_claude_permission_control_deadlines() {
    let (mut driver, mut peer) = running(4096, true).await;
    callback(&mut driver, &mut peer, "pending", json!({})).await;
    let owner = driver.approval_deadline("pending").unwrap();
    let (send, receive) = tokio::sync::oneshot::channel::<usize>();
    tokio::pin!(receive);
    peer.output.write_all(&frame(notice())).await.unwrap();
    assert!(matches!(
        driver.next_or_control(receive.as_mut()).await.unwrap(),
        ControlUpdate::Message(_)
    ));
    send.send(7).unwrap();
    assert!(matches!(
        driver.next_or_control(receive.as_mut()).await.unwrap(),
        ControlUpdate::Control(Ok(7))
    ));
    assert_eq!(driver.approval_deadline("pending").unwrap(), owner);
    tokio::time::advance(Duration::from_millis(1000)).await;
    assert!(matches!(
        driver.prepare_approval("pending", PermissionDecision::Allow),
        Err(Error::Timeout)
    ));

    let (mut driver, _peer) = running(4096, true).await;
    for _ in 0..4 {
        let ready = std::future::ready(9);
        tokio::pin!(ready);
        assert!(matches!(
            driver.next_or_control(ready.as_mut()).await.unwrap(),
            ControlUpdate::Control(9)
        ));
        tokio::time::advance(Duration::from_millis(500)).await;
    }
    let ready = std::future::ready(9);
    tokio::pin!(ready);
    assert!(matches!(
        driver.next_or_control(ready.as_mut()).await,
        Err(Error::Timeout)
    ));

    let (mut driver, mut peer) = running(4096, true).await;
    // Both messages are returned by one actual pipe read. Consuming the second
    // later must not replace its original receive timestamp with "now".
    let mut bytes = frame(notice());
    bytes.extend(frame(request("delayed", json!({}))));
    peer.output.write_all(&bytes).await.unwrap();
    driver.next_message().await.unwrap();
    tokio::time::advance(Duration::from_millis(1001)).await;
    assert!(matches!(driver.next_message().await, Err(Error::Timeout)));

    let (mut driver, mut peer) = running(4096, true).await;
    callback(&mut driver, &mut peer, "fixed-write", json!({})).await;
    let mut prepared = driver
        .prepare_approval("fixed-write", PermissionDecision::Allow)
        .unwrap();
    peer.output.write_all(&frame(notice())).await.unwrap();
    assert!(matches!(
        driver.send_prepared_approval(&mut prepared).await.unwrap(),
        PreparedUpdate::Message(_)
    ));
    tokio::time::advance(Duration::from_millis(501)).await;
    assert!(matches!(
        driver.send_prepared_approval(&mut prepared).await,
        Err(Error::Timeout)
    ));
    assert_eq!(
        driver
            .termination()
            .unwrap()
            .unconfirmed_write
            .unwrap()
            .accepted_bytes,
        0
    );
}

#[tokio::test(start_paused = true)]
async fn native_claude_permission_write_barriers() {
    for terminal in [false, true] {
        let (mut driver, mut peer) = running(4096, true).await;
        callback(&mut driver, &mut peer, "barrier", json!({})).await;
        let mut prepared = driver
            .prepare_approval("barrier", PermissionDecision::Allow)
            .unwrap();
        let bytes = frame(if terminal {
            result()
        } else {
            cancel("barrier")
        });
        peer.output
            .write_all(&bytes[..bytes.len() - 1])
            .await
            .unwrap();
        let (sent, ()) = tokio::join!(driver.send_prepared_approval(&mut prepared), async {
            tokio::time::sleep(Duration::from_millis(25)).await;
            // A readable partial stdout frame must fence the first response byte.
            let mut byte = [0];
            assert!(
                tokio::time::timeout(Duration::from_millis(1), peer.input.read(&mut byte))
                    .await
                    .is_err()
            );
            peer.output.write_all(b"\n").await.unwrap();
        });
        assert!(matches!(sent.unwrap(), PreparedUpdate::Message(_)));
        assert_eq!(driver.write_progress().unwrap().accepted_bytes, 0);
        assert!(matches!(
            driver.send_prepared_approval(&mut prepared).await,
            Err(Error::State | Error::PermissionUnavailable)
        ));
        assert_eq!(
            driver
                .termination()
                .unwrap()
                .unconfirmed_write
                .unwrap()
                .accepted_bytes,
            0
        );
    }
    let (mut driver, mut peer) = running(128, true).await;
    callback(
        &mut driver,
        &mut peer,
        "partial",
        json!({"command":"x".repeat(4000)}),
    )
    .await;
    let mut prepared = driver
        .prepare_approval("partial", PermissionDecision::Allow)
        .unwrap();
    let (sent, ()) = tokio::join!(driver.send_prepared_approval(&mut prepared), async {
        // Observe a prefix before sending cancellation; leave the rest blocked.
        let mut prefix = [0; 8];
        peer.input.read_exact(&mut prefix).await.unwrap();
        peer.output
            .write_all(&frame(cancel("partial")))
            .await
            .unwrap();
    });
    assert!(matches!(
        sent.unwrap(),
        PreparedUpdate::Message(Message::ControlCancel { .. })
    ));
    let before = driver.write_progress().unwrap();
    assert!(before.accepted_bytes > 0 && before.accepted_bytes < before.total_bytes);
    let ready = std::future::ready(());
    tokio::pin!(ready);
    assert!(matches!(
        driver.next_or_control(ready.as_mut()).await.unwrap(),
        ControlUpdate::Control(())
    ));
    assert_eq!(driver.write_progress().unwrap(), before);
    assert!(matches!(
        driver.send_prepared_approval(&mut prepared).await,
        Err(Error::PermissionUnavailable)
    ));
    assert_eq!(
        driver.termination().unwrap().unconfirmed_write.unwrap(),
        before
    );
}

#[tokio::test(start_paused = true)]
async fn native_claude_permission_bounds_and_cancel() {
    let (mut driver, mut peer) = running(4096, false).await;
    callback(&mut driver, &mut peer, "disabled", json!({})).await;
    assert!(matches!(
        driver.prepare_approval("disabled", PermissionDecision::Allow),
        Err(Error::State)
    ));
    for policy in [
        ApprovalControlPolicy {
            owner_wait_ms: 0,
            ..policy()
        },
        ApprovalControlPolicy {
            response_reserve_ms: 499,
            ..policy()
        },
        ApprovalControlPolicy {
            owner_wait_ms: 30_000,
            ..policy()
        },
    ] {
        let (mut driver, _) = running(4096, false).await;
        assert!(driver.enable_approval_control(policy).is_err());
        assert_eq!(driver.phase(), Phase::Closed);
    }
    let (mut driver, mut peer) = running(4096, true).await;
    assert!(matches!(
        event(
            &mut driver,
            &mut peer,
            request("oversize", json!({"x":"x".repeat(64*1024)}))
        )
        .await,
        Err(Error::Capacity)
    ));
    let (mut driver, mut peer) = running(4096, true).await;
    for n in 0..16 {
        callback(&mut driver, &mut peer, &format!("p-{n}"), json!({})).await;
    }
    assert!(matches!(
        event(&mut driver, &mut peer, request("over-count", json!({}))).await,
        Err(Error::Capacity)
    ));

    let (mut driver, _peer) = running(4096, true).await;
    let pending = std::future::pending::<()>();
    tokio::pin!(pending);
    assert!(
        tokio::time::timeout(
            Duration::from_millis(10),
            driver.next_or_control(pending.as_mut())
        )
        .await
        .is_err()
    );
    assert_eq!(driver.termination().unwrap().cause, Error::Cancelled);

    let (mut driver, mut peer) = running(128, true).await;
    callback(
        &mut driver,
        &mut peer,
        "cancel-send",
        json!({"x":"x".repeat(4000)}),
    )
    .await;
    let mut prepared = driver
        .prepare_approval("cancel-send", PermissionDecision::Allow)
        .unwrap();
    assert!(
        tokio::time::timeout(
            Duration::from_millis(10),
            driver.send_prepared_approval(&mut prepared)
        )
        .await
        .is_err()
    );
    assert_eq!(driver.termination().unwrap().cause, Error::Cancelled);
    let progress = driver.termination().unwrap().unconfirmed_write.unwrap();
    assert!(progress.accepted_bytes > 0 && progress.accepted_bytes < progress.total_bytes);
    assert!(matches!(
        driver.send_prepared_approval(&mut prepared).await,
        Err(Error::Closed)
    ));
}
