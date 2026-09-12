use super::tests::fixture as upload_fixture;
use super::*;
use crate::{collector::fixtures as common, outgoing::OutgoingState};
use serde_json::{Value, json};
use std::sync::OnceLock;
use std::sync::atomic::Ordering;
use std::time::Duration;
mod fixture;
mod recovery;
use fixture::*;
use upload_fixture::{DATA, Fixture, post};

fn serial() -> &'static tokio::sync::Mutex<()> {
    static SERIAL: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    SERIAL.get_or_init(|| tokio::sync::Mutex::new(()))
}

#[tokio::test]
async fn native_file_publication_recipient() {
    let _serial = serial().lock().await;
    for (direct, caption) in [(true, None), (false, Some("请查看附件"))] {
        let mut f = Fixture::new_profile(direct, !direct).await;
        let Some((original, identity, ciphertext)) = accepted(&mut f, "recipient", caption).await
        else {
            f.finish().await;
            continue;
        };
        let (claim, send) = publication(&f, identity.clone()).await;
        let mut op = original
            .prepare_file_publication(claim, send)
            .map_err(|e| e.error())
            .unwrap();
        let peer = f
            .collector
            .inner
            .owner
            .lock()
            .await
            .as_ref()
            .unwrap()
            .outgoing_fixture(true)
            .await;
        let cancel = CancellationToken::new();
        let (result, plain) = common::scripted(op.run(&cancel), async {
            let (request, content) = wire(
                &mut f.fake,
                &peer,
                if direct {
                    ruma::room_id!("!direct:example.test")
                } else {
                    ruma::room_id!("!project:example.test")
                },
            )
            .await;
            request.json(200, json!({"event_id":"$file-accepted"}));
            content
        })
        .await;
        assert_eq!(result.unwrap().state, OutgoingState::Delivered);
        let content = &plain["content"];
        assert_eq!(content["msgtype"], "m.file");
        assert_eq!(content["body"], caption.unwrap_or("结果.txt"));
        if caption.is_some() {
            assert_eq!(content["filename"], "结果.txt");
        }
        assert_eq!(content["info"]["size"], DATA.len());
        assert_eq!(content["info"]["mimetype"], "application/octet-stream");
        assert_eq!(
            content["m.relates_to"]["event_id"],
            if direct {
                Value::Null
            } else {
                json!("$file-root")
            }
        );
        let mut descriptor = content["file"].clone();
        assert_eq!(
            descriptor.as_object_mut().unwrap().remove("url").unwrap(),
            "mxc://remote.test/original"
        );
        let descriptor = hagency_media::Descriptor::from_private_event_json(
            &serde_json::to_vec(&descriptor).unwrap(),
        )
        .unwrap();
        let codec = hagency_media::Codec::new(hagency_media::Limits::new(4096, 2).unwrap());
        assert_eq!(
            codec.decrypt(&descriptor, &ciphertext).unwrap().bytes(),
            DATA
        );
        let receipt = f
            .base
            .store
            .inspect_file_delivery(f.cap.clone(), identity.id().into())
            .await
            .unwrap();
        assert_eq!(
            receipt.status,
            hagency_core::file_delivery::FileDeliveryStatus::Delivered
        );
        assert_eq!(receipt.event_id.as_deref(), Some("$file-accepted"));
        assert_eq!(op.run(&cancel).await, Err(Error::Conflict));
        f.fake.no_request().await;
        drop(op);
        f.finish().await;
    }
}

#[tokio::test]
async fn native_file_publication_association() {
    let _serial = serial().lock().await;
    let mut f = Fixture::new().await;
    let Some((a, aid, _)) = accepted(&mut f, "association-a", None).await else {
        f.finish().await;
        return;
    };
    let (b, bid, _) = accepted(&mut f, "association-b", Some("different"))
        .await
        .unwrap();
    let (ac, a_send) = publication(&f, aid).await;
    let (bc, b_send) = publication(&f, bid).await;
    let error = match a.prepare_file_publication(bc, b_send) {
        Err(e) => e,
        Ok(_) => panic!("foreign upload admitted"),
    };
    assert_eq!(error.error(), Error::Conflict);
    let (a, bc, b_send) = error.into_parts();
    let error = match a.prepare_file_publication(bc.clone(), a_send) {
        Err(e) => e,
        Ok(_) => panic!("foreign claim admitted"),
    };
    assert_eq!(error.error(), Error::Conflict);
    let (a, _, a_send) = error.into_parts();
    let a = a
        .prepare_file_publication(ac, a_send)
        .map_err(|e| e.error())
        .unwrap();
    let b = b
        .prepare_file_publication(bc, b_send)
        .map_err(|e| e.error())
        .unwrap();
    let (third, _, _) = f.file_input("association-third", None).await.unwrap();
    let refused = match f.collector.stage_upload(third) {
        Err(e) => e,
        Ok(_) => panic!("original media capacity reset"),
    };
    assert_eq!(refused.error(), Error::Capacity);
    drop(refused.into_input());
    f.fake.no_request().await;
    let weak = Arc::downgrade(&f.collector.inner);
    drop((a, b));
    f.finish().await;
    assert!(
        weak.upgrade().is_none(),
        "unrun registry retained its Collector"
    );
}

#[tokio::test]
async fn native_file_publication_current_scope() {
    let _serial = serial().lock().await;
    let mut f = Fixture::new().await;
    let Some((original, id, _)) = accepted(&mut f, "retired", None).await else {
        f.finish().await;
        return;
    };
    let (claim, send) = publication(&f, id.clone()).await;
    let mut op = original
        .prepare_file_publication(claim, send)
        .map_err(|e| e.error())
        .unwrap();
    let peer = f
        .collector
        .inner
        .owner
        .lock()
        .await
        .as_ref()
        .unwrap()
        .outgoing_fixture(true)
        .await;
    f.collector.inner.outgoing_fault.store(5, Ordering::SeqCst);
    let inner = f.collector.inner.clone();
    let cancel = CancellationToken::new();
    let (result, ()) = common::scripted(op.run(&cancel), async {
        preflight(&mut f.fake).await;
        f.fake.next().await.json(200, peer.query.clone());
        preflight(&mut f.fake).await;
        f.fake.next().await.json(200, peer.query.clone());
        tokio::time::timeout(Duration::from_secs(10), inner.outgoing_reached.notified())
            .await
            .unwrap();
        f.revoke().await;
        inner.outgoing_continue.notify_one();
    })
    .await;
    assert_eq!(result, Err(Error::Domain));
    assert_eq!(op.run(&cancel).await, Err(Error::Conflict));
    let resumed = f.collector.resume_outgoing_custody(&cancel).await.unwrap();
    assert_eq!(resumed.state, OutgoingState::Uncertain);
    f.fake.no_request().await;
    drop((inner, op));
    f.finish().await;
}

#[tokio::test]
async fn native_file_publication_privacy() {
    let _serial = serial().lock().await;
    for identity in [true, false] {
        let mut f = Fixture::new().await;
        let Some((original, id, _)) = accepted(&mut f, "privacy", None).await else {
            f.finish().await;
            return;
        };
        let (claim, send) = publication(&f, id.clone()).await;
        let mut op = original
            .prepare_file_publication(claim, send)
            .map_err(|e| e.error())
            .unwrap();
        let cancel = CancellationToken::new();
        let (result, ()) = common::scripted(op.run(&cancel), async {
            let request = f.fake.next().await;
            assert_eq!(request.target, "/_matrix/client/v3/account/whoami");
            let mut who = common::who();
            if identity { who["user_id"] = json!("@different:example.test"); }
            request.json(200, who);
            if !identity {
                let mut state = common::state();
                state.as_array_mut().unwrap().push(json!({"type":"m.room.member","state_key":"@stranger:example.test","content":{"membership":"join"}}));
                f.fake.next().await.json(200, state);
            }
        }).await;
        assert!(result.is_err());
        f.fake.no_request().await;
        if identity {
            assert!(!f.base.available().await);
        }
        assert_eq!(op.run(&cancel).await, Err(Error::Conflict));
        drop(op);
        f.finish().await;
    }
}

#[tokio::test]
async fn native_file_publication_uncertain() {
    let _serial = serial().lock().await;
    let mut f = Fixture::new().await;
    let Some((original, id, _)) = accepted(&mut f, "uncertain", None).await else {
        f.finish().await;
        return;
    };
    let (claim, send) = publication(&f, id.clone()).await;
    let mut op = original
        .prepare_file_publication(claim, send)
        .map_err(|e| e.error())
        .unwrap();
    let peer = f
        .collector
        .inner
        .owner
        .lock()
        .await
        .as_ref()
        .unwrap()
        .outgoing_fixture(true)
        .await;
    let cancel = CancellationToken::new();
    let (result, ()) = common::scripted(op.run(&cancel), async {
        let (request, _) = wire(&mut f.fake, &peer, ruma::room_id!("!direct:example.test")).await;
        drop(request); // Real complete encrypted event left; HTTP ACK is lost.
    })
    .await;
    assert!(result.is_err());
    let receipt = f
        .base
        .store
        .inspect_file_delivery(f.cap.clone(), id.id().into())
        .await
        .unwrap();
    assert_eq!(
        receipt.status,
        hagency_core::file_delivery::FileDeliveryStatus::OutcomeUnknown
    );
    assert!(receipt.event_id.is_none());
    f.collector.reopen_upload_owner(&cancel).await.unwrap();
    assert_eq!(
        f.collector
            .resume_outgoing_custody(&cancel)
            .await
            .unwrap()
            .state,
        OutgoingState::Uncertain
    );
    assert_eq!(op.run(&cancel).await, Err(Error::Conflict));
    f.fake.no_request().await;
    let weak = Arc::downgrade(&f.collector.inner);
    drop(op);
    f.finish().await;
    assert!(
        weak.upgrade().is_none(),
        "uncertain registry retained its Collector"
    );
}

#[tokio::test]
async fn native_file_publication_custody() {
    let _serial = serial().lock().await;
    let mut f = Fixture::new().await;
    let Some((original, id, _)) = accepted(&mut f, "caller-loss", None).await else {
        f.finish().await;
        return;
    };
    let (claim, send) = publication(&f, id.clone()).await;
    let mut op = original
        .prepare_file_publication(claim, send)
        .map_err(|e| e.error())
        .unwrap();
    let peer = f
        .collector
        .inner
        .owner
        .lock()
        .await
        .as_ref()
        .unwrap()
        .outgoing_fixture(true)
        .await;
    let cancel = CancellationToken::new();
    let mut caller = Box::pin(op.run(&cancel));
    let mut script = Box::pin(wire(
        &mut f.fake,
        &peer,
        ruma::room_id!("!direct:example.test"),
    ));
    let (request, _) = tokio::select! {
        result = &mut caller => panic!("sender finished before ACK: {result:?}"),
        result = &mut script => result,
    };
    drop(caller);
    request.json(200, json!({"event_id":"$caller-gone"}));
    tokio::time::timeout(Duration::from_secs(10), async {
        while op.outcome().unwrap().is_none() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        op.outcome().unwrap().unwrap().unwrap().state,
        OutgoingState::Delivered
    );
    drop(script);
    f.fake.no_request().await;
    drop(op);
    let (original, cancelled_id, _) = accepted(&mut f, "cancelled", None).await.unwrap();
    let (claim, send) = publication(&f, cancelled_id.clone()).await;
    let mut cancelled = original
        .prepare_file_publication(claim, send)
        .map_err(|e| e.error())
        .unwrap();
    let token = CancellationToken::new();
    token.cancel();
    assert_eq!(cancelled.run(&token).await, Err(Error::Cancelled));
    let status = f
        .base
        .store
        .inspect_file_delivery(f.cap.clone(), cancelled_id.id().into())
        .await
        .unwrap();
    assert!(status.cancel_requested);
    assert!(status.event_id.is_none());
    let view = f
        .collector
        .inner
        .owner
        .lock()
        .await
        .as_ref()
        .unwrap()
        .outgoing(crate::outgoing::state::Command::Read)
        .await
        .unwrap();
    assert!(view.attempt.is_none());
    assert!(
        view.receipts
            .iter()
            .any(|receipt| receipt.kind == crate::outgoing::state::Kind::File
                && receipt.id != cancelled_id.id())
    );
    assert_eq!(cancelled.outcome().unwrap(), Some(Err(Error::Cancelled)));
    assert_eq!(
        f.collector
            .resume_outgoing_custody(&CancellationToken::new())
            .await,
        Err(Error::OutcomeUnknown)
    );
    assert_eq!(cancelled.outcome().unwrap(), Some(Err(Error::Cancelled)));
    assert_eq!(f.collector.close().await, Err(Error::Busy));
    f.fake.no_request().await;
    let weak = Arc::downgrade(&f.collector.inner);
    drop(cancelled);
    f.finish().await;
    assert!(
        weak.upgrade().is_none(),
        "cancelled registry retained its Collector"
    );
}

#[tokio::test]
async fn native_file_publication_historical() {
    if recovery::child().await {
        return;
    }
    let _serial = serial().lock().await;
    let mut f = Fixture::new().await;
    let Some((original, id, _)) = accepted(&mut f, "historical", None).await else {
        f.finish().await;
        return;
    };
    let (claim, send) = publication(&f, id.clone()).await;
    let mut op = original
        .prepare_file_publication(claim, send)
        .map_err(|e| e.error())
        .unwrap();
    let peer = f
        .collector
        .inner
        .owner
        .lock()
        .await
        .as_ref()
        .unwrap()
        .outgoing_fixture(true)
        .await;
    f.collector.inner.outgoing_fault.store(1, Ordering::SeqCst);
    let cancel = CancellationToken::new();
    let (result, ()) = common::scripted(op.run(&cancel), async {
        let (request, _) = wire(&mut f.fake, &peer, ruma::room_id!("!direct:example.test")).await;
        request.json(200, json!({"event_id":"$historical-file"}));
    })
    .await;
    assert_eq!(result, Err(Error::Busy));
    let receipt = f
        .base
        .store
        .inspect_file_delivery(f.cap.clone(), id.id().into())
        .await
        .unwrap();
    assert_eq!(
        receipt.event,
        hagency_core::file_delivery::FileEventState::WritePossible
    );
    assert!(receipt.event_id.is_none());
    drop((op, peer));
    recovery::restart(f, id.id()).await;
    recovery::settle_ack_loss().await;
}

#[tokio::test]
async fn native_file_publication_journal() {
    let _serial = serial().lock().await;
    recovery::settled_receipt_substitution().await;
    for variant in 10..=14 {
        let mut f = Fixture::new().await;
        let Some((original, id, _)) = accepted(&mut f, "journal", None).await else {
            f.finish().await;
            return;
        };
        let (claim, send) = publication(&f, id.clone()).await;
        let mut op = original
            .prepare_file_publication(claim, send)
            .map_err(|e| e.error())
            .unwrap();
        let peer = f
            .collector
            .inner
            .owner
            .lock()
            .await
            .as_ref()
            .unwrap()
            .outgoing_fixture(true)
            .await;
        f.collector.inner.outgoing_fault.store(1, Ordering::SeqCst);
        let cancel = CancellationToken::new();
        let (result, ()) = common::scripted(op.run(&cancel), async {
            let (request, _) =
                wire(&mut f.fake, &peer, ruma::room_id!("!direct:example.test")).await;
            request.json(200, json!({"event_id":"$journal"}));
        })
        .await;
        assert_eq!(result, Err(Error::Busy));
        f.collector
            .inner
            .owner
            .lock()
            .await
            .as_ref()
            .unwrap()
            .corrupt_outgoing_fixture(variant)
            .await;
        let reopened = f.collector.reopen_upload_owner(&cancel).await;
        if variant == 14 {
            reopened.unwrap();
            let (next, next_id, _) = accepted(&mut f, "catalog-full", None).await.unwrap();
            let (claim, send) = publication(&f, next_id).await;
            let mut refused = next
                .prepare_file_publication(claim, send)
                .map_err(|e| e.error())
                .unwrap();
            assert_eq!(refused.run(&cancel).await, Err(Error::Capacity));
            drop(refused);
        } else if variant == 10 || variant == 11 {
            reopened.unwrap(); // A valid envelope is not original metadata proof.
            assert!(
                f.collector.resume_outgoing_custody(&cancel).await.is_err(),
                "changed metadata settled, variant {variant}"
            );
        } else {
            assert!(
                reopened.is_err(),
                "changed original descriptor or upload accepted, variant {variant}"
            );
        }
        let receipt = f
            .base
            .store
            .inspect_file_delivery(f.cap.clone(), id.id().into())
            .await
            .unwrap();
        assert_eq!(
            receipt.event,
            hagency_core::file_delivery::FileEventState::WritePossible
        );
        assert!(receipt.event_id.is_none());
        f.fake.no_request().await;
        drop((op, peer));
        f.finish().await;
    }
}
