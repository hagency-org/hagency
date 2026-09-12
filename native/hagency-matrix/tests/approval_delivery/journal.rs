use super::*;
async fn complete_with_lost_reply(f: &mut Fixture, card: Arc<PrivateApprovalCard>) {
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
            phase: 6,
            reached,
            release: resume,
            lose: true,
        }))
        .await
        .unwrap();
    let cancel = CancellationToken::new();
    let mut operation = Box::pin(f.collector.send_private_approval_card(card, &cancel));
    tokio::pin!(wait);
    loop {
        tokio::select! {r=&mut operation=>panic!("send escaped original complete reply: {r:?}"),r=&mut wait=>{r.unwrap();break;},request=f.fake.next()=>respond(request,&mut f.peer).await}
    }
    release.send(()).unwrap();
    assert_eq!(operation.await, Err(Error::OutcomeUnknown));
    assert_eq!(f.peer.events.len(), 1);
}
#[tokio::test]
async fn native_private_approval_historical_restart_and_capacity() {
    let mut f = Fixture::new().await;
    f.enroll().await.unwrap();
    let card = f.card(1, true).await;
    complete_with_lost_reply(&mut f, card.clone()).await;
    // Current authority retires; the actual stored response remains historical.
    f.base
        .store
        .observe_approval_room(ApprovalRoomObservation {
            engagement_id: card.target().authority.engagement_id.clone(),
            registration_generation: 1,
            generation: 2,
            room_id: ROOM.into(),
            device_id: "NEW_BOT".into(),
            joined: std::collections::BTreeSet::from([BOT.into(), crypto::HUMAN.into()]),
            invite_only: true,
            encrypted: true,
            available: true,
        })
        .await
        .unwrap();
    assert!(
        f.base
            .store
            .check_private_approval_card(card)
            .await
            .is_err()
    );
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
    assert_eq!(history.state, PrivateApprovalDeliveryState::Accepted);
    assert!(history.replayed);
    assert_eq!(
        reopened
            .private_approval_delivery_status()
            .await
            .unwrap()
            .receipts,
        1
    );
    f.fake.no_request().await;
    reopened.close().await.unwrap();
    common::shutdown_domain(&f.base.store, "card-complete-reopen").await;
    f.fake.close().await;

    for variant in 1..=8 {
        let mut f = Fixture::new().await;
        f.enroll().await.unwrap();
        let card = f.card(1, false).await;
        complete_with_lost_reply(&mut f, card).await;
        f.collector
            .inner
            .owner
            .lock()
            .await
            .as_ref()
            .unwrap()
            .approval_delivery_handle()
            .command(Command::Corrupt(variant))
            .await
            .unwrap();
        f.collector.close().await.unwrap();
        assert!(
            Owner::open_existing(&f.collector.inner.config)
                .await
                .is_err(),
            "protected corruption {variant}"
        );
        f.fake.no_request().await;
        common::shutdown_domain(&f.base.store, "card-corrupt-reopen").await;
        f.fake.close().await;
    }
    let mut f = Fixture::new().await;
    f.enroll().await.unwrap();
    let original = f.card(1, false).await;
    f.send(original.clone()).await.unwrap();
    let changed = Arc::new(
        f.base
            .store
            .private_approval_card(original.target().request_id.clone(), now() + 20_000)
            .await
            .unwrap(),
    );
    assert_eq!(
        f.collector
            .send_private_approval_card(changed, &CancellationToken::new())
            .await,
        Err(Error::Conflict)
    );
    f.fake.no_request().await;
    f.close().await;

    let mut f = Fixture::new().await;
    f.enroll().await.unwrap();
    let original = f.card(1, true).await;
    f.send(original).await.unwrap();
    let next = f.card(2, true).await;
    let handle = f
        .collector
        .inner
        .owner
        .lock()
        .await
        .as_ref()
        .unwrap()
        .approval_delivery_handle();
    let before = handle
        .command(Command::Read)
        .await
        .unwrap()
        .receipts
        .remove(0);
    handle.command(Command::Corrupt(100)).await.unwrap();
    assert_eq!(
        f.collector
            .private_approval_delivery_status()
            .await
            .unwrap()
            .receipts,
        64
    );
    assert_eq!(
        f.collector
            .send_private_approval_card(next, &CancellationToken::new())
            .await,
        Err(Error::Capacity)
    );
    let after = handle.command(Command::Read).await.unwrap().receipts;
    assert_eq!(after.len(), 64);
    assert_eq!(after[0].request_id, before.request_id);
    assert_eq!(after[0].attempt_digest, before.attempt_digest);
    f.fake.no_request().await;
    f.close().await;

    // Actual legal metadata expands beyond the unchanged encrypted event cap.
    let mut f = Fixture::new().await;
    f.enroll().await.unwrap();
    let input = HostApprovalRequest {
        context_id: "context".into(),
        upstream_id: ApprovalRpcId::Number(1),
        item_id: "item1".into(),
        method: "unknown/requestApproval".into(),
        params: json!({"threadId":"thread","turnId":"turn","itemId":"item1","opaque":"x".repeat(23_000)}),
        expires_at: now() + 50_000,
    };
    let a = f
        .base
        .store
        .request_owner_approval(f.cap.clone(), input)
        .await
        .unwrap();
    let card = Arc::new(
        f.base
            .store
            .private_approval_card(a.id, now() + 40_000)
            .await
            .unwrap(),
    );
    assert!(serde_json::to_vec(card.content()).unwrap().len() <= state::MAX_CARD);
    assert_eq!(f.send(card).await, Err(Error::Capacity));
    assert_eq!(f.peer.shares, 0);
    assert!(f.peer.events.is_empty());
    f.fake.no_request().await;
    f.close().await;
}
