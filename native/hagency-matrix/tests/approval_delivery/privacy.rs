use super::*;
#[tokio::test]
async fn native_private_approval_current_private_authority() {
    for variant in ["who", "device", "owner", "group", "encryption", "keys"] {
        let mut f = Fixture::new().await;
        f.enroll().await.unwrap();
        let card = f.card(1, false).await;
        let result=drive_with(f.collector.send_private_approval_card(card,&CancellationToken::new()),&mut f.fake,&mut f.peer,|r,_,reply|{
            if r.target.ends_with("/whoami"){
                if variant=="who"{reply.1["user_id"]=json!("@stranger:example.test");}
                if variant=="device"{reply.1["device_id"]=json!("OTHER");}
            }
            if r.target.ends_with("/state"){
                if variant=="owner"{reply.1[0]["state_key"]=json!("@stranger:example.test");}
                if variant=="group"{reply.1.as_array_mut().unwrap().push(json!({"type":"m.room.member","state_key":"@third:example.test","content":{"membership":"join"}}));}
                if variant=="encryption"{reply.1.as_array_mut().unwrap().pop();}
            }
            if variant=="keys"&&r.target.ends_with("/keys/query"){reply.1["master_keys"][crypto::HUMAN]=json!({});}
        }).await;
        assert!(result.is_err(), "{variant}");
        assert_eq!(f.peer.shares, 0);
        assert!(f.peer.events.is_empty());
        f.fake.no_request().await;
        assert!(
            f.base.available().await,
            "approval failure must not fence Agent authority"
        );
        f.close().await;
    }
    // Actual SDK Possible has committed, but its original response is held.
    // The current original-domain card changes during that exact await.
    for change in ["binding", "expiry", "task"] {
        let mut f = Fixture::new().await;
        f.enroll().await.unwrap();
        let original = f.card(1, true).await;
        let card = if change == "expiry" {
            Arc::new(
                f.base
                    .store
                    .private_approval_card(original.target().request_id.clone(), now() + 1000)
                    .await
                    .unwrap(),
            )
        } else {
            original
        };
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
                phase: 3,
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
            tokio::select! {r=&mut operation=>panic!("original send escaped held Possible: {r:?}"),r=&mut wait=>{r.unwrap();break;},request=f.fake.next()=>respond(request,&mut f.peer).await}
        }
        match change {
            "binding" => {
                f.base
                    .store
                    .observe_approval_room(ApprovalRoomObservation {
                        engagement_id: card.target().authority.engagement_id.clone(),
                        registration_generation: 1,
                        generation: 2,
                        room_id: ROOM.into(),
                        device_id: "NEW_BOT".into(),
                        joined: std::collections::BTreeSet::from([
                            BOT.into(),
                            crypto::HUMAN.into(),
                        ]),
                        invite_only: true,
                        encrypted: true,
                        available: true,
                    })
                    .await
                    .unwrap();
            }
            "expiry" => tokio::time::sleep(Duration::from_millis(1100)).await,
            "task" => {
                f.base
                    .store
                    .revoke(
                        "fixture-revoke".into(),
                        card.target().authority.engagement_id.clone(),
                    )
                    .await
                    .unwrap();
            }
            _ => unreachable!(),
        }
        release.send(()).unwrap();
        let result = drive(operation, &mut f.fake, &mut f.peer).await;
        assert!(result.is_err(), "{change}");
        assert_eq!(f.peer.shares, 0);
        assert!(f.peer.events.is_empty());
        f.fake.no_request().await;
        let close = f.collector.close().await;
        assert!(
            close.is_ok() || change == "task",
            "actual close {change}: {close:?}"
        );
        common::shutdown_domain(&f.base.store, "private-card-mutation").await;
        f.fake.close().await;
    }
}
