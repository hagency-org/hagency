//! Explicit offline fixture Applied is NOT physical factory completion proof.
use super::*;

async fn activate_fixture(f: &common::Fixture) {
    let id = format!("provision_{}", admitted_id(f));
    let sql = rusqlite::Connection::open(f.root.path().join("domain/domain.sqlite3")).unwrap();
    let (state, fence): (String, u64) = sql
        .query_row("SELECT state,fence FROM effects WHERE id=?1", [&id], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!(state, "started");
    drop(sql);
    f.store
        .observe_effect(
            id,
            fence,
            hagency_store::EffectOutcome::Applied {
                receipt: "explicit offline activation, not physical factory proof".into(),
            },
        )
        .await
        .unwrap();
}
async fn active_response(
    request: common::Request,
    account: &crate::ProvisionedTokenAccount,
    peer: &mut crypto::Peer,
    change: &mut impl FnMut(&common::Request, &mut (u16, Value)),
) {
    if request.target.starts_with("/_matrix/client/v3/sync?") {
        assert_eq!(request.method, "GET");
        assert_eq!(
            request.headers["authorization"],
            format!("Bearer {ACCOUNT_TOKEN}")
        );
        let mut reply = (
            200,
            json!({"next_batch":format!("active-{}",request.target.len()),"rooms":{"join":{}},"to_device":{"events":[]}}),
        );
        change(&request, &mut reply);
        request.json(reply.0, reply.1);
    } else {
        respond(request, account, peer, change).await;
    }
}
async fn active_with(
    account: &crate::ProvisionedTokenAccount,
    fake: &mut common::Fake,
    peer: &mut crypto::Peer,
    mut change: impl FnMut(&common::Request, &mut (u16, Value)),
) -> Result<Collector, Error> {
    let cancel = CancellationToken::new();
    let operation = account.active_collector(&cancel);
    tokio::pin!(operation);
    for _ in 0..128 {
        tokio::select! {
            result=&mut operation=>return result,
            request=fake.next()=>active_response(request,account,peer,&mut change).await,
        }
    }
    panic!("original finite Active handoff failed to settle")
}
async fn finish_owned(
    account: &crate::ProvisionedTokenAccount,
    enrolled: &Collector,
    fake: &mut common::Fake,
    peer: &mut crypto::Peer,
) -> Result<(), Error> {
    let done = enrolled.inner.busy.clone().acquire_owned();
    tokio::pin!(done);
    for _ in 0..128 {
        tokio::select! {
            permit=&mut done=>{drop(permit.unwrap());return account.observed_active_handoff().expect("original Active result retained");},
            request=fake.next()=>active_response(request,account,peer,&mut |_,_| {}).await,
        }
    }
    panic!("original Active custody did not settle")
}
async fn close_all(
    f: common::Fixture,
    fake: common::Fake,
    c: Collector,
    account: &crate::ProvisionedTokenAccount,
) {
    account.close_enrollment_sdk().await.unwrap();
    // Fixture registration rotation can retire the unrelated reception host.
    // Close only its actual SDK; do not fabricate a current Domain close receipt.
    if let Some(owner) = c.inner.owner.lock().await.take() {
        owner.close().await.unwrap();
    }
    f.store.shutdown().await.unwrap();
    fake.close().await;
}
#[tokio::test]
async fn native_provisioning_sdk_active_handoff() {
    let (f, mut fake, c, account, mut peer) = observed().await;
    let enrolled = drive(&account, &mut fake, &mut peer).await.unwrap();
    let retained = Arc::downgrade(&enrolled.inner);
    let identity = enrolled
        .inner
        .owner
        .lock()
        .await
        .as_ref()
        .unwrap()
        .owner_identity();
    let binding = enrolled.inner.config.binding().unwrap();
    let original_public = peer.query.clone();
    activate_fixture(&f).await;
    assert!(
        f.store
            .matrix_transport_state(admitted_id(&f))
            .await
            .unwrap()
            .is_none()
    );
    for _ in 0..2 {
        let active = active_with(&account, &mut fake, &mut peer, |_, _| {})
            .await
            .unwrap();
        assert!(Arc::ptr_eq(&enrolled.inner, &active.inner));
        let owner = active.inner.owner.lock().await;
        assert!(owner.as_ref().unwrap().same_owner(&identity));
        assert!(matches!(
            owner.as_ref().unwrap().enrollment(Command::Status).await,
            Ok(View::Complete)
        ));
        drop(owner);
        assert_eq!(active.inner.config.binding().unwrap(), binding);
        let transport = f
            .store
            .matrix_transport_state(admitted_id(&f))
            .await
            .unwrap()
            .unwrap();
        assert!(transport.available);
        assert!(transport.observation == active.inner.config.identity.transport);
        for room in rooms() {
            assert!(
                f.store
                    .matrix_room_state(admitted_id(&f), room.room_id)
                    .await
                    .unwrap()
                    .unwrap()
                    .available
            );
        }
        assert_eq!(peer.query, original_public);
        assert_eq!(peer.writes.len(), 5);
        assert_eq!(peer.claims, 1);
        assert_eq!(route_rows(&f), 0, "handoff is not session-route authority");
    }
    let before = fake.requests();
    assert!(
        drive(&account, &mut fake, &mut peer).await.is_err(),
        "Active job cannot move backwards"
    );
    assert_eq!(fake.requests(), before);
    assert_eq!(account.observed_active_handoff(), Some(Ok(())));
    close_all(f, fake, c, &account).await;
    drop(enrolled);
    drop(account);
    assert!(
        retained.upgrade().is_none(),
        "settled Active job has no owner/task Arc cycle"
    );
}
#[tokio::test]
async fn native_provisioning_sdk_active_refusals() {
    for case in 0..8 {
        let (f, mut fake, c, account, mut peer) = observed().await;
        let enrolled = drive(&account, &mut fake, &mut peer).await.unwrap();
        let identity = enrolled
            .inner
            .owner
            .lock()
            .await
            .as_ref()
            .unwrap()
            .owner_identity();
        if case != 0 {
            activate_fixture(&f).await;
        }
        match case {
            1 => {
                let mut reg = common::domain::registration();
                reg.generation += 1;
                f.store.register(reg).await.unwrap();
            }
            2 => {
                f.store
                    .revoke("revoked_active_sdk".into(), admitted_id(&f))
                    .await
                    .unwrap();
            }
            6 => {
                account.close_enrollment_sdk().await.unwrap();
            }
            7 => {
                enrolled
                    .inner
                    .owner
                    .lock()
                    .await
                    .take()
                    .unwrap()
                    .close()
                    .await
                    .unwrap();
            }
            _ => {}
        }
        let result = active_with(&account, &mut fake, &mut peer, |request, reply| {
            if request.target.ends_with("/state") {
                if case == 3 && request.target.contains("new_agent_dm") {
                    reply
                        .1
                        .as_array_mut()
                        .unwrap()
                        .push(member(&representative()));
                }
                if case == 4 && request.target.contains("project_provision") {
                    reply.1[4]["content"]["users"][OWNER] = json!(-1);
                }
            }
            if case == 5 && request.target.ends_with("/keys/query") {
                reply.1["device_keys"][OWNER][crypto::HUMAN_DEVICE]["signatures"] = json!({});
            }
        })
        .await;
        assert!(result.is_err(), "Active refusal {case}");
        if let Some(owner) = enrolled.inner.owner.lock().await.as_ref() {
            assert!(owner.same_owner(&identity));
        }
        let before = fake.requests();
        assert!(
            active_with(&account, &mut fake, &mut peer, |_, _| {})
                .await
                .is_err()
        );
        assert_eq!(
            fake.requests(),
            before,
            "failed/closed handoff cannot rearm {case}"
        );
        assert!(drive(&account, &mut fake, &mut peer).await.is_err());
        assert_eq!(fake.requests(), before);
        assert_eq!(peer.writes.len(), 5);
        assert_eq!(peer.claims, 1);
        assert_eq!(route_rows(&f), 0);
        if case == 5 {
            assert!(
                !f.store
                    .matrix_transport_state(admitted_id(&f))
                    .await
                    .unwrap()
                    .unwrap()
                    .available,
                "post-collect verification failure fences original positive transport"
            );
        }
        fake.quiesced(before, &common::limits()).await;
        close_all(f, fake, c, &account).await;
    }
}
#[tokio::test]
async fn native_provisioning_sdk_active_custody() {
    for revoked in [false, true] {
        let (f, mut fake, c, account, mut peer) = observed().await;
        let enrolled = drive(&account, &mut fake, &mut peer).await.unwrap();
        let identity = enrolled
            .inner
            .owner
            .lock()
            .await
            .as_ref()
            .unwrap()
            .owner_identity();
        activate_fixture(&f).await;
        let original = account.clone();
        let waiter =
            tokio::spawn(async move { original.active_collector(&CancellationToken::new()).await });
        let held = loop {
            let request = fake.next().await;
            if request.target.contains("new_agent_dm") && request.target.ends_with("/state") {
                break request;
            }
            active_response(request, &account, &mut peer, &mut |_, _| {}).await;
        };
        assert!(matches!(
            account.active_collector(&CancellationToken::new()).await,
            Err(Error::Busy)
        ));
        waiter.abort();
        assert!(matches!(waiter.await,Err(error) if error.is_cancelled()));
        if revoked {
            f.store
                .revoke("revoked_held_active_read".into(), admitted_id(&f))
                .await
                .unwrap();
        }
        held.json(200, state(&account, false));
        let result = finish_owned(&account, &enrolled, &mut fake, &mut peer).await;
        assert_eq!(result.is_ok(), !revoked);
        assert!(
            enrolled
                .inner
                .owner
                .lock()
                .await
                .as_ref()
                .unwrap()
                .same_owner(&identity)
        );
        assert_eq!(peer.writes.len(), 5);
        assert_eq!(peer.claims, 1);
        assert_eq!(route_rows(&f), 0);
        if revoked {
            let before = fake.requests();
            assert!(
                active_with(&account, &mut fake, &mut peer, |_, _| {})
                    .await
                    .is_err()
            );
            assert_eq!(fake.requests(), before);
        } else {
            assert!(
                active_with(&account, &mut fake, &mut peer, |_, _| {})
                    .await
                    .is_ok()
            );
        }
        close_all(f, fake, c, &account).await;
    }
}
