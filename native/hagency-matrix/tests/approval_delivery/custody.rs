use super::*;
use crate::collector::observation::{Phase as ObservedPhase, Trace, observed};
#[tokio::test]
async fn native_private_approval_send_cancellation_and_loss() {
    // The actual successful to-device response is in the original SDK before
    // cancellation/caller loss. Its next room write must not start.
    let mut f = Fixture::new().await;
    f.enroll().await.unwrap();
    let card = f.card(1, true).await;
    let (reached, wait) = tokio::sync::oneshot::channel();
    let (release, resume) = tokio::sync::oneshot::channel();
    let handle = f
        .collector
        .inner
        .owner
        .lock()
        .await
        .as_ref()
        .unwrap()
        .approval_delivery_handle();
    handle
        .command(Command::Hold(crate::sdk::approval_delivery::ReplyHold {
            phase: 4,
            reached,
            release: resume,
            lose: false,
        }))
        .await
        .unwrap();
    let cancel = CancellationToken::new();
    let mut operation = Box::pin(
        f.collector
            .send_private_approval_card(card.clone(), &cancel),
    );
    tokio::pin!(wait);
    loop {
        tokio::select! {r=&mut operation=>panic!("send escaped original response: {r:?}"),r=&mut wait=>{r.unwrap();break;},request=f.fake.next()=>respond(request,&mut f.peer).await}
    }
    assert_eq!(f.collector.close().await, Err(Error::Busy));
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
    let status = f
        .collector
        .private_approval_delivery_status()
        .await
        .unwrap();
    assert_eq!(status.accepted, 1);
    assert_eq!(status.writes, 2);
    assert_eq!(status.stage, PrivateApprovalDeliveryStage::Ready);
    assert_eq!(
        f.collector
            .resume_private_approval_delivery_custody(&cancel)
            .await
            .unwrap()
            .state,
        PrivateApprovalDeliveryState::Uncertain
    );
    assert_eq!(f.peer.shares, 1);
    assert!(f.peer.events.is_empty());
    assert!(
        f.collector
            .send_private_approval_card(card, &CancellationToken::new())
            .await
            .is_err()
    );
    f.fake.no_request().await;
    f.close().await;

    // No actual typed response means WritePossible, not fabricated NotSent.
    for room_write in [false, true] {
        let mut f = Fixture::new().await;
        f.enroll().await.unwrap();
        let card = f.card(1, true).await;
        let cancel = CancellationToken::new();
        let mut operation = Box::pin(f.collector.send_private_approval_card(card, &cancel));
        loop {
            tokio::select! {r=&mut operation=>panic!("send finished before held request: {r:?}"),request=f.fake.next()=>{
                if request.method=="PUT"&&(request.target.contains("/send/")==room_write){drop(request);break;}
                respond(request,&mut f.peer).await;
            }}
        }
        assert!(operation.await.is_err());
        let status = f
            .collector
            .private_approval_delivery_status()
            .await
            .unwrap();
        assert_eq!(status.stage, PrivateApprovalDeliveryStage::WritePossible);
        assert_eq!(status.accepted, usize::from(room_write));
        f.collector.close().await.unwrap();
        let reopened = ApprovalCollector::new(
            config(&f.base, &f.fake, Some(f.peer.anchor())),
            f.base.store.clone(),
        )
        .unwrap();
        let history = reopened
            .resume_private_approval_delivery_custody(&CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(history.state, PrivateApprovalDeliveryState::Uncertain);
        f.fake.no_request().await;
        reopened.close().await.unwrap();
        common::shutdown_domain(&f.base.store, "unknown-card-reopen").await;
        f.fake.close().await;
    }
}
#[tokio::test]
async fn native_private_approval_retained_close() {
    for failure in [false, true] {
        let mut f = Fixture::new().await;
        f.enroll().await.unwrap();
        if failure {
            f.collector
                .inner
                .owner
                .lock()
                .await
                .as_ref()
                .unwrap()
                .approval_close_fault()
                .await;
        }
        let trace = Trace::new("private card original shutdown", None, None);
        let mut held = trace.hold(ObservedPhase::RuntimeDropStarted);
        let mut closing = Box::pin(observed(trace.clone(), f.collector.close()));
        tokio::select! {_ =held.reached()=>{},r=&mut closing=>panic!("original close escaped held SDK: {r:?}")}
        assert!(matches!(
            Owner::open_existing(&f.collector.inner.config).await,
            Err(Error::Busy)
        ));
        drop(closing);
        held.release();
        let expected = if failure {
            Err(Error::OutcomeUnknown)
        } else {
            Ok(())
        };
        assert_eq!(f.collector.close().await, expected);
        assert_eq!(f.collector.close().await, expected);
        assert!(trace.has(ObservedPhase::LockDropped));
        assert!(
            f.collector
                .observe(&CancellationToken::new())
                .await
                .is_err()
        );
        assert!(
            f.collector
                .intake(
                    HostApprovalPlan::new(vec![]).unwrap(),
                    &CancellationToken::new()
                )
                .await
                .is_err()
        );
        assert!(
            f.collector
                .enroll_fresh_account(&CancellationToken::new())
                .await
                .is_err()
        );
        f.fake.no_request().await;
        let reopened = Owner::open_existing(&f.collector.inner.config)
            .await
            .unwrap();
        reopened.close().await.unwrap();
        common::shutdown_domain(&f.base.store, "private-card-close-result").await;
        f.fake.close().await;
    }
}
#[tokio::test]
async fn native_private_approval_send_cancellation_and_loss_retains_intake() {
    let mut f = Fixture::new().await;
    f.enroll().await.unwrap();
    let card = f.card(1, true).await;
    f.collector
        .inner
        .owner
        .lock()
        .await
        .as_ref()
        .unwrap()
        .apply_fault()
        .await;
    let cancel = CancellationToken::new();
    let intake = drive(
        f.collector.intake(
            HostApprovalPlan::new(vec![card.target().request_id.clone()]).unwrap(),
            &cancel,
        ),
        &mut f.fake,
        &mut f.peer,
    )
    .await;
    assert_eq!(intake, Err(Error::OutcomeUnknown));
    // Cancel only after the original SDK has returned its interrupted-apply
    // result. Cancelling while the fixture is preparing HTTP could correctly
    // stop before Apply and would not prove retained SDK mutation custody.
    cancel.cancel();
    // The real sync entered SDK Applying; the injected lost apply result is
    // explicitly an interrupted-custody model, not fabricated crypto proof.
    let sent = drive(
        f.collector
            .send_private_approval_card(card, &CancellationToken::new()),
        &mut f.fake,
        &mut f.peer,
    )
    .await;
    assert!(sent.is_err());
    assert_eq!(f.peer.shares, 0);
    assert!(f.peer.events.is_empty());
    f.fake.no_request().await;
    f.close().await;
}
