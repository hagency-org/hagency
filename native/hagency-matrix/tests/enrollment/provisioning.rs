//! Exercise the opaque account from actual inline approval, not a seeded SDK.
use super::*;
use crate::enrollment::crypto_fixture as crypto;
use crate::enrollment::state::View;
use crate::sdk::enrollment::Command;
use std::sync::Arc;
#[path = "active_handoff.rs"]
mod active_handoff;

const DM: &str = "!new_agent_dm:example.test";
const ACCOUNT_TOKEN: &str = "actual-returned-synthetic-token";

fn rooms() -> Vec<HostRoom> {
    vec![
        HostRoom {
            room_id: PROJECT.into(),
            generation: 1,
            privacy: RoomPrivacy::Group {},
        },
        HostRoom {
            room_id: DM.into(),
            generation: 1,
            privacy: RoomPrivacy::Direct {
                human_mxid: OWNER.into(),
            },
        },
    ]
}
async fn observed() -> (
    common::Fixture,
    common::Fake,
    Collector,
    Arc<crate::ProvisionedTokenAccount>,
    crypto::Peer,
) {
    let (f, mut fake, c) = ready_inline_account().await;
    let (result, ()) = common::scripted(c.intake(plan(), &CancellationToken::new()), async {
        inline_input(&mut fake, "account_enrollment").await;
        complete_account_step(&f, &mut fake).await;
    })
    .await;
    result.unwrap();
    let account = c
        .inner
        .config
        .provisioning
        .as_ref()
        .unwrap()
        .observed_account_handle(&admitted_id(&f));
    let peer = crypto::Peer::for_sender(account.sender_mxid(), account.device_id()).await;
    (f, fake, c, account, peer)
}
fn state(account: &crate::ProvisionedTokenAccount, project: bool) -> Value {
    let mut result = common::state();
    result[0]["state_key"] = json!(account.sender_mxid());
    if project {
        result
            .as_array_mut()
            .unwrap()
            .push(power_levels(json!({OWNER:100})));
        result.as_array_mut().unwrap().push(json!({
            "type":"com.hagency.project.binding.v1", "state_key":"",
            "content":{"v":1,"purpose":"project","authVersion":1,"fleetId":fleet_id(),"projectId":"project_provision","ownerMxid":OWNER}
        }));
    }
    result
}
async fn respond(
    request: common::Request,
    account: &crate::ProvisionedTokenAccount,
    peer: &mut crypto::Peer,
    change: &mut impl FnMut(&common::Request, &mut (u16, Value)),
) {
    assert_eq!(
        request.headers.get("authorization"),
        Some(&format!("Bearer {ACCOUNT_TOKEN}"))
    );
    let mut reply = if request.target == "/_matrix/client/v3/account/whoami" {
        (
            200,
            json!({"user_id":account.sender_mxid(),"device_id":account.device_id(),"is_guest":false}),
        )
    } else if request.target.ends_with("/state") {
        assert!(
            request.target.contains("project_provision") || request.target.contains("new_agent_dm")
        );
        (
            200,
            state(account, request.target.contains("project_provision")),
        )
    } else {
        let body = if request.body.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&request.body).unwrap()
        };
        peer.protocol(&request.method, &request.target, &body)
            .await
            .expect("fixed actual enrollment protocol")
    };
    change(&request, &mut reply);
    request.json(reply.0, reply.1);
}
async fn drive_with(
    account: &crate::ProvisionedTokenAccount,
    fake: &mut common::Fake,
    peer: &mut crypto::Peer,
    mut change: impl FnMut(&common::Request, &mut (u16, Value)),
) -> Result<Collector, Error> {
    let cancel = CancellationToken::new();
    let operation = account.enroll_before_activation(
        1,
        [44; 32],
        rooms(),
        vec![(OWNER.into(), peer.anchor())],
        &cancel,
    );
    tokio::pin!(operation);
    for _ in 0..128 {
        tokio::select! {
            result=&mut operation=>return result,
            request=fake.next()=>respond(request,account,peer,&mut change).await,
        }
    }
    panic!("original bounded enrollment did not settle")
}
async fn drive(
    account: &crate::ProvisionedTokenAccount,
    fake: &mut common::Fake,
    peer: &mut crypto::Peer,
) -> Result<Collector, Error> {
    drive_with(account, fake, peer, |_, _| {}).await
}
async fn close(
    f: common::Fixture,
    fake: common::Fake,
    c: Collector,
    enrolled: Option<Collector>,
    account: &crate::ProvisionedTokenAccount,
) {
    account.close_enrollment_sdk().await.unwrap();
    drop(enrolled);
    c.close().await.unwrap();
    f.store.shutdown().await.unwrap();
    fake.close().await;
}
async fn decrypt_original_sessions(
    account: &crate::ProvisionedTokenAccount,
    enrolled: &Collector,
    peer: &mut crypto::Peer,
) {
    decrypt_in_room(account, enrolled, peer, DM).await;
}
pub(super) async fn decrypt_in_room(
    account: &crate::ProvisionedTokenAccount,
    enrolled: &Collector,
    peer: &mut crypto::Peer,
    room_id: &str,
) {
    use matrix_sdk_crypto::{CollectStrategy, EncryptionSettings, OlmMachine, store::CryptoStore};
    use matrix_sdk_sqlite::{SqliteCryptoStore, SqliteStoreConfig};
    account.close_enrollment_sdk().await.unwrap();
    let config = &enrolled.inner.config;
    let store = SqliteCryptoStore::open_with_config(
        &SqliteStoreConfig::new(&config.root)
            .key(Some(&config.key))
            .pool_max_size(2),
    )
    .await
    .unwrap();
    assert!(
        store.load_account().await.unwrap().is_some(),
        "open only the already-created original SDK account"
    );
    let user: ruma::OwnedUserId = account.sender_mxid().try_into().unwrap();
    let device: ruma::OwnedDeviceId = account.device_id().into();
    let machine = OlmMachine::with_store(&user, &device, store.clone(), None)
        .await
        .unwrap();
    let curve = peer.query["device_keys"][OWNER][crypto::HUMAN_DEVICE]["keys"]
        [format!("curve25519:{}", crypto::HUMAN_DEVICE)]
    .as_str()
    .unwrap();
    assert_eq!(store.get_sessions(curve).await.unwrap().unwrap().len(), 1);
    let room: ruma::OwnedRoomId = room_id.try_into().unwrap();
    let human: ruma::OwnedUserId = OWNER.try_into().unwrap();
    let shares = machine
        .share_room_key(
            &room,
            [human.as_ref()].into_iter(),
            EncryptionSettings {
                sharing_strategy: CollectStrategy::OnlyTrustedDevices,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(shares.len(), 1);
    for share in shares {
        // Recipient-side verification uses only the actual captured public
        // signing upload. It cannot modify service keys, anchors or sessions.
        peer.share(json!({"messages":share.messages})).await;
        machine
            .mark_request_as_sent(
                &share.txn_id,
                &ruma::api::client::to_device::send_event_to_device::v3::Response::new(),
            )
            .await
            .unwrap();
    }
    let content = json!({"msgtype":"m.text","body":"original factory enrollment session"});
    let raw = ruma::serde::Raw::from_json_string(content.to_string()).unwrap();
    let event = machine
        .encrypt_room_event_raw(&room, "m.room.message", &raw)
        .await
        .unwrap();
    let plain = peer
        .decrypt(serde_json::to_value(event.content).unwrap(), &room)
        .await;
    assert_eq!(plain["content"], content);
    drop(machine);
    tokio::task::spawn_blocking(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async move {
            store.close().await.unwrap();
            drop(store);
        });
        drop(runtime);
    })
    .await
    .unwrap();
    // Read the same protected Complete with its original identity/session
    // associations. This inspector is not rearming the closed factory job.
    let inspector = crate::sdk::Owner::open_existing(config).await.unwrap();
    assert!(matches!(
        inspector.enrollment(Command::Status).await,
        Ok(View::Complete)
    ));
    inspector.close().await.unwrap();
    assert_eq!(peer.writes.len(), 5);
    assert_eq!(peer.claims, 1);
}
#[tokio::test]
async fn native_provisioning_account_enrollment() {
    let (f, mut fake, c, account, mut peer) = observed().await;
    assert!(!account_root(&f).join("sdk").exists());
    let enrolled = drive(&account, &mut fake, &mut peer).await.unwrap();
    assert_eq!(peer.writes.len(), 5);
    assert_eq!(peer.claims, 1);
    let owner = enrolled.inner.owner.lock().await;
    assert!(matches!(
        owner.as_ref().unwrap().enrollment(Command::Status).await,
        Ok(View::Complete)
    ));
    drop(owner);
    assert_account_only(&f, &c);
    assert!(matches!(
        f.store.matrix_transport_state(admitted_id(&f)).await,
        Err(hagency_store::Error::RunnerAuthority)
    ));
    let sql = rusqlite::Connection::open(f.root.path().join("domain/domain.sqlite3")).unwrap();
    let count: u64 = sql
        .query_row(
            "SELECT COUNT(*) FROM matrix_transports WHERE engagement_id=?1",
            [admitted_id(&f)],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
    drop(sql);
    {
        let cancel = CancellationToken::new();
        let normal = enrolled.enroll_fresh_account(&cancel);
        tokio::pin!(normal);
        let result = loop {
            tokio::select! {
                result=&mut normal=>break result,
                request=fake.next()=>respond(request,&account,&mut peer,&mut |_,_| {}).await,
            }
        };
        assert!(
            result.is_err(),
            "normal enrollment still refuses Reserved scope"
        );
        assert_eq!(peer.writes.len(), 5);
    }
    decrypt_original_sessions(&account, &enrolled, &mut peer).await;
    close(f, fake, c, Some(enrolled), &account).await;
}
#[tokio::test]
async fn native_provisioning_account_enrollment_refusals() {
    for variant in 0..10 {
        let (f, mut fake, c, account, mut peer) = observed().await;
        if variant < 3 {
            let mut wrong = rooms();
            match variant {
                0 => wrong[1].room_id = PRIVATE.into(),
                1 => wrong[0].room_id = RECEPTION.into(),
                2 => {
                    wrong[1].privacy = RoomPrivacy::Direct {
                        human_mxid: representative(),
                    }
                }
                _ => unreachable!(),
            }
            assert!(matches!(
                account
                    .enroll_before_activation(
                        1,
                        [44; 32],
                        wrong,
                        vec![(OWNER.into(), peer.anchor())],
                        &CancellationToken::new()
                    )
                    .await,
                Err(Error::Config)
            ));
            assert_eq!(fake.requests(), 12);
        } else {
            if variant == 9 {
                f.store
                    .revoke("before_enrollment".into(), admitted_id(&f))
                    .await
                    .unwrap();
            }
            let result = drive_with(&account, &mut fake, &mut peer, |request, reply| {
                if variant == 3 && request.target.ends_with("/account/whoami") {
                    reply.1["device_id"] = json!("wrong_device");
                }
                if request.target.ends_with("/state") {
                    if request.target.contains("new_agent_dm") {
                        match variant {
                            4 => reply
                                .1
                                .as_array_mut()
                                .unwrap()
                                .push(member(&representative())),
                            5 => reply.1[3]["content"]["algorithm"] = json!("unsupported"),
                            6 => reply.1[1]["content"]["membership"] = json!("invite"),
                            _ => {}
                        }
                    } else {
                        if variant == 7 {
                            reply.1[5]["content"]["ownerMxid"] = json!(representative());
                        }
                        if variant == 8 {
                            reply.1[4]["content"]["users"][OWNER] = json!(-1);
                        }
                    }
                }
            })
            .await;
            assert!(result.is_err(), "scope variant {variant}");
            assert!(peer.writes.is_empty());
            assert_eq!(peer.claims, 0);
            assert!(
                !account_root(&f).join("sdk").exists(),
                "initial scope refusal created no SDK"
            );
        }
        assert_eq!(route_rows(&f), 0);
        close(f, fake, c, None, &account).await;
    }
}
#[tokio::test]
async fn native_provisioning_account_enrollment_replay() {
    let (f, mut fake, c, account, mut peer) = observed().await;
    let enrolled = drive(&account, &mut fake, &mut peer).await.unwrap();
    let original = Arc::downgrade(&enrolled.inner);
    let again = drive(&account, &mut fake, &mut peer).await.unwrap();
    assert!(Arc::ptr_eq(&enrolled.inner, &again.inner));
    assert_eq!(peer.writes.len(), 5);
    assert_eq!(peer.claims, 1);
    let before = fake.requests();
    assert!(matches!(
        account
            .enroll_before_activation(
                2,
                [44; 32],
                rooms(),
                vec![(OWNER.into(), peer.anchor())],
                &CancellationToken::new()
            )
            .await,
        Err(Error::Conflict)
    ));
    assert!(matches!(
        account
            .enroll_before_activation(
                1,
                [45; 32],
                rooms(),
                vec![(OWNER.into(), peer.anchor())],
                &CancellationToken::new()
            )
            .await,
        Err(Error::Conflict)
    ));
    assert_eq!(fake.requests(), before);
    let refused = drive_with(&account, &mut fake, &mut peer, |request, reply| {
        if request.target.contains("new_agent_dm") {
            reply.1[1]["content"]["membership"] = json!("leave");
        }
    })
    .await;
    assert!(refused.is_err());
    assert_eq!(peer.writes.len(), 5);
    assert_eq!(peer.claims, 1);
    assert_eq!(
        effect_row(&f),
        Some(("provision".into(), "uncertain".into()))
    );
    assert_eq!(route_rows(&f), 0);
    let before = fake.requests();
    assert!(drive(&account, &mut fake, &mut peer).await.is_err());
    assert_eq!(
        fake.requests(),
        before,
        "failed current readiness cannot rearm"
    );
    drop(again);
    account.close_enrollment_sdk().await.unwrap();
    account.close_enrollment_sdk().await.unwrap();
    assert!(matches!(
        drive(&account, &mut fake, &mut peer).await,
        Err(Error::Storage)
    ));
    drop(enrolled);
    drop(account);
    c.close().await.unwrap();
    drop(c);
    assert!(
        original.upgrade().is_none(),
        "settled account owner has no Arc cycle"
    );
    f.store.shutdown().await.unwrap();
    fake.close().await;
}
#[tokio::test]
async fn native_provisioning_account_enrollment_custody() {
    for lost in [false, true] {
        let (f, mut fake, c, account, mut peer) = observed().await;
        let owner = account.clone();
        let anchor = peer.anchor();
        let waiter = tokio::spawn(async move {
            owner
                .enroll_before_activation(
                    1,
                    [44; 32],
                    rooms(),
                    vec![(OWNER.into(), anchor)],
                    &CancellationToken::new(),
                )
                .await
        });
        let mut held = None;
        for _ in 0..128 {
            let request = fake.next().await;
            if request.target == "/_matrix/client/v3/keys/upload" {
                held = Some(request);
                break;
            }
            respond(request, &account, &mut peer, &mut |_, _| {}).await;
        }
        let request = held.expect("actual first SDK key upload admitted");
        assert!(matches!(
            account
                .enroll_before_activation(
                    1,
                    [44; 32],
                    rooms(),
                    vec![(OWNER.into(), peer.anchor())],
                    &CancellationToken::new()
                )
                .await,
            Err(Error::Busy)
        ));
        waiter.abort();
        assert!(matches!(waiter.await, Err(error) if error.is_cancelled()));
        if lost {
            request.raw(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\n{".to_vec());
        } else {
            respond(request, &account, &mut peer, &mut |_, _| {}).await;
        }
        // The account owns the admitted result even after the outer waiter dies.
        // Same-profile calls cannot submit another operation while it is running.
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(4);
        let retained = loop {
            assert!(tokio::time::Instant::now() < deadline);
            if let Some(result) = account.observed_enrollment() {
                break result;
            }
            tokio::select! {
                _=tokio::time::sleep(std::time::Duration::from_millis(10))=>{},
                request=fake.next()=>respond(request,&account,&mut peer,&mut |_,_| {}).await,
            }
        };
        if lost {
            assert!(retained.is_err());
            assert_eq!(peer.claims, 0);
            let before = fake.requests();
            assert!(drive(&account, &mut fake, &mut peer).await.is_err());
            assert_eq!(
                fake.requests(),
                before,
                "lost admitted upload cannot repeat"
            );
            assert_eq!(
                effect_row(&f),
                Some(("provision".into(), "uncertain".into()))
            );
            close(f, fake, c, None, &account).await;
        } else {
            let enrolled = retained.unwrap();
            assert_eq!(peer.writes.len(), 5);
            assert_eq!(peer.claims, 1);
            assert_account_only(&f, &c);
            close(f, fake, c, Some(enrolled), &account).await;
        }
    }
}

#[tokio::test]
async fn native_provisioning_account_enrollment_scope_change() {
    let (f, mut fake, c, account, mut peer) = observed().await;
    let anchor = peer.anchor();
    let cancel = CancellationToken::new();
    let operation = account.enroll_before_activation(
        1,
        [44; 32],
        rooms(),
        vec![(OWNER.into(), anchor)],
        &cancel,
    );
    tokio::pin!(operation);
    let mut direct_reads = 0;
    let result = loop {
        tokio::select! {
            result=&mut operation=>break result,
            request=fake.next()=>{
                if request.target.contains("new_agent_dm") && request.target.ends_with("/state") {
                    direct_reads += 1;
                    if direct_reads == 3 {
                        // Preparing and original SDK Possible are already
                        // durable. Revoke while the last HTTP read is held.
                        f.store.revoke("during_enrollment_room_read".into(),admitted_id(&f)).await.unwrap();
                    }
                }
                respond(request,&account,&mut peer,&mut |_,_| {}).await;
            }
        }
    };
    assert_eq!(direct_reads, 3);
    assert!(result.is_err());
    assert!(account_root(&f).join("sdk").exists());
    assert!(
        peer.writes.is_empty(),
        "post-GET original writer guard refused the actual first key POST"
    );
    assert_eq!(peer.claims, 0);
    assert_eq!(route_rows(&f), 0);
    let before = fake.requests();
    assert!(drive(&account, &mut fake, &mut peer).await.is_err());
    assert_eq!(fake.requests(), before);
    account.close_enrollment_sdk().await.unwrap();
    c.close().await.unwrap();
    f.store.shutdown().await.unwrap();
    fake.close().await;
}
