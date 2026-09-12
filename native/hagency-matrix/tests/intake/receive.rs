use super::*;
use crate::{ReceiveError, ReceivedAttachment};
use base64::{Engine, engine::general_purpose::STANDARD};
use hagency_core::{ingress::VerifiedTaskRequest, task_intents::TaskDefinition, tasks::*};
use sha2::{Digest, Sha256};
use std::{future::Future, pin::Pin, sync::Arc, time::Duration};

fn vector() -> (Value, Vec<u8>, Vec<u8>) {
    let vectors: Value =
        serde_json::from_str(include_str!("../../../fixtures/media.json")).unwrap();
    let row = vectors["vectors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["length"] == 257)
        .unwrap();
    (
        row["descriptor"].clone(),
        STANDARD
            .decode(row["ciphertext"].as_str().unwrap())
            .unwrap(),
        (0..257).map(|i| ((i * 31 + 17) % 256) as u8).collect(),
    )
}
fn file(id: &str, image: bool) -> Value {
    let mut descriptor = vector().0;
    descriptor["url"] = json!("mxc://media.remote/receive_file");
    json!({"event_id":format!("${id}"),"content":{
        "msgtype":if image {"m.image"} else {"m.file"}, "body":"报告.txt",
        "filename":"报告.txt", "info":{"mimetype":"text/plain","size":999},
        "file":descriptor, "m.mentions":{"user_ids":["@worker:example.test"]}
    }})
}
async fn packet(c: &Collector, values: Vec<Value>, token: &str) -> Value {
    let mut value = c
        .inner
        .owner
        .lock()
        .await
        .as_ref()
        .unwrap()
        .attachment_fixture(values, true)
        .await;
    value["next_batch"] = json!(token);
    value
}
async fn ready(direct: bool, values: Vec<Value>) -> (common::Fixture, common::Fake, Collector) {
    let f = common::Fixture::new();
    let mut fake = common::Fake::start(true).await;
    let mut config = config(&f, &fake.endpoint, f.identity.clone(), 1, direct);
    // This fixture deliberately pauses GET while running actual writer work.
    // Independent 100ms total-deadline tests below do not widen production limits.
    config.limits.request = Duration::from_secs(3);
    config.limits.headers = Duration::from_secs(2);
    let c = Collector::new(config, f.store.clone()).unwrap();
    prime(&c, &f, &mut fake, true).await;
    let value = packet(&c, values, "receive_batch").await;
    assert!(run(&c, &mut fake, value, true).await.unwrap().admitted > 0);
    fake.no_request().await;
    (f, fake, c)
}
async fn start(
    f: &common::Fixture,
    id: &str,
    session: &str,
    task: &str,
    selected: Vec<u64>,
    lease: u64,
) -> RunnerCapability {
    f.store
        .enqueue_inbox_dispatch(
            DispatchInput {
                id: id.into(),
                session_id: session.into(),
                task_id: Some(task.into()),
                resources: vec![],
                payload: json!({"instruction":"Read visible file"}),
            },
            selected,
        )
        .await
        .unwrap();
    let cap = f
        .store
        .claim_dispatch("receive_runner".into(), now(), lease, 120_000, 1)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(cap.dispatch_id, id);
    f.store.start_dispatch(cap.clone(), now()).await.unwrap();
    cap
}
async fn cap(f: &common::Fixture, lease: u64) -> RunnerCapability {
    f.store
        .create_canonical_task(
            "receive_task".into(),
            "root".into(),
            "Read file".into(),
            now(),
        )
        .await
        .unwrap();
    let first = f.store.inbox("root".into(), 0, 100, None).await.unwrap()[0]
        .message
        .sequence;
    start(f, "receive_run", "root", "receive_task", vec![first], lease).await
}
fn response(bytes: &[u8]) -> Vec<u8> {
    let mut wire = format!("HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", bytes.len()).into_bytes();
    wire.extend_from_slice(bytes);
    wire
}
async fn next<F>(run: Pin<&mut F>, fake: &mut common::Fake) -> common::Request
where
    F: Future<Output = Result<ReceivedAttachment, ReceiveError>>,
{
    tokio::select! {
        biased;
        request = fake.next() => {
            assert_eq!(request.method,"GET");
            assert_eq!(request.target,"/_matrix/client/v1/media/download/media.remote/receive_file");
            assert_eq!(request.headers["authorization"],format!("Bearer {}",common::TOKEN));
            assert!(request.body.is_empty());
            for secret in ["key_ops", "A256CTR", "报告", "mxc:"] {
                assert!(!request.target.contains(secret));
                assert!(!format!("{:?}",request.headers).contains(secret));
            }
            request
        },
        result = run => panic!("receive completed before media GET: {:?}",result.err()),
    }
}
async fn receive(
    c: &Collector,
    f: &mut common::Fake,
    cap: &RunnerCapability,
    id: &str,
) -> ReceivedAttachment {
    let cancel = CancellationToken::new();
    let run = c.receive_attachment(cap.clone(), id.into(), &cancel);
    tokio::pin!(run);
    next(run.as_mut(), f).await.raw(response(&vector().1));
    run.await.unwrap()
}
async fn close(c: Collector, f: common::Fixture, fake: common::Fake) {
    c.close().await.unwrap();
    finish(f, fake).await;
}
async fn finish(f: common::Fixture, fake: common::Fake) {
    let (result, snapshot) = f.store.shutdown_observed().await;
    assert!(
        result.is_ok(),
        "receive fixture shutdown: {result:?}; {snapshot:?}"
    );
    fake.close().await;
}

#[tokio::test]
async fn native_matrix_receive_verified() {
    for direct in [true, false] {
        let (f, mut fake, c) = ready(direct, vec![file("file", !direct)]).await;
        let sequence = f.store.inbox("root".into(), 0, 100, None).await.unwrap()[0]
            .message
            .sequence;
        let task = f
            .store
            .create_verified_task_intent(VerifiedTaskRequest {
                scope: f.store.matrix_ingress_scope("root".into()).await.unwrap(),
                request_key: "file_task".into(),
                source_sequence: sequence,
                definition: TaskDefinition {
                    title: "Read verified file".into(),
                    ..Default::default()
                },
            })
            .await
            .unwrap();
        assert_eq!(task.session_id == "root", direct);
        // Host-only observed notice is a domain fixture. This test sends no notice
        // or model request; actual receive evidence below is real encrypted SDK/TLS.
        let n = f
            .store
            .claim_verified_task_notice(10_000)
            .await
            .unwrap()
            .unwrap();
        f.store
            .begin_verified_task_notice_send(n.claim.notice.id.clone(), n.claim.token.clone())
            .await
            .unwrap();
        f.store
            .deliver_verified_task_notice(
                n.claim.notice.id.clone(),
                n.claim.token,
                ReplyDeliveryObservation {
                    transaction_id: n.claim.notice.transaction_id,
                    digest: n.digest,
                    server_name: n.route.server_name,
                    room_id: n.route.room_id,
                    sender_mxid: n.route.sender_mxid,
                    device_id: n.route.device_id,
                    event_id: "$receive_ack".into(),
                    encrypted: true,
                },
            )
            .await
            .unwrap();
        let first = start(
            &f,
            "first",
            &task.session_id,
            &task.task_id,
            vec![sequence],
            60_000,
        )
        .await;
        let got = receive(&c, &mut fake, &first, "$file").await;
        assert_eq!(got.bytes(), vector().2);
        assert_eq!(
            got.digest().as_slice(),
            Sha256::digest(got.bytes()).as_slice()
        );
        assert_eq!(got.metadata().filename, "报告.txt");
        assert_eq!(got.metadata().declared_size, Some(999)); // actual length is257
        drop(got);
        f.store
            .complete_dispatch(first.clone(), json!({"fixture":"turn ended"}), now())
            .await
            .unwrap();
        assert!(
            c.receive_attachment(first, "$file".into(), &CancellationToken::new())
                .await
                .is_err()
        );
        // A real encrypted follow-up belongs to the existing main/thread lineage.
        let mut follow = json!({"event_id":"$follow","content":{"msgtype":"m.text","body":"Read the same file again","m.mentions":{"user_ids":["@worker:example.test"]}}});
        if !direct {
            follow["content"]["m.relates_to"] = json!({"rel_type":"m.thread","event_id":"$file"});
        }
        let value = packet(&c, vec![follow], "follow_batch").await;
        let cancel = CancellationToken::new();
        let (out, ()) = tokio::join!(
            c.intake(
                HostIntakePlan::new(vec![task.session_id.clone()]).unwrap(),
                &cancel
            ),
            async {
                fake.next().await.json(200, common::who());
                fake.next().await.json(200, value);
                fake.next().await.json(200, state(true));
            }
        );
        assert_eq!(out.unwrap().admitted, 1);
        let inputs = f
            .store
            .inbox(task.session_id.clone(), sequence, 100, None)
            .await
            .unwrap();
        let follow = inputs
            .iter()
            .find(|i| i.message.event_id == "$follow")
            .unwrap()
            .message
            .sequence;
        let second = start(
            &f,
            "second",
            &task.session_id,
            &task.task_id,
            vec![follow],
            60_000,
        )
        .await;
        let exact = receive(&c, &mut fake, &second, "$file").await;
        assert_eq!(exact.bytes(), vector().2);
        drop(exact);
        let public = serde_json::to_string(&inputs).unwrap();
        for hidden in ["mxc://", "key_ops", "A256CTR", "ciphertext"] {
            assert!(!public.contains(hidden));
        }
        close(c, f, fake).await;
    }
}

#[tokio::test]
async fn native_matrix_receive_visibility() {
    let (f, mut fake, c) = ready(
        true,
        vec![file("selected", false), file("unrelated", false)],
    )
    .await;
    let cap = cap(&f, 60_000).await;
    let cancel = CancellationToken::new();
    for id in ["$unrelated", "$absent"] {
        assert!(
            c.receive_attachment(cap.clone(), id.into(), &cancel)
                .await
                .is_err()
        );
    }
    for field in ["secret", "fence", "dispatch"] {
        let mut wrong = cap.clone();
        match field {
            "secret" => wrong.secret = "wrong".into(),
            "fence" => wrong.fence += 1,
            _ => wrong.dispatch_id = "other".into(),
        }
        assert!(
            c.receive_attachment(wrong, "$selected".into(), &cancel)
                .await
                .is_err()
        );
    }
    fake.no_request().await;
    let got = receive(&c, &mut fake, &cap, "$selected").await;
    drop(got);
    close(c, f, fake).await;
}

#[tokio::test]
async fn native_matrix_receive_retirement() {
    for case in ["revoke", "promotion", "negative", "expiry"] {
        let (f, mut fake, c) = ready(true, vec![file("private", false)]).await;
        let cap = cap(&f, 60_000).await;
        let cancel = CancellationToken::new();
        let mut run = Box::pin(c.receive_attachment(cap.clone(), "$private".into(), &cancel));
        let request = next(run.as_mut(), &mut fake).await;
        // Real GET has begun and both Owner and busy must be available now.
        assert!(c.inner.owner.try_lock().is_ok());
        assert!(c.inner.busy.try_acquire().is_ok());
        match case {
            "revoke" => {
                f.store
                    .revoke(
                        "revoke_receive".into(),
                        f.identity.transport.engagement_id.clone(),
                    )
                    .await
                    .unwrap();
            }
            "promotion" => {
                f.store
                    .observe_matrix_room(MatrixRoomObservation {
                        engagement_id: f.identity.transport.engagement_id.clone(),
                        registration_generation: 1,
                        transport_generation: 1,
                        room_id: "!project:example.test".into(),
                        generation: 2,
                        privacy: RoomPrivacy::Group {},
                        invite_only: true,
                        encrypted: true,
                        joined: [
                            "@owner:example.test".into(),
                            "@worker:example.test".into(),
                            "@third:example.test".into(),
                        ]
                        .into(),
                    })
                    .await
                    .unwrap();
            }
            "negative" => {
                // Actual concurrent collector sees encryption disappear. It must
                // commit negative evidence while receive still waits on TLS.
                let observe = c.collect(&cancel);
                let (result, ()) = tokio::join!(observe, async {
                    fake.next().await.json(200, common::who());
                    fake.next().await.json(200, state(false));
                });
                assert!(result.is_err());
            }
            _ => {
                f.store
                    .renew_dispatch(cap.clone(), now(), 50)
                    .await
                    .unwrap();
                tokio::time::sleep(Duration::from_millis(80)).await;
            }
        }
        request.raw(response(&vector().1));
        assert!(matches!(
            run.await,
            Err(ReceiveError::Authority(Error::Domain | Error::Generation))
        ));
        assert!(
            c.receive_attachment(cap, "$private".into(), &cancel)
                .await
                .is_err()
        );
        fake.no_request().await;
        if case == "revoke" {
            // Revoked engagement no longer permits Collector::close's transport
            // mutation. Close the actual retained SDK first, and preserve that
            // existing domain refusal rather than treating it as successful close.
            c.inner
                .owner
                .lock()
                .await
                .take()
                .unwrap()
                .close()
                .await
                .unwrap();
            assert_eq!(c.close().await, Err(Error::Domain));
            finish(f, fake).await;
        } else {
            close(c, f, fake).await;
        }
    }
}

#[tokio::test]
async fn native_matrix_receive_failure() {
    let (f, mut fake, c) = ready(true, vec![file("file", false)]).await;
    let cap = cap(&f, 60_000).await;
    for case in ["hash", "truncated", "cancel", "drop"] {
        let cancel = CancellationToken::new();
        let mut run = Box::pin(c.receive_attachment(cap.clone(), "$file".into(), &cancel));
        let request = next(run.as_mut(), &mut fake).await;
        match case {
            "hash" => {
                let mut bad = vector().1;
                bad[0] ^= 1;
                request.raw(response(&bad));
            }
            "truncated" => {
                let mut bad = response(&vector().1);
                bad.pop();
                request.raw(bad);
            }
            "cancel" => {
                cancel.cancel();
                drop(request);
            }
            _ => {
                drop(run);
                drop(request);
                let got = receive(&c, &mut fake, &cap, "$file").await;
                drop(got);
                continue;
            }
        }
        let error = run.await.err().expect("failed bytes must never escape");
        match case {
            "hash" => assert_eq!(
                error,
                ReceiveError::Download(crate::MediaDownloadError::Crypto(
                    hagency_media::Error::Integrity
                ))
            ),
            "cancel" => assert_eq!(error, ReceiveError::Authority(Error::Cancelled)),
            _ => assert_eq!(
                error,
                ReceiveError::Download(crate::MediaDownloadError::Transport(Error::Transport))
            ),
        }
        let got = receive(&c, &mut fake, &cap, "$file").await;
        drop(got);
    }
    close(c, f, fake).await;
}

#[tokio::test]
async fn native_matrix_receive_capacity() {
    let (f, mut fake, c) = ready(true, vec![file("file", false)]).await;
    let cap = cap(&f, 60_000).await;
    let c = Arc::new(c);
    let copy = c.clone();
    let mut held = vec![];
    for _ in 0..4 {
        held.push(receive(&copy, &mut fake, &cap, "$file").await);
    }
    c.inner
        .owner
        .lock()
        .await
        .take()
        .unwrap()
        .close()
        .await
        .unwrap();
    assert!(matches!(
        c.receive_attachment(cap.clone(), "$file".into(), &CancellationToken::new())
            .await,
        Err(ReceiveError::Authority(Error::Capacity))
    ));
    fake.no_request().await;
    drop(held.pop());
    held.push(receive(&copy, &mut fake, &cap, "$file").await); // existing SDK reopen
    assert!(matches!(
        copy.receive_attachment(cap.clone(), "$file".into(), &CancellationToken::new())
            .await,
        Err(ReceiveError::Authority(Error::Capacity))
    ));
    fake.no_request().await;
    drop(copy);
    drop(held);
    let c = Arc::try_unwrap(c).ok().unwrap();
    close(c, f, fake).await;
}

#[tokio::test]
async fn native_matrix_receive_deadline() {
    let (f, mut fake, mut c) = ready(true, vec![file("file", false)]).await;
    let cap = cap(&f, 60_000).await;
    Arc::get_mut(&mut c.inner).unwrap().config.limits.sdk = Duration::from_millis(100);
    let lock = c.inner.owner.lock().await;
    let cancel = CancellationToken::new();
    assert!(matches!(
        c.receive_attachment(cap.clone(), "$file".into(), &cancel)
            .await,
        Err(ReceiveError::Authority(Error::Timeout))
    ));
    let cancelled = CancellationToken::new();
    cancelled.cancel();
    assert!(matches!(
        c.receive_attachment(cap.clone(), "$file".into(), &cancelled)
            .await,
        Err(ReceiveError::Authority(Error::Cancelled))
    ));
    drop(lock);
    fake.no_request().await;
    // Keep actual SDK/domain preparation inside its normal ten-second budget;
    // reaching GET must not depend on that work finishing within100ms on CI.
    // The independent HTTP sublimits are deliberately longer than the total.
    let config = &mut Arc::get_mut(&mut c.inner).unwrap().config;
    config.limits.sdk = common::limits().sdk;
    config.limits.request = Duration::from_secs(30);
    config.limits.headers = Duration::from_secs(20);
    let started = tokio::time::Instant::now();
    let mut run = Box::pin(c.receive_attachment(cap.clone(), "$file".into(), &cancel));
    let request = next(run.as_mut(), &mut fake).await;
    // Hold an actual TLS request with no response. A renewed HTTP deadline would
    // exceed this fixture watchdog; the original total must settle first.
    let result = tokio::time::timeout_at(started + Duration::from_secs(15), run)
        .await
        .expect("original total deadline must precede the HTTP header budget");
    assert!(matches!(
        result,
        Err(ReceiveError::Authority(Error::Timeout))
            | Err(ReceiveError::Download(
                crate::MediaDownloadError::Transport(Error::Timeout)
            ))
    ));
    assert!(started.elapsed() >= common::limits().sdk);
    drop(request);
    close(c, f, fake).await;
}

#[tokio::test]
async fn native_matrix_received_scope() {
    let (f, mut fake, c) = ready(true, vec![file("file", false)]).await;
    let cap = cap(&f, 60_000).await;
    let ticket = f
        .store
        .authorize_attachment(cap.clone(), "$file".into())
        .await
        .unwrap();
    let mut scopes = vec![];
    // More retained scopes than either four-result pool: conversion must release
    // both the outer result permit and the plaintext codec permit.
    for _ in 0..6 {
        let got = receive(&c, &mut fake, &cap, "$file").await;
        assert_eq!(got.bytes(), vector().2);
        assert_eq!(got.ticket().manifest_id(), ticket.manifest_id());
        assert_eq!(got.ticket().source_sequence(), ticket.source_sequence());
        assert_eq!(got.ticket().content_digest(), ticket.content_digest());
        got.revalidate().await.unwrap();
        let scope = got.into_scope();
        assert_eq!(scope.ticket().metadata(), ticket.metadata());
        scope
            .revalidate(
                &CancellationToken::new(),
                tokio::time::Instant::now() + common::limits().sdk,
            )
            .await
            .unwrap();
        scopes.push(scope);
    }
    f.store
        .complete_dispatch(cap, json!({"fixture":"original turn ended"}), now())
        .await
        .unwrap();
    for scope in scopes {
        assert!(matches!(
            scope
                .revalidate(
                    &CancellationToken::new(),
                    tokio::time::Instant::now() + common::limits().sdk
                )
                .await,
            Err(ReceiveError::Authority(Error::Domain | Error::Generation))
        ));
    }
    fake.no_request().await;
    close(c, f, fake).await;
}

#[tokio::test]
async fn native_matrix_receive_lower_limit() {
    for declared in [999, 1] {
        let mut value = file("file", false);
        value["content"]["info"]["size"] = json!(declared);
        let (f, mut fake, c) = ready(true, vec![value]).await;
        let cap = cap(&f, 60_000).await;
        let cancel = CancellationToken::new();
        for invalid in [0, 4 * 1024 * 1024 + 1] {
            assert!(matches!(
                c.receive_attachment_until(
                    cap.clone(),
                    "$file".into(),
                    &cancel,
                    tokio::time::Instant::now() + common::limits().sdk,
                    invalid
                )
                .await,
                Err(ReceiveError::Download(crate::MediaDownloadError::Config))
            ));
        }
        fake.no_request().await;
        let mut run = Box::pin(c.receive_attachment_until(
            cap.clone(),
            "$file".into(),
            &cancel,
            tokio::time::Instant::now() + common::limits().sdk,
            256,
        ));
        if declared == 1 {
            // Sender understates size; the authenticated response's actual257
            // bytes must be refused by the shared downloader's lower256 bound.
            next(run.as_mut(), &mut fake)
                .await
                .raw(response(&vector().1));
        }
        assert!(matches!(
            run.await,
            Err(ReceiveError::Download(
                crate::MediaDownloadError::Transport(Error::BodyTooLarge)
            ))
        ));
        fake.no_request().await;
        let allowed = if declared == 1 { 257 } else { 999 };
        let mut run = Box::pin(c.receive_attachment_until(
            cap.clone(),
            "$file".into(),
            &cancel,
            tokio::time::Instant::now() + common::limits().sdk,
            allowed,
        ));
        next(run.as_mut(), &mut fake)
            .await
            .raw(response(&vector().1));
        let got = run.await.unwrap();
        assert_eq!(got.bytes(), vector().2);
        assert_eq!(
            got.digest().as_slice(),
            Sha256::digest(got.bytes()).as_slice()
        );
        drop(got);
        close(c, f, fake).await;
    }
}

#[tokio::test]
async fn native_matrix_received_scope_deadline() {
    let (f, mut fake, c) = ready(true, vec![file("file", false)]).await;
    let cap = cap(&f, 60_000).await;
    let cancel = CancellationToken::new();
    let deadline = tokio::time::Instant::now() + common::limits().sdk;
    let mut run =
        Box::pin(c.receive_attachment_until(cap.clone(), "$file".into(), &cancel, deadline, 1024));
    next(run.as_mut(), &mut fake)
        .await
        .raw(response(&vector().1));
    let got = run.await.unwrap();
    got.revalidate().await.unwrap();
    // Use the real original budget, without shortening SDK setup on loaded CI.
    tokio::time::sleep_until(deadline).await;
    assert_eq!(
        got.revalidate().await,
        Err(ReceiveError::Authority(Error::Timeout))
    );
    cancel.cancel();
    assert_eq!(
        got.revalidate().await,
        Err(ReceiveError::Authority(Error::Cancelled))
    );
    let scope = got.into_scope();
    let fresh = CancellationToken::new();
    let read_deadline = tokio::time::Instant::now() + common::limits().sdk;
    scope.revalidate(&fresh, read_deadline).await.unwrap();
    assert_eq!(
        scope.revalidate(&cancel, read_deadline).await,
        Err(ReceiveError::Authority(Error::Cancelled))
    );
    assert_eq!(
        scope.revalidate(&fresh, deadline).await,
        Err(ReceiveError::Authority(Error::Timeout))
    );
    // Fresh read time cannot restore completed original task authority.
    f.store
        .complete_dispatch(
            cap,
            json!({"fixture":"expired receive and ended turn"}),
            now(),
        )
        .await
        .unwrap();
    assert!(matches!(
        scope.revalidate(&fresh, read_deadline).await,
        Err(ReceiveError::Authority(Error::Domain | Error::Generation))
    ));
    fake.no_request().await;
    close(c, f, fake).await;
}
