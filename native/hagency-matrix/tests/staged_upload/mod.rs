use super::*;
use crate::collector::fixtures as common;
use std::{
    sync::{OnceLock, atomic::Ordering},
    time::Duration,
};
pub(super) mod fixture;
use fixture::*;
fn serial() -> &'static tokio::sync::Mutex<()> {
    static SERIAL: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    SERIAL.get_or_init(|| tokio::sync::Mutex::new(()))
}
async fn gate(inner: &Inner) {
    tokio::time::timeout(Duration::from_secs(15), inner.uploads.reached.notified())
        .await
        .unwrap();
}
#[tokio::test]
async fn native_staged_upload_complete() {
    if child_settlement().await {
        return;
    }
    let _serial = serial().lock().await;
    let mut f = Fixture::new().await;
    let Some((input, identity, ciphertext)) = f.input("complete").await else {
        f.finish().await;
        return;
    };
    let mut op = f.admit(input);
    let cancel = CancellationToken::new();
    let (result, ()) = common::scripted(op.run(&cancel), post(&mut f.fake, &ciphertext)).await;
    let receipt = result.unwrap();
    assert_eq!(receipt.id, identity.id());
    assert_eq!(receipt.upload, hagency_core::uploads::UploadState::Accepted);
    assert!(!receipt.cancel_requested);
    assert_eq!(op.run(&cancel).await, Err(Error::Conflict));
    let projected = serde_json::to_string(&receipt).unwrap();
    for forbidden in [
        "remote.test",
        "original.txt",
        "workspace",
        "mxc://",
        "example.test",
        "key",
    ] {
        assert!(!projected.contains(forbidden));
    }
    // First prove SDK reopen. The separate child below drops all original
    // in-memory inputs before reconstructing only the exact historical lookup.
    drop(op);
    let id = receipt.id.clone();
    f.collector.reopen_upload_owner(&cancel).await.unwrap();
    let restored = f.collector.settle_upload(&id, &cancel).await.unwrap();
    assert!(restored.replayed);
    let sql = rusqlite::Connection::open(f.base.root.path().join("domain/domain.sqlite3")).unwrap();
    // Canonical task remains executable; settlement never emits a Done mutation.
    {
        let done: bool = sql.query_row("SELECT json_extract(config,'$.status')='done' FROM canonical_tasks WHERE id='task'", [], |r|r.get(0)).unwrap();
        assert!(!done);
    }
    f.fake.no_request().await;
    drop(sql);
    f.settle_in_new_process(id, true).await;
}
#[tokio::test]
async fn native_staged_upload_scope() {
    let _serial = serial().lock().await;
    let mut f = Fixture::new().await;
    let Some((a, _, acipher)) = f.input("a").await else {
        f.finish().await;
        return;
    };
    let (b, _, _) = f.input("b").await.unwrap();
    let (cap_a, claim_a, mut send_a, mut media_a) = a.into_parts();
    for field in 0..4 {
        let mut invalid = cap_a.clone();
        match field {
            0 => invalid.secret = "a".repeat(4096),
            1 => invalid.runner_id = "r".repeat(4096),
            2 => invalid.dispatch_id = "d".repeat(4096),
            _ => invalid.fence = 0,
        }
        let failure = StagedUpload::new(invalid, claim_a.clone(), send_a, media_a)
            .err()
            .unwrap();
        assert_eq!(failure.error(), Error::Conflict);
        let failure = f
            .collector
            .stage_upload(failure.into_input())
            .err()
            .unwrap();
        assert_eq!(failure.error(), Error::Conflict);
        let (returned, _, original_send, original_media) = failure.into_input().into_parts();
        match field {
            0 => assert_eq!(returned.secret.len(), 4096),
            1 => assert_eq!(returned.runner_id.len(), 4096),
            2 => assert_eq!(returned.dispatch_id.len(), 4096),
            _ => assert_eq!(returned.fence, 0),
        }
        assert_eq!(original_media.ciphertext(), acipher);
        send_a = original_send;
        media_a = original_media;
    }
    let (cap_b, claim_b, send_b, media_b) = b.into_parts();
    let descriptor = media_a.descriptor().private_event_json().to_vec();
    let bad = StagedUpload::new(cap_a.clone(), claim_b.clone(), send_a, media_a)
        .err()
        .unwrap();
    assert_eq!(bad.error(), Error::Conflict);
    let rejected = f.collector.stage_upload(bad.into_input()).err().unwrap();
    assert_eq!(rejected.error(), Error::Conflict);
    let (_, _, send_a, media_a) = rejected.into_input().into_parts();
    assert_eq!(media_a.ciphertext(), acipher);
    assert_eq!(media_a.descriptor().private_event_json(), descriptor);
    let bad = StagedUpload::new(cap_a.clone(), claim_a.clone(), send_a, media_b)
        .err()
        .unwrap();
    assert_eq!(bad.error(), Error::Conflict);
    let (_, _, send_a, media_b) = bad.into_input().into_parts();
    let a = StagedUpload::new(cap_a, claim_a, send_a, media_a)
        .map_err(|e| e.error())
        .unwrap();
    let _b = StagedUpload::new(cap_b, claim_b, send_b, media_b)
        .map_err(|e| e.error())
        .unwrap();
    let mut op = f.admit(a);
    f.revoke().await;
    assert!(op.run(&CancellationToken::new()).await.is_err());
    assert_eq!(
        op.run(&CancellationToken::new()).await,
        Err(Error::Conflict)
    );
    f.fake.no_request().await;
    drop(op);
    f.finish().await;
    let mut f = Fixture::new().await;
    let (input, _, _) = f.input("wrong_account").await.unwrap();
    // Existing SDK plus available prior observation survive credential rotation.
    // This fixture closes the actual SDK owner without a transport invalidation.
    f.collector
        .inner
        .owner
        .lock()
        .await
        .take()
        .unwrap()
        .close()
        .await
        .unwrap();
    assert!(f.base.available().await);
    let mut config = f
        .base
        .config(&f.fake.endpoint)
        .with_root_pem(include_bytes!("../fixtures/ca.pem"))
        .unwrap();
    config.authorization =
        reqwest::header::HeaderValue::from_static("Bearer synthetic-other-account-token");
    let other = Collector::new(config, f.base.store.clone()).unwrap();
    let mut op = other.stage_upload(input).map_err(|e| e.error()).unwrap();
    let cancel = CancellationToken::new();
    let script = async {
        let request = f.fake.next().await;
        assert_eq!(request.target, "/_matrix/client/v3/account/whoami");
        assert_eq!(
            request.headers["authorization"],
            "Bearer synthetic-other-account-token"
        );
        request.json(200, serde_json::json!({"user_id":"@someone:example.test","device_id":"OTHER","is_guest":false}));
    };
    let (result, ()) = common::scripted(op.run(&cancel), script).await;
    assert_eq!(result, Err(Error::Identity));
    assert!(!f.base.available().await);
    f.fake.no_request().await;
    drop(op);
    other.close().await.unwrap();
    f.finish().await;
}
#[tokio::test]
async fn native_staged_upload_fence() {
    let _serial = serial().lock().await;
    let mut f = Fixture::new().await;
    let Some((input, identity, _)) = f.input("fence").await else {
        f.finish().await;
        return;
    };
    let mut op = f.admit(input);
    let id = op.id().to_owned();
    f.collector.inner.uploads.gate.store(1, Ordering::SeqCst);
    let cancel = CancellationToken::new();
    let control = async {
        authenticate(&mut f.fake).await;
        gate(&f.collector.inner).await;
        assert_eq!(f.collector.release_unstarted_upload(&id), Err(Error::Busy));
        assert_eq!(f.collector.close().await, Err(Error::Busy));
        f.base
            .store
            .invalidate_matrix_room(hagency_core::replies::MatrixRoomInvalidation {
                engagement_id: f.base.identity.transport.engagement_id.clone(),
                registration_generation: 1,
                transport_generation: 1,
                room_id: "!direct:example.test".into(),
                generation: 2,
                reason: "privacy lost".into(),
            })
            .await
            .unwrap();
        f.collector.inner.uploads.proceed.notify_one();
    };
    let (result, ()) = tokio::join!(op.run(&cancel), control);
    assert!(result.is_err());
    assert_eq!(op.run(&cancel).await, Err(Error::Conflict));
    assert_eq!(
        f.base.store.inspect_upload(identity).await.unwrap().upload,
        hagency_core::uploads::UploadState::WritePossible
    );
    f.fake.no_request().await;
    drop(op);
    f.finish().await;
    // Negative account evidence survives abandoning the caller while its
    // bounded independent writer completion is still pending.
    let mut f = Fixture::new().await;
    let (input, _, _) = f.input("negative_abandoned").await.unwrap();
    let mut op = f.admit(input);
    f.collector.inner.uploads.gate.store(3, Ordering::SeqCst);
    let cancel = CancellationToken::new();
    {
        let run = op.run(&cancel);
        tokio::pin!(run);
        tokio::select! {
            result = &mut run => panic!("negative fencing ended before gate: {result:?}"),
            () = async {
                let request = f.fake.next().await;
                assert_eq!(request.target, "/_matrix/client/v3/account/whoami");
                request.json(200, serde_json::json!({"user_id":"@wrong:example.test","device_id":"OTHER"}));
                gate(&f.collector.inner).await;
            } => {}
        }
    }
    assert!(f.base.available().await);
    f.collector.inner.uploads.proceed.notify_one();
    tokio::time::timeout(Duration::from_secs(10), op.job.fence_finished.notified())
        .await
        .unwrap();
    assert!(!f.base.available().await);
    assert_eq!(op.outcome().unwrap(), Some(Err(Error::Identity)));
    f.collector.release_unstarted_upload(op.id()).unwrap();
    f.fake.no_request().await;
    drop(op);
    f.finish().await;
}
#[tokio::test]
async fn native_staged_upload_cancellation() {
    let _serial = serial().lock().await;
    let mut f = Fixture::new().await;
    let Some((input, identity, _)) = f.input("cancel").await else {
        f.finish().await;
        return;
    };
    let mut op = f.admit(input);
    let cancel = CancellationToken::new();
    let script = async {
        authenticate(&mut f.fake).await;
        let request = f.fake.next().await;
        assert_eq!(request.method, "POST");
        cancel.cancel();
        drop(request);
    };
    let (result, ()) = common::scripted(op.run(&cancel), script).await;
    assert_eq!(result, Err(Error::Cancelled));
    assert_eq!(
        f.collector.release_unstarted_upload(op.id()),
        Err(Error::OutcomeUnknown)
    );
    assert_eq!(f.collector.close().await, Err(Error::Busy));
    assert_eq!(
        f.base.store.inspect_upload(identity).await.unwrap().upload,
        hagency_core::uploads::UploadState::WritePossible
    );
    drop(op);
    let (input, _, _) = f.input("future_drop").await.unwrap();
    let mut op = f.admit(input);
    let cancel = CancellationToken::new();
    {
        let future = op.run(&cancel);
        tokio::pin!(future);
        tokio::select! {
            result = &mut future => panic!("upload ended before paused response: {result:?}"),
            () = async { authenticate(&mut f.fake).await; let request = f.fake.next().await; assert_eq!(request.method,"POST"); drop(request); } => {}
        }
    }
    assert_eq!(op.run(&cancel).await, Err(Error::Conflict));
    assert!(op.outcome().unwrap().is_none());
    assert_eq!(
        f.collector.release_unstarted_upload(op.id()),
        Err(Error::OutcomeUnknown)
    );
    f.collector.reopen_upload_owner(&cancel).await.unwrap();
    assert_eq!(
        f.collector.settle_upload(op.id(), &cancel).await,
        Err(Error::OutcomeUnknown)
    );
    f.fake.no_request().await;
    f.finish().await;
    for truncated in [false, true] {
        let mut f = Fixture::new().await;
        let (input, identity, _) = f.input("bad_response").await.unwrap();
        let mut op = f.admit(input);
        let cancel = CancellationToken::new();
        let script = async {
            authenticate(&mut f.fake).await;
            let request = f.fake.next().await;
            assert_eq!(request.method, "POST");
            if truncated {
                request.raw(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 100\r\nConnection: close\r\n\r\n{}".to_vec());
            } else {
                request.raw(common::response(
                    200,
                    br#"{"content_uri":"mxc://remote.test/a","content_uri":"mxc://remote.test/b"}"#,
                ));
            }
        };
        let (result, ()) = common::scripted(op.run(&cancel), script).await;
        assert_eq!(
            result,
            Err(if truncated {
                Error::Transport
            } else {
                Error::InvalidJson
            })
        );
        assert!(op.job.state.lock().await.response.is_none());
        assert_eq!(
            f.collector.settle_upload(op.id(), &cancel).await,
            Err(Error::OutcomeUnknown)
        );
        assert_eq!(op.run(&cancel).await, Err(Error::Conflict));
        assert_eq!(
            f.base.store.inspect_upload(identity).await.unwrap().upload,
            hagency_core::uploads::UploadState::WritePossible
        );
        f.fake.no_request().await;
        f.finish().await;
    }
}
#[tokio::test]
async fn native_staged_upload_recovery() {
    let _serial = serial().lock().await;
    let mut f = Fixture::new().await;
    let Some((input, identity, ciphertext)) = f.input("recover").await else {
        f.finish().await;
        return;
    };
    let mut op = f.admit(input);
    f.collector.inner.uploads.gate.store(2, Ordering::SeqCst);
    let cancel = CancellationToken::new();
    let sql = rusqlite::Connection::open(f.base.root.path().join("sdk/matrix-sdk-state.sqlite3"))
        .unwrap();
    let control = async {
        post(&mut f.fake, &ciphertext).await;
        gate(&f.collector.inner).await;
        sql.execute_batch("CREATE TRIGGER staged_upload_abort BEFORE INSERT ON kv_blob BEGIN SELECT RAISE(ABORT,'fixture abort'); END;").unwrap();
        f.collector.inner.uploads.proceed.notify_one();
    };
    let (result, ()) = common::scripted(op.run(&cancel), control).await;
    assert_eq!(result, Err(Error::OutcomeUnknown));
    assert_eq!(f.collector.close().await, Err(Error::Busy));
    sql.execute_batch("DROP TRIGGER staged_upload_abort;")
        .unwrap();
    drop(sql);
    let id = op.id().to_owned();
    drop(op); // Collector retains original actual response across SDK replacement.
    f.base.store.cancel_upload(identity).await.unwrap();
    f.revoke().await;
    f.collector.reopen_upload_owner(&cancel).await.unwrap();
    let receipt = f.collector.settle_upload(&id, &cancel).await.unwrap();
    assert_eq!(receipt.upload, hagency_core::uploads::UploadState::Accepted);
    assert!(receipt.cancel_requested);
    assert!(
        f.collector
            .settle_upload(&id, &cancel)
            .await
            .unwrap()
            .replayed
    );
    f.fake.no_request().await;
    f.finish().await;
    // Fully validated response observation survives caller cancellation before
    // any SDK acceptance wait; historical settlement never repeats HTTP.
    let mut f = Fixture::new().await;
    let (input, identity, ciphertext) = f.input("late_cancel").await.unwrap();
    let mut op = f.admit(input);
    f.collector.inner.uploads.gate.store(2, Ordering::SeqCst);
    let cancel = CancellationToken::new();
    let script = async {
        post(&mut f.fake, &ciphertext).await;
        gate(&f.collector.inner).await;
        cancel.cancel();
    };
    let (result, ()) = common::scripted(op.run(&cancel), script).await;
    assert_eq!(result, Err(Error::Cancelled));
    assert_eq!(
        op.job.state.lock().await.response.as_ref().unwrap().body(),
        RAW
    );
    assert_eq!(op.outcome().unwrap(), Some(Err(Error::Cancelled)));
    let id = op.id().to_owned();
    drop(op);
    f.base.store.cancel_upload(identity).await.unwrap();
    f.revoke().await;
    let recovery = CancellationToken::new();
    f.collector.reopen_upload_owner(&recovery).await.unwrap();
    let receipt = f.collector.settle_upload(&id, &recovery).await.unwrap();
    assert_eq!(receipt.upload, hagency_core::uploads::UploadState::Accepted);
    assert!(receipt.cancel_requested);
    f.fake.no_request().await;
    f.finish().await;
    // Actual SDK acceptance precedes this failed first domain commit. Only
    // protected history reaches a fresh process; no original cap survives.
    let mut f = Fixture::new().await;
    let (input, identity, ciphertext) = f.input("domain_gap").await.unwrap();
    let mut op = f.admit(input);
    let sql = rusqlite::Connection::open(f.base.root.path().join("domain/domain.sqlite3")).unwrap();
    sql.execute_batch("CREATE TRIGGER upload_settle_abort BEFORE UPDATE ON file_uploads WHEN NEW.upload_state='accepted' BEGIN SELECT RAISE(ABORT,'fixture acceptance abort'); END;").unwrap();
    let cancel = CancellationToken::new();
    let (result, ()) = common::scripted(op.run(&cancel), post(&mut f.fake, &ciphertext)).await;
    assert_eq!(result, Err(Error::Domain));
    assert_eq!(
        f.base.store.inspect_upload(identity).await.unwrap().upload,
        hagency_core::uploads::UploadState::WritePossible
    );
    sql.execute_batch("DROP TRIGGER upload_settle_abort;")
        .unwrap();
    drop(sql);
    let id = op.id().to_owned();
    drop(op);
    f.settle_in_new_process(id, false).await;
}
#[tokio::test]
async fn native_staged_upload_capacity() {
    let _serial = serial().lock().await;
    let mut f = Fixture::new().await;
    let Some((a, _, _)) = f.input("capacity_a").await else {
        f.finish().await;
        return;
    };
    let (b, _, _) = f.input("capacity_b").await.unwrap();
    let (c, _, ciphertext) = f.input("capacity_c").await.unwrap();
    let mut first = f.admit(a);
    let second = f.admit(b);
    let failure = f.collector.stage_upload(c).err().unwrap();
    assert_eq!(failure.error(), Error::Capacity);
    let c = failure.into_input();
    assert_eq!(c.media.ciphertext(), ciphertext);
    f.collector
        .reopen_upload_owner(&CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(
        f.collector.stage_upload(c).err().unwrap().error(),
        Error::Capacity
    );
    let id = first.id().to_owned();
    f.collector.release_unstarted_upload(&id).unwrap();
    assert_eq!(
        first.run(&CancellationToken::new()).await,
        Err(Error::Conflict)
    );
    // Removing the map entry alone does not release the held handle's slot.
    let (d, _, _) = f.input("capacity_d").await.unwrap();
    let d = f.collector.stage_upload(d).err().unwrap().into_input();
    drop(first);
    let third = f.admit(d);
    f.collector.release_unstarted_upload(second.id()).unwrap();
    f.collector.release_unstarted_upload(third.id()).unwrap();
    drop((second, third));
    f.collector.close().await.unwrap();
    f.fake.no_request().await;
    f.finish().await;
}
