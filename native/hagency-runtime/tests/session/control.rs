use super::*;
use hagency_runtime::codex::{
    RequestId,
    session::{ApprovalControlPolicy, ControlUpdate, ObservationKind, PreparedUpdate},
};
use std::{
    future::{pending, poll_fn},
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    task::Poll,
};
use tokio::io::{AsyncRead, ReadBuf};

fn metrics() -> Value {
    json!({"threadId":"thread-one", "turnId":"turn-one", "tokenUsage": {
        "total":{"inputTokens":100, "outputTokens":50, "totalTokens":150},
        "last":{"inputTokens":10, "outputTokens":5, "totalTokens":15}
    }})
}
fn policy() -> ApprovalControlPolicy {
    ApprovalControlPolicy {
        owner_wait_ms: 5_000,
        response_reserve_ms: 500,
    }
}
async fn controlled(stdin_bytes: usize, stdout_bytes: usize) -> (Session, Peer) {
    controlled_with_policy(stdin_bytes, stdout_bytes, policy()).await
}
async fn controlled_with_policy(
    stdin_bytes: usize,
    stdout_bytes: usize,
    policy: ApprovalControlPolicy,
) -> (Session, Peer) {
    let (stdin, peer_in) = tokio::io::duplex(stdin_bytes);
    let (stdout, peer_out) = tokio::io::duplex(stdout_bytes);
    let (stderr, peer_err) = tokio::io::duplex(1024);
    let mut session = Session::new(
        stdout,
        stdin,
        stderr,
        settings(false),
        Limits {
            write_timeout_ms: 500,
            event_wait_ms: 1000,
            lifetime_ms: 30_000,
        },
        1000,
    )
    .unwrap();
    let mut peer = Peer {
        stdin: peer_in,
        stdout: peer_out,
        _stderr: peer_err,
    };
    initialize(&mut session, &mut peer).await;
    open(&mut session, &mut peer, false).await;
    start(&mut session, &mut peer, vec![]).await.unwrap();
    session.enable_approval_control(policy).unwrap();
    (session, peer)
}
async fn observe(
    s: &mut Session,
    p: &mut Peer,
    event: Value,
) -> (Update, hagency_runtime::codex::session::Observation) {
    let mut control = Box::pin(pending::<()>());
    let (result, ()) = tokio::join!(
        s.next_observed_or_control(control.as_mut()),
        write(p, event)
    );
    let ControlUpdate::Update(update, observation) = result.unwrap() else {
        panic!("real update required");
    };
    (update, *observation)
}
async fn callback(
    s: &mut Session,
    p: &mut Peer,
    id: Value,
) -> hagency_runtime::codex::approval::ApprovalRequest {
    let (Update::Approval(request), _) = observe(s, p, approval_event(id)).await else {
        panic!("approval");
    };
    request
}
async fn no_response(p: &mut Peer) {
    poll_fn(|cx| {
        let mut bytes = [0u8; 1];
        let mut buffer = ReadBuf::new(&mut bytes);
        assert!(
            Pin::new(&mut p.stdin)
                .poll_read(cx, &mut buffer)
                .is_pending(),
            "response overtook update"
        );
        Poll::Ready(())
    })
    .await;
}
struct DropMarker(Arc<AtomicBool>);
impl Drop for DropMarker {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn native_codex_control_partial_frame_and_pinned_future() {
    // The one-byte stdout channel proves the original reader consumed the
    // partial prefix before the host control output becomes available.
    let (mut s, mut p) = controlled(4096, 1).await;
    let source = s.observation_source().unwrap();
    let dropped = Arc::new(AtomicBool::new(false));
    let (release, wait) = tokio::sync::oneshot::channel();
    let mut grant_batch = [false];
    let mut admission = Box::pin(async {
        let _retained = DropMarker(dropped.clone());
        wait.await.unwrap();
        grant_batch[0] = true;
        17
    });
    let mut pending_callbacks = Vec::new();
    let events = [
        note("thread/tokenUsage/updated", metrics()),
        approval_event(json!(7)),
    ];
    let mut prepared = None;
    for (index, event) in events.into_iter().enumerate() {
        let (result, ()) = tokio::join!(
            s.next_observed_or_control(admission.as_mut()),
            write(&mut p, event)
        );
        let ControlUpdate::Update(update, observation) = result.unwrap() else {
            panic!("ordered update");
        };
        assert_eq!(observation.sequence(), index as u64 + 1);
        assert!(observation.source() == &source);
        assert!(!dropped.load(Ordering::SeqCst));
        match update {
            Update::Approval(request) => {
                prepared = Some(s.prepare_approval(request.response(true)).unwrap());
                pending_callbacks.push(request.id().clone());
            }
            Update::Progress => assert!(matches!(observation.kind(), ObservationKind::Usage(_))),
            _ => panic!("unexpected update"),
        }
    }
    let mut frame = serde_json::to_vec(&note("thread/tokenUsage/updated", metrics())).unwrap();
    frame.push(b'\n');
    let split = frame.len() / 2;
    let (result, ()) = tokio::join!(s.next_observed_or_control(admission.as_mut()), async {
        p.stdout.write_all(&frame[..split]).await.unwrap();
        release.send(()).unwrap();
    });
    assert!(matches!(result.unwrap(), ControlUpdate::Control(17)));
    drop(admission);
    assert!(grant_batch[0]);
    assert!(dropped.load(Ordering::SeqCst));
    assert_eq!(pending_callbacks, [RequestId::Number(7)]);
    assert_eq!(
        prepared.as_ref().unwrap().request_id(),
        &RequestId::Number(7)
    );
    let (result, ()) = tokio::join!(
        s.send_prepared_approval(prepared.as_mut().unwrap()),
        async {
            p.stdout.write_all(&frame[split..]).await.unwrap();
        }
    );
    let PreparedUpdate::Update(Update::Progress, observation) = result.unwrap() else {
        panic!("retained partial frame");
    };
    assert_eq!(observation.sequence(), 3);
    assert!(observation.source() == &source);
    assert!(matches!(observation.kind(), ObservationKind::Usage(_)));
    assert_eq!(s.phase(), Phase::Running);
    no_response(&mut p).await;
}

#[tokio::test]
async fn native_codex_control_prepared_response_order_and_once() {
    let (mut s, mut p) = controlled(4096, 4096).await;
    let request = callback(&mut s, &mut p, json!(7)).await;
    let mut prepared = s.prepare_approval(request.response(true)).unwrap();
    // These are genuinely ready bytes, not a manufactured host queue.
    for event in [
        note("thread/tokenUsage/updated", metrics()),
        approval_event(json!("7")),
        note(
            "serverRequest/resolved",
            json!({"threadId":"thread-one","requestId":7}),
        ),
    ] {
        write(&mut p, event).await;
    }
    for sequence in 2..=4 {
        let PreparedUpdate::Update(update, observation) =
            s.send_prepared_approval(&mut prepared).await.unwrap()
        else {
            panic!("unsent frame overtook input");
        };
        assert_eq!(observation.sequence(), sequence);
        match sequence {
            2 => assert!(matches!(observation.kind(), ObservationKind::Usage(_))),
            3 => assert!(
                matches!(update, Update::Approval(ref request) if request.id() == &RequestId::String("7".into()))
            ),
            4 => assert!(matches!(
                update,
                Update::ApprovalResolved {
                    id: RequestId::Number(7)
                }
            )),
            _ => unreachable!(),
        }
        no_response(&mut p).await;
    }
    assert!(s.send_prepared_approval(&mut prepared).await.is_err());
    assert_eq!(s.phase(), Phase::Ended);

    let (mut s, mut p) = controlled(4096, 4096).await;
    let request = callback(&mut s, &mut p, json!("7")).await;
    let mut prepared = s.prepare_approval(request.response(false)).unwrap();
    let PreparedUpdate::WriteAccepted(receipt) =
        s.send_prepared_approval(&mut prepared).await.unwrap()
    else {
        panic!("receipt");
    };
    let expected = json!({"id":"7","result":{"decision":"decline"}});
    assert_eq!(receipt.request_id, Some(RequestId::String("7".into())));
    assert_eq!(
        receipt.bytes,
        serde_json::to_vec(&expected).unwrap().len() + 1
    );
    assert_eq!(read(&mut p.stdin).await, expected);
    assert!(s.send_prepared_approval(&mut prepared).await.is_err());

    let (mut s, mut p) = controlled(4096, 4096).await;
    let request = callback(&mut s, &mut p, json!(7)).await;
    let mut foreign = s.prepare_approval(request.response(true)).unwrap();
    let (mut other, mut peer) = controlled(4096, 4096).await;
    callback(&mut other, &mut peer, json!(7)).await;
    assert!(matches!(
        other.send_prepared_approval(&mut foreign).await,
        Err(Error::Scope)
    ));
    assert_eq!(s.phase(), Phase::Running);

    let (mut s, mut p) = controlled(4096, 4096).await;
    for id in 0..16 {
        callback(&mut s, &mut p, json!(id)).await;
    }
    let mut control = Box::pin(pending::<()>());
    let (result, ()) = tokio::join!(
        s.next_observed_or_control(control.as_mut()),
        write(&mut p, approval_event(json!(16)))
    );
    assert!(matches!(result, Err(Error::Capacity)));
}

#[tokio::test(start_paused = true)]
async fn native_codex_control_absolute_deadlines() {
    let (mut s, mut p) = controlled(4096, 4096).await;
    for _ in 0..3 {
        let mut control = Box::pin(async {
            tokio::time::sleep(Duration::from_millis(300)).await;
        });
        assert!(matches!(
            s.next_observed_or_control(control.as_mut()).await.unwrap(),
            ControlUpdate::Control(())
        ));
    }
    let mut control = Box::pin(pending::<()>());
    assert!(matches!(
        s.next_observed_or_control(control.as_mut()).await,
        Err(Error::Transport(TransportError::Timeout))
    ));

    let (mut s, mut p2) = controlled(4096, 4096).await;
    let request = callback(&mut s, &mut p2, json!(1)).await;
    let original = s.approval_deadline(request.id()).unwrap();
    let mut control = Box::pin(async {
        tokio::time::sleep(Duration::from_secs(2)).await;
    });
    assert!(matches!(
        s.next_observed_or_control(control.as_mut()).await.unwrap(),
        ControlUpdate::Control(())
    ));
    callback(&mut s, &mut p2, json!(2)).await;
    assert_eq!(s.approval_deadline(request.id()).unwrap(), original);
    let mut control = Box::pin(pending::<()>());
    assert!(matches!(
        s.next_observed_or_control(control.as_mut()).await,
        Err(Error::Transport(TransportError::Timeout))
    ));
    assert_eq!(tokio::time::Instant::now(), original);

    // Now the actual 10s partial-frame deadline is the earliest bound, well
    // before the configured 15s owner wait and the original 30s lifetime.
    let (mut s, mut p3) = controlled_with_policy(
        4096,
        4096,
        ApprovalControlPolicy {
            owner_wait_ms: 15_000,
            response_reserve_ms: 500,
        },
    )
    .await;
    callback(&mut s, &mut p3, json!(3)).await;
    p3.stdout.write_all(b"{\"method\":").await.unwrap();
    let original = tokio::time::Instant::now();
    let mut control = Box::pin(async {
        tokio::time::sleep(Duration::from_secs(2)).await;
    });
    assert!(matches!(
        s.next_observed_or_control(control.as_mut()).await.unwrap(),
        ControlUpdate::Control(())
    ));
    let mut control = Box::pin(pending::<()>());
    assert!(matches!(
        s.next_observed_or_control(control.as_mut()).await,
        Err(Error::Transport(TransportError::Timeout))
    ));
    assert_eq!(
        tokio::time::Instant::now() - original,
        Duration::from_secs(10)
    );

    // Enabling early cannot promise enough lifetime for a callback arriving
    // near the original lifetime end; callback admission checks it again.
    let (mut s, mut p4) = controlled(4096, 4096).await;
    tokio::time::advance(Duration::from_secs(26)).await;
    let mut control = Box::pin(pending::<()>());
    let (result, ()) = tokio::join!(
        s.next_observed_or_control(control.as_mut()),
        write(&mut p4, approval_event(json!(5)))
    );
    assert!(matches!(
        result,
        Err(Error::Transport(TransportError::Timeout))
    ));

    let (mut s, mut peer) = running().await;
    tokio::time::advance(Duration::from_secs(29)).await;
    assert!(matches!(
        s.enable_approval_control(policy()),
        Err(Error::Settings)
    ));
    // Silence peers remain owned through each original timeout assertion.
    let _ = (&mut p, &mut peer);

    // An update returned before the first byte does not restart write timeout.
    let (mut s, mut p) = controlled(4096, 4096).await;
    let request = callback(&mut s, &mut p, json!(4)).await;
    let mut prepared = s.prepare_approval(request.response(true)).unwrap();
    write(&mut p, note("warning", json!({}))).await;
    assert!(matches!(
        s.send_prepared_approval(&mut prepared).await.unwrap(),
        PreparedUpdate::Update(_, _)
    ));
    tokio::time::advance(Duration::from_millis(501)).await;
    assert!(matches!(
        s.send_prepared_approval(&mut prepared).await,
        Err(Error::Transport(TransportError::Timeout))
    ));
}

#[tokio::test(start_paused = true)]
async fn native_codex_control_absolute_deadlines_response_reserve() {
    // Preparing before owner expiry admits only the fixed response reserve;
    // the host's durable-begin future may complete inside that reserve.
    let (mut s, mut p) = controlled(4096, 4096).await;
    let request = callback(&mut s, &mut p, json!(6)).await;
    let owner = s.approval_deadline(request.id()).unwrap();
    tokio::time::advance(Duration::from_millis(4900)).await;
    let mut prepared = s.prepare_approval(request.response(true)).unwrap();
    let response = prepared.response_deadline();
    assert_eq!(response - owner, Duration::from_millis(500));
    let mut admission = Box::pin(async {
        tokio::time::sleep(Duration::from_millis(200)).await;
        true
    });
    assert!(matches!(
        s.next_observed_or_control(admission.as_mut())
            .await
            .unwrap(),
        ControlUpdate::Control(true)
    ));
    assert!(tokio::time::Instant::now() > owner && tokio::time::Instant::now() < response);
    assert!(matches!(
        s.send_prepared_approval(&mut prepared).await.unwrap(),
        PreparedUpdate::WriteAccepted(_)
    ));
    assert_eq!(
        read(&mut p.stdin).await,
        json!({"id":6,"result":{"decision":"accept"}})
    );
    assert_eq!(prepared.response_deadline(), response);

    // Preparing one callback does not grant response reserve to an unprepared
    // sibling: that sibling's original owner deadline still expires the pump.
    let (mut s, mut p) = controlled(4096, 4096).await;
    let request = callback(&mut s, &mut p, json!(7)).await;
    let sibling = callback(&mut s, &mut p, json!(8)).await;
    let owner = s.approval_deadline(sibling.id()).unwrap();
    tokio::time::advance(Duration::from_millis(4900)).await;
    let _prepared = s.prepare_approval(request.response(true)).unwrap();
    let mut admission = Box::pin(async {
        tokio::time::sleep(Duration::from_millis(200)).await;
        true
    });
    assert!(matches!(
        s.next_observed_or_control(admission.as_mut()).await,
        Err(Error::Transport(TransportError::Timeout))
    ));
    assert_eq!(tokio::time::Instant::now(), owner);
}

#[tokio::test]
async fn native_codex_control_write_cancellation() {
    let (mut s, mut p) = controlled(1, 4096).await;
    let request = callback(&mut s, &mut p, json!(7)).await;
    let mut prepared = s.prepare_approval(request.response(true)).unwrap();
    let mut send = Box::pin(s.send_prepared_approval(&mut prepared));
    timeout(Duration::from_secs(1), async {
        tokio::select! {
            result = send.as_mut() => panic!("write completed before first-byte gate: {}", result.is_ok()),
            byte = p.stdin.read_u8() => assert_eq!(byte.unwrap(), b'{'),
        }
    }).await.unwrap();
    drop(send);
    assert_eq!(s.phase(), Phase::Ended);
    let termination = s.transport_termination().unwrap();
    assert_eq!(termination.cause, TransportError::CancelledOperation);
    let write = termination.unconfirmed_write.as_ref().unwrap();
    assert!(write.accepted_bytes >= 1 && write.accepted_bytes < write.total_bytes);
    assert_eq!(write.request_id, Some(RequestId::Number(7)));
    assert!(s.send_prepared_approval(&mut prepared).await.is_err());
}
