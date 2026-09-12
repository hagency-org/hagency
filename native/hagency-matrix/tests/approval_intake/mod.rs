use super::*;
use crate::collector::observation::{Phase, Trace, observed};
use crate::{HostIdentity, HostRoom, collector::fixtures as common};
use hagency_core::{replies::*, tasks::*};
use serde_json::{Value, json};
use std::time::{SystemTime, UNIX_EPOCH};
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
fn who() -> Value {
    json!({"user_id":"@approval:example.test","device_id":"BOT_DEVICE","is_guest":false})
}
fn state() -> Value {
    json!([
     {"type":"m.room.member","state_key":"@owner:example.test","content":{"membership":"join"}},
     {"type":"m.room.member","state_key":"@approval:example.test","content":{"membership":"join"}},
     {"type":"m.room.join_rules","state_key":"","content":{"join_rule":"invite"}},
     {"type":"m.room.encryption","state_key":"","content":{"algorithm":"m.megolm.v1.aes-sha2"}}
    ])
}
fn config(f: &common::Fixture, endpoint: &str) -> HostConfig {
    HostConfig::new(
        HostIdentity {
            server_name: "example.test".into(),
            registration_fingerprint: "a".repeat(64),
            transport: MatrixTransportObservation {
                engagement_id: f.identity.transport.engagement_id.clone(),
                registration_generation: 1,
                generation: 1,
                sender_mxid: "@approval:example.test".into(),
                device_id: "BOT_DEVICE".into(),
            },
        },
        endpoint,
        common::TOKEN,
        f.root.path().join("approval-sdk"),
        [42; 32],
        vec![HostRoom {
            room_id: "!private:example.test".into(),
            generation: 1,
            privacy: RoomPrivacy::Direct {
                human_mxid: "@owner:example.test".into(),
            },
        }],
        common::limits(),
    )
    .unwrap()
    .with_root_pem(include_bytes!("../fixtures/ca.pem"))
    .unwrap()
}
async fn preflight(fake: &mut common::Fake) {
    let r = fake.next().await;
    assert!(r.target.ends_with("/whoami"));
    r.json(200, who());
    let r = fake.next().await;
    assert!(r.target.ends_with("/state"));
    r.json(200, state());
}
async fn ready() -> (
    common::Fixture,
    common::Fake,
    ApprovalCollector,
    RunnerCapability,
) {
    ready_variant(None).await
}
async fn ready_variant(
    variant: Option<&'static str>,
) -> (
    common::Fixture,
    common::Fake,
    ApprovalCollector,
    RunnerCapability,
) {
    let f = common::Fixture::new();
    // Separate synthetic host Agent route. Approval HTTP may never replace it.
    f.store
        .observe_matrix_transport(f.identity.transport.clone())
        .await
        .unwrap();
    f.store
        .observe_matrix_room(MatrixRoomObservation {
            engagement_id: f.identity.transport.engagement_id.clone(),
            registration_generation: 1,
            transport_generation: 1,
            room_id: "!project:example.test".into(),
            generation: 1,
            privacy: RoomPrivacy::Group {},
            joined: BTreeSet::from(["@worker:example.test".into(), "@owner:example.test".into()]),
            invite_only: true,
            encrypted: false,
        })
        .await
        .unwrap();
    f.store
        .resolve_verified_matrix_session(SessionBinding {
            id: "root".into(),
            engagement_id: f.identity.transport.engagement_id.clone(),
            room_id: "!project:example.test".into(),
            thread_root: None,
        })
        .await
        .unwrap();
    f.store
        .create_canonical_task(
            "task".into(),
            "root".into(),
            "Approval fixture".into(),
            now(),
        )
        .await
        .unwrap();
    f.store
        .register_workspace("workspace".into())
        .await
        .unwrap();
    f.store
        .enqueue_dispatch(DispatchInput {
            id: "dispatch".into(),
            session_id: "root".into(),
            task_id: Some("task".into()),
            resources: vec![ResourceLease {
                id: "workspace".into(),
                exclusive: true,
            }],
            payload: json!({"instruction":"fixture"}),
        })
        .await
        .unwrap();
    let cap = f
        .store
        .claim_dispatch("runner".into(), now(), 60_000, 120_000, 1)
        .await
        .unwrap()
        .unwrap();
    f.store.start_dispatch(cap.clone(), now()).await.unwrap();
    let mut fake = common::Fake::start(true).await;
    let c = ApprovalCollector::new(
        HostApprovalConfig::new(
            config(&f, &fake.endpoint),
            vec![f.identity.transport.engagement_id.clone()],
        )
        .unwrap(),
        f.store.clone(),
    )
    .unwrap();
    let cancel = CancellationToken::new();
    let (r, ()) = common::scripted(
        observed(
            Trace::new("approval authenticated bootstrap", variant, None),
            c.observe(&cancel),
        ),
        async {
            preflight(&mut fake).await;
        },
    )
    .await;
    r.unwrap();
    // Private test-only provisioning follows the actual authenticated bootstrap.
    *c.inner.owner.lock().await = Some(
        observed(
            Trace::new("approval fixture sdk open", variant, None),
            Owner::open(&c.inner.config),
        )
        .await
        .unwrap(),
    );
    f.store
        .bind_approval_context(
            cap.clone(),
            HostApprovalContext {
                id: "context".into(),
                connection_id: "connection".into(),
                thread_id: "thread".into(),
                turn_id: "turn".into(),
                workspace_resource: "workspace".into(),
                workspace: "/fixture/workspace".into(),
                windows_paths: false,
                environment_id: None,
                may_write: true,
                yolo: false,
            },
        )
        .await
        .unwrap();
    (f, fake, c, cap)
}
async fn request(f: &common::Fixture, cap: &RunnerCapability, id: u64) -> ApprovalIntakeTarget {
    let a=f.store.request_owner_approval(cap.clone(),HostApprovalRequest {context_id:"context".into(),upstream_id:ApprovalRpcId::Number(id),item_id:format!("item{id}"),method:"item/commandExecution/requestApproval".into(),params:json!({"threadId":"thread","turnId":"turn","itemId":format!("item{id}"),"command":"echo approved","cwd":"/fixture/workspace"}),expires_at:now()+60_000}).await.unwrap();
    f.store.approval_intake_target(a.id).await.unwrap()
}
fn verdict(t: &ApprovalIntakeTarget, action: &str) -> Value {
    json!({"msgtype":"com.agentchat.approval.verdict.v1","body":"Owner button action","com.agentchat.approval":{"version":1,"kind":"verdict","agent":t.authority.engagement_id,"project":t.authority.project_id,"project_room_id":t.authority.project_room_id,"request_id":t.request_id,"input_digest":t.request_digest,"action":action}})
}
#[tokio::test]
async fn native_matrix_approval_verdict_real_encrypted_owner_actions_exact_scopes() {
    for (action, choice) in [
        ("approve_once", ApprovalChoice::Once),
        ("approve_task", ApprovalChoice::Task),
        ("approve_always", ApprovalChoice::Always),
        ("deny", ApprovalChoice::Deny),
    ] {
        let (f, mut fake, c, cap) = ready().await;
        let target = request(&f, &cap, 1).await;
        let packet = c
            .inner
            .owner
            .lock()
            .await
            .as_ref()
            .unwrap()
            .approval_fixture(vec![verdict(&target, action)], true)
            .await;
        let cancel = CancellationToken::new();
        let (r, ()) = common::scripted(
            c.intake(
                HostApprovalPlan::new(vec![target.request_id.clone()]).unwrap(),
                &cancel,
            ),
            async {
                preflight(&mut fake).await;
                let q = fake.next().await;
                assert_eq!(q.method, "POST");
                assert!(q.target.ends_with("/keys/query"));
                q.json(200, packet.query.clone());
                let sync = fake.next().await;
                assert!(sync.target.contains("/sync?"));
                sync.json(200, packet.sync.clone());
                preflight(&mut fake).await;
                fake.next().await.json(200, packet.query.clone());
            },
        )
        .await;
        assert_eq!(
            r.unwrap(),
            ApprovalIntakeSummary {
                accepted: 1,
                replayed: 0,
                rejected: 0,
                pending: 0
            }
        );
        let result = f.store.approval_summary(target.request_id).await.unwrap();
        assert_eq!(result.state, "decided");
        assert_eq!(result.choice, Some(choice));
        let grants = f
            .store
            .approval_grants(target.authority.engagement_id.clone(), String::new(), 100)
            .await
            .unwrap();
        assert_eq!(
            grants.len(),
            usize::from(matches!(
                choice,
                ApprovalChoice::Task | ApprovalChoice::Always
            ))
        );
        assert!(f.available().await);
        c.close().await.unwrap();
        f.store.shutdown().await.unwrap();
        fake.close().await;
    }
}

async fn wire_batch(fake: &mut common::Fake, query: &Value, sync: &Value, grants: usize) {
    preflight(fake).await;
    let r = fake.next().await;
    assert!(r.target.ends_with("/keys/query"));
    r.json(200, query.clone());
    let r = fake.next().await;
    assert!(r.target.contains("/sync?"));
    r.json(200, sync.clone());
    for _ in 0..grants {
        preflight(fake).await;
        fake.next().await.json(200, query.clone());
    }
}
async fn packet(c: &ApprovalCollector, contents: Vec<Value>) -> (Value, Value) {
    let p = c
        .inner
        .owner
        .lock()
        .await
        .as_ref()
        .unwrap()
        .approval_fixture(contents, true)
        .await;
    (p.query, p.sync)
}
fn plan(t: &ApprovalIntakeTarget) -> HostApprovalPlan {
    HostApprovalPlan::new(vec![t.request_id.clone()]).unwrap()
}
async fn shutdown(f: common::Fixture, fake: common::Fake, c: ApprovalCollector) {
    observed(
        Trace::new("approval collector cleanup", None, None),
        c.close(),
    )
    .await
    .unwrap();
    common::shutdown_domain(&f.store, "approval fixture cleanup").await;
    fake.close().await;
}
#[tokio::test]
async fn native_matrix_approval_observation_owner_lifecycle() {
    let (f, fake, c, _) = ready().await;
    let inner = c.inner.clone();
    let config = &inner.config;
    observed(
        Trace::new("approval lifecycle initial sdk close", None, None),
        async { c.inner.owner.lock().await.take().unwrap().close().await },
    )
    .await
    .unwrap();

    let opening = Trace::new("approval held sdk open", None, None);
    let mut held = opening.hold(Phase::Prepared);
    let mut open = Box::pin(observed(opening.clone(), Owner::open(config)));
    tokio::select! {
        _ = held.reached() => {},
        _ = &mut open => panic!("approval open escaped its original held boundary"),
    }
    assert!(opening.has(Phase::PrepareStarted));
    assert!(!opening.has(Phase::SdkOpenStarted));
    assert!(matches!(Owner::open(config).await, Err(Error::Busy)));
    held.release();
    *c.inner.owner.lock().await = Some(open.await.unwrap());
    assert!(opening.has(Phase::OpenCallerReturned));
    assert!(opening.has(Phase::SdkOpenReturned));

    let closing = Trace::new("approval held collector close", None, None);
    let mut held = closing.hold(Phase::RuntimeDropStarted);
    let mut close = Box::pin(observed(closing.clone(), c.close()));
    tokio::select! {
        _ = held.reached() => {},
        result = &mut close => panic!("approval close escaped its original held boundary: {result:?}"),
    }
    assert!(closing.has(Phase::Queued));
    assert!(closing.has(Phase::Started));
    assert!(closing.has(Phase::CloseStoresReturned));
    assert!(!closing.has(Phase::LockDropped));
    assert!(matches!(Owner::open(config).await, Err(Error::Busy)));
    // The fixture caller disappears; the original spawned close and its SDK
    // command retain this same trace and private lock until actual completion.
    drop(close);
    held.release();
    closing.wait(Phase::OwnerReturned).await;
    assert!(closing.has(Phase::RuntimeDropped));
    assert!(closing.has(Phase::LockDropped));
    assert!(closing.has(Phase::CloseAcknowledgement));
    assert!(closing.has(Phase::CallerReturned));
    assert!(!closing.has(Phase::OperationReturned));
    assert_eq!(closing.snapshot().callsite, "approval held collector close");
    assert!(closing.snapshot().sdk_failure.is_none());
    let reopened = observed(
        Trace::new("approval lifecycle sdk reopen", None, None),
        Owner::open_existing(config),
    )
    .await
    .unwrap();
    observed(
        Trace::new("approval lifecycle reopened sdk close", None, None),
        reopened.close(),
    )
    .await
    .unwrap();
    common::shutdown_domain(&f.store, "approval lifecycle cleanup").await;
    fake.close().await;
}
#[tokio::test]
async fn native_matrix_approval_identity_wrong_device_fences_only_approval_and_purpose_cannot_adopt()
 {
    let (f, mut fake, c, _) = ready().await;
    let owner = c.inner.owner.lock().await.take().unwrap();
    owner.close().await.unwrap();
    assert!(matches!(
        Owner::open_existing(&config(&f, &fake.endpoint)).await,
        Err(Error::Identity)
    ));
    *c.inner.owner.lock().await = Some(Owner::open_existing(&c.inner.config).await.unwrap());
    let cancel = CancellationToken::new();
    let (r, ()) = common::scripted(c.observe(&cancel), async {
        fake.next().await.json(
            200,
            json!({"user_id":"@approval:example.test","device_id":"REPLACED","is_guest":false}),
        );
    })
    .await;
    assert_eq!(r, Err(Error::Identity));
    assert!(f.available().await);
    let authority = f
        .store
        .approval_room_authority(f.identity.transport.engagement_id.clone())
        .await
        .unwrap();
    assert!(
        !f.store
            .approval_room_capture(authority)
            .await
            .unwrap()
            .unwrap()
            .available
    );
    shutdown(f, fake, c).await;
}
#[tokio::test]
async fn native_matrix_approval_scope_plaintext_altered_request_action_and_edits_are_terminal() {
    for variant in 0..6 {
        let (f, mut fake, c, cap) = ready().await;
        let target = request(&f, &cap, 1).await;
        let mut content = verdict(&target, "approve_always");
        match variant {
            0 => {}
            1 => content["com.agentchat.approval"]["input_digest"] = json!("b".repeat(64)),
            2 => content["com.agentchat.approval"]["project"] = json!("other-project"),
            3 => content["com.agentchat.approval"]["action"] = json!("approve_everything"),
            4 => {
                content["m.new_content"] = content.clone();
                content["m.relates_to"] = json!({"rel_type":"m.replace","event_id":"$old"});
            }
            5 => {
                content["com.agentchat.approval"]["request_id"] =
                    json!(format!("approval_{}", "a".repeat(32)))
            }
            _ => unreachable!(),
        }
        let (query, mut sync) = packet(&c, vec![content.clone()]).await;
        if variant == 0 {
            sync["rooms"]["join"]["!private:example.test"]["timeline"]["events"][0] = json!({"event_id":"$verdict0","sender":"@owner:example.test","origin_server_ts":now(),"type":"m.room.message","content":content});
        }
        let cancel = CancellationToken::new();
        let (r, ()) = common::scripted(
            c.intake(plan(&target), &cancel),
            wire_batch(&mut fake, &query, &sync, 0),
        )
        .await;
        assert_eq!(
            r.unwrap_or_else(|e| panic!("variant {variant}: {e:?}"))
                .rejected,
            1,
            "variant {variant}"
        );
        assert_eq!(
            f.store
                .approval_summary(target.request_id.clone())
                .await
                .unwrap()
                .state,
            "pending"
        );
        assert_eq!(c.custody_status().await.unwrap().terminal_sources, 1);
        assert!(
            f.store
                .approval_grants(target.authority.engagement_id, String::new(), 100)
                .await
                .unwrap()
                .is_empty()
        );
        shutdown(f, fake, c).await;
    }
}
#[tokio::test]
async fn native_matrix_approval_scope_third_member_and_fresh_malformed_keys_fence_private_grants() {
    for variant in 0..2 {
        let (f, mut fake, c, cap) = ready().await;
        let target = request(&f, &cap, 1).await;
        let (mut query, sync) = packet(&c, vec![verdict(&target, "approve_always")]).await;
        let cancel = CancellationToken::new();
        let result = if variant == 0 {
            let (r,())=common::scripted(c.intake(plan(&target),&cancel),async {
                fake.next().await.json(200,who());
                let mut unsafe_state=state(); unsafe_state.as_array_mut().unwrap().push(json!({"type":"m.room.member","state_key":"@third:example.test","content":{"membership":"join"}}));
                fake.next().await.json(200,unsafe_state);
            }).await;
            r
        } else {
            query["master_keys"]["@owner:example.test"] = json!({});
            let (r, ()) = common::scripted(
                c.intake(plan(&target), &cancel),
                wire_batch(&mut fake, &query, &sync, 0),
            )
            .await;
            r
        };
        assert!(result.is_err());
        assert!(f.available().await);
        let a = f
            .store
            .approval_room_authority(target.authority.engagement_id.clone())
            .await
            .unwrap();
        assert!(
            !f.store
                .approval_room_capture(a)
                .await
                .unwrap()
                .unwrap()
                .available
        );
        assert_eq!(
            f.store
                .approval_summary(target.request_id)
                .await
                .unwrap()
                .state,
            "pending"
        );
        shutdown(f, fake, c).await;
    }
}
#[tokio::test]
async fn native_matrix_approval_recovery_lost_commit_reopens_exact_receipt_after_private_rotation()
{
    let (f, mut fake, c, cap) = ready().await;
    let target = request(&f, &cap, 1).await;
    let (query, sync) = packet(&c, vec![verdict(&target, "approve_always")]).await;
    c.inner
        .handoff_fault
        .store(2, std::sync::atomic::Ordering::SeqCst);
    let cancel = CancellationToken::new();
    let (r, ()) = common::scripted(
        c.intake(plan(&target), &cancel),
        wire_batch(&mut fake, &query, &sync, 1),
    )
    .await;
    assert_eq!(r, Err(Error::OutcomeUnknown));
    assert!(f.available().await);
    let status = c.custody_status().await.unwrap();
    assert_eq!(status.stage, ApprovalCustodyStage::Derived);
    assert_eq!(status.acknowledged, 0);
    c.inner
        .owner
        .lock()
        .await
        .take()
        .unwrap()
        .close()
        .await
        .unwrap();
    f.store
        .observe_approval_room(ApprovalRoomObservation {
            engagement_id: target.authority.engagement_id.clone(),
            registration_generation: 1,
            generation: 2,
            room_id: target.authority.room_id.clone(),
            device_id: "NEW_BOT_DEVICE".into(),
            joined: BTreeSet::from([
                "@owner:example.test".into(),
                "@approval:example.test".into(),
            ]),
            invite_only: true,
            encrypted: true,
            available: true,
        })
        .await
        .unwrap();
    let result = c.resume_custody(&cancel).await.unwrap();
    assert_eq!(result.replayed, 1);
    assert_eq!(result.accepted, 0);
    fake.no_request().await;
    assert_eq!(
        c.custody_status().await.unwrap().stage,
        ApprovalCustodyStage::Idle
    );
    let grants = f
        .store
        .approval_grants(target.authority.engagement_id.clone(), String::new(), 100)
        .await
        .unwrap();
    assert_eq!(grants.len(), 1);
    assert!(grants[0].revoked);
    c.close().await.unwrap();
    let capture = f
        .store
        .approval_room_capture(target.authority)
        .await
        .unwrap()
        .unwrap();
    assert!(capture.available);
    assert_eq!(capture.device_id, "NEW_BOT_DEVICE");
    f.store.shutdown().await.unwrap();
    fake.close().await;
}
#[tokio::test]
async fn native_matrix_approval_recovery_busy_then_negative_room_settles_unaccepted_without_network()
 {
    let (f, mut fake, c, cap) = ready().await;
    let target = request(&f, &cap, 1).await;
    let (query, sync) = packet(&c, vec![verdict(&target, "approve_once")]).await;
    c.inner
        .handoff_fault
        .store(1, std::sync::atomic::Ordering::SeqCst);
    let cancel = CancellationToken::new();
    let (r, ()) = common::scripted(
        c.intake(plan(&target), &cancel),
        wire_batch(&mut fake, &query, &sync, 0),
    )
    .await;
    assert_eq!(r, Err(Error::Busy));
    assert!(f.available().await);
    assert!(
        f.store
            .approval_room_capture(target.authority.clone())
            .await
            .unwrap()
            .unwrap()
            .available
    );
    assert_eq!(c.resume_custody(&cancel).await.unwrap().pending, 1);
    f.store
        .fence_approval_room(
            target.authority.clone(),
            target.device_id.clone(),
            target.room_generation,
            None,
        )
        .await
        .unwrap();
    let result = c.resume_custody(&cancel).await.unwrap();
    assert_eq!(result.rejected, 1);
    assert_eq!(result.pending, 0);
    fake.no_request().await;
    assert_eq!(
        f.store
            .approval_summary(target.request_id)
            .await
            .unwrap()
            .state,
        "pending"
    );
    shutdown(f, fake, c).await;
}
#[tokio::test]
async fn native_matrix_approval_recovery_sdk_applying_reopens_inspectable_without_cursor_skip() {
    let (f, mut fake, c, cap) = ready().await;
    let target = request(&f, &cap, 1).await;
    let (query, sync) = packet(&c, vec![verdict(&target, "approve_once")]).await;
    c.inner
        .owner
        .lock()
        .await
        .as_ref()
        .unwrap()
        .apply_fault()
        .await;
    let cancel = CancellationToken::new();
    let (r, ()) = common::scripted(
        c.intake(plan(&target), &cancel),
        wire_batch(&mut fake, &query, &sync, 0),
    )
    .await;
    assert_eq!(r, Err(Error::OutcomeUnknown));
    assert!(f.available().await);
    c.inner
        .owner
        .lock()
        .await
        .take()
        .unwrap()
        .close()
        .await
        .unwrap();
    let status = c.custody_status().await.unwrap();
    assert_eq!(status.stage, ApprovalCustodyStage::Applying);
    assert!(status.retained_response_bytes > 100);
    assert_eq!(status.frozen_targets, 1);
    assert_eq!(status.completed_batches, 0);
    assert_eq!(c.resume_custody(&cancel).await, Err(Error::OutcomeUnknown));
    fake.no_request().await;
    shutdown(f, fake, c).await;
}
#[tokio::test]
async fn native_matrix_approval_scope_cancel_and_negative_after_fresh_checks_cannot_commit() {
    for cancel_operation in [false, true] {
        let (f, mut fake, c, cap) = ready().await;
        let target = request(&f, &cap, 1).await;
        let (query, sync) = packet(&c, vec![verdict(&target, "approve_always")]).await;
        c.inner
            .handoff_fault
            .store(3, std::sync::atomic::Ordering::SeqCst);
        let cancel = CancellationToken::new();
        let (result, ()) = tokio::join!(c.intake(plan(&target), &cancel), async {
            wire_batch(&mut fake, &query, &sync, 1).await;
            c.inner.handoff_reached.notified().await;
            f.store
                .fence_approval_room(
                    target.authority.clone(),
                    target.device_id.clone(),
                    target.room_generation,
                    None,
                )
                .await
                .unwrap();
            if cancel_operation {
                cancel.cancel();
            }
            c.inner.handoff_continue.notify_one();
        });
        if cancel_operation {
            assert_eq!(result, Err(Error::Cancelled));
            assert_eq!(
                c.resume_custody(&CancellationToken::new())
                    .await
                    .unwrap()
                    .rejected,
                1
            );
        } else {
            assert_eq!(result.unwrap().rejected, 1);
        }
        assert!(
            f.store
                .approval_grants(target.authority.engagement_id, String::new(), 100)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(f.available().await);
        shutdown(f, fake, c).await;
    }
}
#[tokio::test]
async fn native_matrix_approval_bounds_same_ciphertext_cannot_acquire_new_target_authority_after_reopen()
 {
    let (f, mut fake, c, cap) = ready().await;
    let target = request(&f, &cap, 1).await;
    let (query, mut sync) = packet(&c, vec![verdict(&target, "approve_always")]).await;
    let cancel = CancellationToken::new();
    let (r, ()) = common::scripted(
        c.intake(HostApprovalPlan::new(vec![]).unwrap(), &cancel),
        wire_batch(&mut fake, &query, &sync, 0),
    )
    .await;
    assert_eq!(r.unwrap().rejected, 1);
    c.inner
        .owner
        .lock()
        .await
        .take()
        .unwrap()
        .close()
        .await
        .unwrap();
    c.custody_status().await.unwrap();
    sync["next_batch"] = json!("second");
    sync["to_device"]["events"] = json!([]);
    let (r, ()) = common::scripted(
        c.intake(plan(&target), &cancel),
        wire_batch(&mut fake, &query, &sync, 0),
    )
    .await;
    assert_eq!(r.unwrap().replayed, 1);
    assert_eq!(
        f.store
            .approval_summary(target.request_id)
            .await
            .unwrap()
            .state,
        "pending"
    );
    assert_eq!(c.custody_status().await.unwrap().terminal_sources, 1);
    shutdown(f, fake, c).await;
}
#[tokio::test]
async fn native_matrix_approval_bounds_filtered_plain_source_changed_into_verdict_is_quarantined() {
    let (f, mut fake, c, cap) = ready().await;
    let target = request(&f, &cap, 1).await;
    let (query, mut sync) = packet(&c, vec![verdict(&target, "approve_always")]).await;
    sync["rooms"]["join"]["!private:example.test"]["timeline"]["events"][0] = json!({"event_id":"$verdict0","sender":"@owner:example.test","origin_server_ts":now(),"type":"m.room.message","content":{"msgtype":"m.text","body":"ordinary chat"}});
    let cancel = CancellationToken::new();
    let (r, ()) = common::scripted(
        c.intake(plan(&target), &cancel),
        wire_batch(&mut fake, &query, &sync, 0),
    )
    .await;
    assert_eq!(r.unwrap().rejected, 1);
    sync["next_batch"] = json!("second");
    sync["to_device"]["events"] = json!([]);
    sync["rooms"]["join"]["!private:example.test"]["timeline"]["events"][0]["content"] =
        verdict(&target, "approve_always");
    let (r, ()) = common::scripted(
        c.intake(plan(&target), &cancel),
        wire_batch(&mut fake, &query, &sync, 0),
    )
    .await;
    assert_eq!(r, Err(Error::Conflict));
    assert_eq!(
        c.custody_status().await.unwrap().stage,
        ApprovalCustodyStage::Quarantined
    );
    assert_eq!(c.custody_status().await.unwrap().terminal_sources, 1);
    shutdown(f, fake, c).await;
}
#[tokio::test]
async fn native_matrix_approval_bounds_receipt_and_source_capacity_refuse_without_evicting_or_polling()
 {
    for source_capacity in [false, true] {
        let (f, mut fake, c, _) = ready().await;
        let (query, base) = packet(&c, vec![]).await;
        let cancel = CancellationToken::new();
        let batches = if source_capacity { 3 } else { 64 };
        for batch in 0..batches {
            let mut sync = base.clone();
            sync["next_batch"] = json!(format!("bounded_{batch}"));
            sync["to_device"]["events"] = json!([]);
            let n = if source_capacity {
                if batch < 2 { 100 } else { 56 }
            } else {
                0
            };
            sync["rooms"]["join"]["!private:example.test"]["timeline"]["events"]=Value::Array((0..n).map(|i|json!({"event_id":format!("$ordinary_{batch}_{i}"),"sender":"@owner:example.test","origin_server_ts":now(),"type":"m.room.message","content":{"msgtype":"m.text","body":"not a verdict"}})).collect());
            let (r, ()) = common::scripted(
                c.intake(HostApprovalPlan::new(vec![]).unwrap(), &cancel),
                wire_batch(&mut fake, &query, &sync, 0),
            )
            .await;
            assert_eq!(r.unwrap().rejected, n);
        }
        let before = c.custody_status().await.unwrap();
        assert_eq!(
            c.intake(HostApprovalPlan::new(vec![]).unwrap(), &cancel)
                .await,
            Err(Error::Capacity)
        );
        fake.no_request().await;
        c.inner
            .owner
            .lock()
            .await
            .take()
            .unwrap()
            .close()
            .await
            .unwrap();
        assert_eq!(c.custody_status().await.unwrap(), before);
        shutdown(f, fake, c).await;
    }
}
#[tokio::test]
async fn native_matrix_approval_bounds_corrupt_encrypted_journal_cannot_resume_authority() {
    for variant in 0..9 {
        let label = [
            "corruption0",
            "corruption1",
            "corruption2",
            "corruption3",
            "corruption4",
            "corruption5",
            "corruption6",
            "corruption7",
            "corruption8",
        ][variant as usize];
        let (f, mut fake, c, cap) = ready_variant(Some(label)).await;
        let target = request(&f, &cap, 1).await;
        let (query, sync) = packet(&c, vec![verdict(&target, "approve_once")]).await;
        c.inner
            .handoff_fault
            .store(1, std::sync::atomic::Ordering::SeqCst);
        let cancel = CancellationToken::new();
        let (r, ()) = common::scripted(
            c.intake(plan(&target), &cancel),
            wire_batch(&mut fake, &query, &sync, 0),
        )
        .await;
        assert_eq!(r, Err(Error::Busy));
        c.inner
            .owner
            .lock()
            .await
            .as_ref()
            .unwrap()
            .corrupt_approval(variant)
            .await;
        observed(
            Trace::new("approval corrupted sdk close", Some(label), None),
            async { c.inner.owner.lock().await.take().unwrap().close().await },
        )
        .await
        .unwrap();
        assert!(
            matches!(c.custody_status().await, Err(Error::Storage)),
            "variant {variant}"
        );
        assert_eq!(
            f.store
                .approval_summary(target.request_id)
                .await
                .unwrap()
                .state,
            "pending"
        );
        fake.no_request().await;
        shutdown(f, fake, c).await;
    }
}
#[tokio::test]
async fn native_matrix_approval_bounds_duplicate_timeline_and_undecryptable_event_retain_quarantine()
 {
    for duplicate in [true, false] {
        let (f, mut fake, c, cap) = ready().await;
        let target = request(&f, &cap, 1).await;
        let (query, mut sync) = packet(&c, vec![verdict(&target, "approve_once")]).await;
        if duplicate {
            let event =
                sync["rooms"]["join"]["!private:example.test"]["timeline"]["events"][0].clone();
            sync["rooms"]["join"]["!private:example.test"]["timeline"]["events"]
                .as_array_mut()
                .unwrap()
                .push(event);
        } else {
            sync["to_device"]["events"] = json!([]);
        }
        let cancel = CancellationToken::new();
        let (r, ()) = common::scripted(
            c.intake(plan(&target), &cancel),
            wire_batch(&mut fake, &query, &sync, 0),
        )
        .await;
        assert_eq!(
            r,
            Err(if duplicate {
                Error::Conflict
            } else {
                Error::Unsupported
            })
        );
        let status = c.custody_status().await.unwrap();
        assert_eq!(status.stage, ApprovalCustodyStage::Quarantined);
        assert_eq!(status.completed_batches, 0);
        assert!(status.retained_response_bytes > 100);
        assert!(f.available().await);
        assert!(
            f.store
                .approval_room_capture(target.authority)
                .await
                .unwrap()
                .unwrap()
                .available
        );
        shutdown(f, fake, c).await;
    }
}
#[tokio::test]
async fn native_matrix_approval_scope_unverified_owner_cannot_supply_crypto_authority() {
    let (f, mut fake, c, cap) = ready().await;
    let target = request(&f, &cap, 1).await;
    let p = c
        .inner
        .owner
        .lock()
        .await
        .as_ref()
        .unwrap()
        .approval_fixture(vec![verdict(&target, "approve_always")], false)
        .await;
    let cancel = CancellationToken::new();
    let (r, ()) = common::scripted(
        c.intake(plan(&target), &cancel),
        wire_batch(&mut fake, &p.query, &p.sync, 0),
    )
    .await;
    assert_eq!(r, Err(Error::Recipients));
    assert_eq!(
        f.store
            .approval_summary(target.request_id)
            .await
            .unwrap()
            .state,
        "pending"
    );
    assert!(
        f.store
            .approval_grants(target.authority.engagement_id, String::new(), 100)
            .await
            .unwrap()
            .is_empty()
    );
    shutdown(f, fake, c).await;
}
#[tokio::test]
async fn native_matrix_approval_bounds_sync_framing_and_body_limits_do_not_advance_crypto_cursor() {
    for variant in 0..4 {
        let label = [
            "duplicate next batch",
            "trailing document",
            "oversized body",
            "excess events",
        ][variant];
        let (f, mut fake, c, _) = ready_variant(Some(label)).await;
        let (query, mut base) = packet(&c, vec![]).await;
        let cancel = CancellationToken::new();
        let bytes = match variant {
            0 => br#"{"next_batch":"one","next_batch":"two"}"#.to_vec(),
            1 => br#"{"next_batch":"one"}{"next_batch":"two"}"#.to_vec(),
            2 => vec![b' '; 1024 * 1024 + 1],
            _ => {
                base["rooms"]["join"]["!private:example.test"]["timeline"]["events"]=Value::Array((0..101).map(|i|json!({"event_id":format!("$excess_{i}"),"sender":"@owner:example.test","origin_server_ts":now(),"type":"m.room.message","content":{"msgtype":"m.text","body":"ordinary"}})).collect());
                serde_json::to_vec(&base).unwrap()
            }
        };
        let (r, ()) = common::scripted(
            c.intake(HostApprovalPlan::new(vec![]).unwrap(), &cancel),
            async {
                preflight(&mut fake).await;
                fake.next().await.json(200, query.clone());
                fake.next().await.raw(common::response(200, &bytes));
            },
        )
        .await;
        assert_eq!(
            r,
            Err(if variant == 3 {
                Error::Capacity
            } else if variant == 2 {
                Error::BodyTooLarge
            } else {
                Error::InvalidJson
            })
        );
        assert_eq!(
            c.custody_status().await.unwrap().stage,
            ApprovalCustodyStage::Idle
        );
        assert_eq!(c.custody_status().await.unwrap().completed_batches, 0);
        shutdown(f, fake, c).await;
    }
}
#[tokio::test]
async fn native_matrix_approval_recovery_ack_journal_rollback_requires_reopen_and_exact_receipt() {
    let (f, mut fake, c, cap) = ready().await;
    let target = request(&f, &cap, 1).await;
    let (query, sync) = packet(&c, vec![verdict(&target, "approve_always")]).await;
    let sql =
        rusqlite::Connection::open(f.root.path().join("approval-sdk/matrix-sdk-state.sqlite3"))
            .unwrap();
    c.inner
        .handoff_fault
        .store(3, std::sync::atomic::Ordering::SeqCst);
    let cancel = CancellationToken::new();
    let (r, ()) = tokio::join!(c.intake(plan(&target), &cancel), async {
        wire_batch(&mut fake, &query, &sync, 1).await;
        c.inner.handoff_reached.notified().await;
        sql.execute_batch("CREATE TRIGGER reject_approval_journal BEFORE INSERT ON kv_blob BEGIN SELECT RAISE(ABORT,'fixture rollback'); END;").unwrap();
        c.inner.handoff_continue.notify_one();
    });
    assert_eq!(r, Err(Error::OutcomeUnknown));
    assert_eq!(
        f.store
            .approval_summary(target.request_id)
            .await
            .unwrap()
            .state,
        "decided"
    );
    assert_eq!(c.custody_status().await, Err(Error::OutcomeUnknown));
    sql.execute_batch("DROP TRIGGER reject_approval_journal")
        .unwrap();
    drop(sql);
    c.inner
        .owner
        .lock()
        .await
        .take()
        .unwrap()
        .close()
        .await
        .unwrap();
    let status = c.custody_status().await.unwrap();
    assert_eq!(status.stage, ApprovalCustodyStage::Derived);
    assert_eq!(status.acknowledged, 0);
    let result = c.resume_custody(&cancel).await.unwrap();
    assert_eq!(result.replayed, 1);
    assert_eq!(result.accepted, 0);
    fake.no_request().await;
    assert_eq!(
        f.store
            .approval_grants(target.authority.engagement_id, String::new(), 100)
            .await
            .unwrap()
            .len(),
        1
    );
    shutdown(f, fake, c).await;
}
