use super::*;
#[tokio::test]
async fn native_private_approval_fresh_enrollment_and_delivery() {
    let mut f = Fixture::new().await;
    f.enroll().await.unwrap();
    assert_eq!(f.peer.claims, 1);
    assert_eq!(f.peer.writes.len(), 5);
    let card = f.card(1, true).await;
    let expected = card.content().clone();
    let id = card.target().request_id.clone();
    let sent = f.send(card.clone()).await.unwrap();
    assert_eq!(sent.state, PrivateApprovalDeliveryState::Accepted);
    assert!(!sent.replayed);
    assert_eq!(f.peer.events.len(), 1);
    assert_eq!(f.peer.events[0]["content"], expected);
    assert_eq!(f.peer.shares, 1);
    assert_eq!(
        f.base.store.approval_summary(id).await.unwrap().state,
        "pending"
    );
    let replay = f
        .collector
        .send_private_approval_card(card, &CancellationToken::new())
        .await
        .unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.state, PrivateApprovalDeliveryState::Accepted);
    f.fake.no_request().await;
    f.close().await;
}
#[tokio::test]
async fn native_private_approval_enrollment_refusals() {
    for variant in [
        "anchor",
        "versions",
        "existing",
        "signature",
        "claim",
        "fresh_device",
        "uia",
    ] {
        let mut f = Fixture::new().await;
        if variant == "anchor" {
            f.peer = crypto::Peer::for_sender(BOT, DEVICE).await;
        }
        let result=drive_with(f.collector.enroll_fresh_account(&CancellationToken::new()),&mut f.fake,&mut f.peer,|request,peer,reply|{
            if variant=="versions"&&request.target.ends_with("/versions"){reply.1=json!({"versions":["v9.99"]});}
            if request.target.ends_with("/keys/query") {
                if variant=="existing"&&peer.writes.is_empty(){reply.1["master_keys"][BOT]=reply.1["master_keys"][crypto::HUMAN].clone();}
                if variant=="signature"&&peer.writes.is_empty(){reply.1["device_keys"][crypto::HUMAN][crypto::HUMAN_DEVICE]["signatures"]=json!({});}
                if variant=="fresh_device"&&!peer.writes.is_empty(){reply.1["device_keys"][BOT][DEVICE]["keys"][format!("ed25519:{DEVICE}")]=json!("A".repeat(43));}
            }
            if variant=="claim"&&request.target.ends_with("/keys/claim"){reply.1["one_time_keys"]=json!({});}
            if variant=="uia"&&request.target.ends_with("/device_signing/upload"){reply.0=401;reply.1=json!({"flows":[{"stages":["m.login.password"]}],"session":"disposable-uia"});}
        }).await;
        assert!(result.is_err(), "{variant}");
        let writes = f.peer.writes.len();
        let claims = f.peer.claims;
        assert_eq!(
            f.collector
                .enroll_fresh_account(&CancellationToken::new())
                .await,
            result,
            "non-rearmable {variant}"
        );
        f.fake.no_request().await;
        assert_eq!(f.peer.writes.len(), writes);
        assert_eq!(f.peer.claims, claims);
        f.close().await;
    }
    let mut f = Fixture::new().await;
    f.enroll().await.unwrap();
    let guard = f.collector.inner.owner.lock().await;
    let owner = guard.as_ref().unwrap();
    assert!(matches!(
        owner
            .enrollment_handle_for(crate::sdk::enrollment::Purpose::Agent)
            .command(crate::sdk::enrollment::Command::Status)
            .await,
        Err(Error::Config)
    ));
    assert!(matches!(
        owner.outgoing(crate::outgoing::state::Command::Read).await,
        Err(Error::Generation)
    ));
    drop(guard);
    f.close().await;
}
#[tokio::test]
async fn native_private_approval_enrollment_refusals_original_custody() {
    use crate::sdk::enrollment::{Command as Enroll, Purpose, ReplyHold};
    for phase in 1..=3 {
        let mut f = Fixture::new().await;
        // Actual authenticated approval observations precede creating this same
        // owner; no keys/trust/sessions are seeded into it by the fixture.
        *f.collector.inner.owner.lock().await =
            Some(Owner::open(&f.collector.inner.config).await.unwrap());
        let handle = f
            .collector
            .inner
            .owner
            .lock()
            .await
            .as_ref()
            .unwrap()
            .enrollment_handle_for(Purpose::Approval);
        let (reached, wait) = tokio::sync::oneshot::channel();
        let (release, resume) = tokio::sync::oneshot::channel();
        handle
            .command(Enroll::HoldReply(ReplyHold {
                target: phase,
                reached,
                release: resume,
                lose_reply: false,
            }))
            .await
            .unwrap();
        let cancel = CancellationToken::new();
        let mut operation = Box::pin(f.collector.enroll_fresh_account(&cancel));
        tokio::pin!(wait);
        loop {
            tokio::select! {r=&mut operation=>panic!("enrollment escaped its original held command {phase}: {r:?}"),r=&mut wait=>{r.unwrap();break;},request=f.fake.next()=>respond(request,&mut f.peer).await}
        }
        cancel.cancel();
        drop(operation);
        release.send(()).unwrap();
        let permit = f
            .collector
            .inner
            .busy
            .clone()
            .acquire_owned()
            .await
            .unwrap();
        drop(permit);
        assert!(
            f.collector
                .enroll_fresh_account(&CancellationToken::new())
                .await
                .is_err()
        );
        f.fake.no_request().await;
        assert_eq!(
            f.peer.writes.len(),
            match phase {
                1 => 0,
                2 => 1,
                3 => 5,
                _ => unreachable!(),
            }
        );
        f.collector.close().await.unwrap();
        let owner = Owner::open_existing(&f.collector.inner.config)
            .await
            .unwrap();
        let history = owner
            .enrollment_handle_for(Purpose::Approval)
            .command(Enroll::Status)
            .await;
        if phase == 3 {
            assert!(matches!(
                history,
                Ok(crate::enrollment::state::View::Complete)
            ));
        } else {
            assert!(matches!(history, Err(Error::OutcomeUnknown)));
        }
        f.fake.no_request().await;
        owner.close().await.unwrap();
        common::shutdown_domain(&f.base.store, "approval-enrollment-original").await;
        f.fake.close().await;
    }
}

#[tokio::test]
async fn native_private_approval_fresh_enrollment_and_delivery_after_expired_refusal() {
    let mut f = Fixture::new().await;
    f.enroll().await.unwrap();
    let original = f.card(1, true).await;
    let expired = Arc::new(
        f.base
            .store
            .private_approval_card(original.target().request_id.clone(), now() + 100)
            .await
            .unwrap(),
    );
    tokio::time::sleep(Duration::from_millis(120)).await;
    assert!(
        f.collector
            .send_private_approval_card(expired, &CancellationToken::new())
            .await
            .is_err()
    );
    f.fake.no_request().await;
    let status = f
        .collector
        .private_approval_delivery_status()
        .await
        .unwrap();
    assert_eq!(status.stage, PrivateApprovalDeliveryStage::Idle);
    assert_eq!((status.receipts, status.writes, status.accepted), (0, 0, 0));
    let valid = f.card(2, true).await;
    let expected = valid.content().clone();
    assert_eq!(
        f.send(valid).await.unwrap().state,
        PrivateApprovalDeliveryState::Accepted
    );
    assert_eq!(f.peer.events.len(), 1);
    assert_eq!(f.peer.events[0]["content"], expected);
    f.close().await;
}
