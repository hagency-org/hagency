use super::*;
use crate::collector::observation::{Phase as ObservationPhase, Trace, observed};
use crate::{HostConfig, HostIdentity, HostIntakePlan, HostRoom, collector::fixtures as common};
use hagency_core::{ingress::VerifiedTaskRequest, task_intents::TaskDefinition, tasks::*};
use serde_json::{Value, json};
use std::{
    sync::atomic::Ordering,
    time::{SystemTime, UNIX_EPOCH},
};
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
fn config(f: &common::Fixture, endpoint: &str, identity: HostIdentity, direct: bool) -> HostConfig {
    HostConfig::new(
        identity,
        endpoint,
        common::TOKEN,
        f.root.path().join("sdk"),
        [42; 32],
        vec![HostRoom {
            room_id: "!project:example.test".into(),
            generation: 1,
            privacy: if direct {
                RoomPrivacy::Direct {
                    human_mxid: "@owner:example.test".into(),
                }
            } else {
                RoomPrivacy::Group {}
            },
        }],
        common::limits(),
    )
    .unwrap()
    .with_root_pem(include_bytes!("../fixtures/ca.pem"))
    .unwrap()
}
fn room(encrypted: bool) -> Value {
    let mut v = common::state();
    if !encrypted {
        v.as_array_mut()
            .unwrap()
            .retain(|e| e["type"] != "m.room.encryption");
    }
    v
}
async fn scripted<T: std::fmt::Debug, S: std::future::Future>(
    callsite: &'static str,
    variant: Option<&'static str>,
    operation: impl std::future::Future<Output = Result<T, Error>>,
    script: S,
) -> (Result<T, Error>, S::Output) {
    let trace = Trace::new(callsite, variant, None);
    let result = common::scripted(observed(trace.clone(), operation), script).await;
    assert!(trace.has(ObservationPhase::OwnerReturned));
    result
}
async fn ready(encrypted: bool, direct: bool) -> (common::Fixture, common::Fake, Collector) {
    ready_named(encrypted, direct, "outgoing bootstrap", None).await
}
async fn ready_named(
    encrypted: bool,
    direct: bool,
    callsite: &'static str,
    variant: Option<&'static str>,
) -> (common::Fixture, common::Fake, Collector) {
    let f = common::Fixture::new();
    let mut fake = common::Fake::start(true).await;
    let c = Collector::new(
        config(&f, &fake.endpoint, f.identity.clone(), direct),
        f.store.clone(),
    )
    .unwrap();
    let cancel = CancellationToken::new();
    let (r, ()) = scripted(callsite, variant, c.collect(&cancel), async {
        fake.next().await.json(200, common::who());
        fake.next().await.json(200, common::sync("boot"));
        fake.next().await.json(200, room(encrypted));
    })
    .await;
    r.unwrap();
    f.store
        .resolve_verified_matrix_session(SessionBinding {
            id: "root".into(),
            engagement_id: f.identity.transport.engagement_id.clone(),
            room_id: "!project:example.test".into(),
            thread_root: None,
        })
        .await
        .unwrap();
    (f, fake, c)
}
async fn final_claim(f: &common::Fixture) -> ReplyClaim {
    final_claim_named(f, "task").await
}
async fn final_claim_named(f: &common::Fixture, name: &str) -> ReplyClaim {
    f.store
        .create_canonical_task(name.into(), "root".into(), "Finish result".into(), now())
        .await
        .unwrap();
    f.store
        .enqueue_dispatch(DispatchInput {
            id: format!("run_{name}"),
            session_id: "root".into(),
            task_id: Some(name.into()),
            resources: vec![],
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
    f.store
        .mutate_task(
            cap.clone(),
            name.into(),
            "done".into(),
            TaskMutation::Transition {
                status: TaskState::Done,
                waiting_reason: None,
                waiting_until: None,
            },
            now(),
        )
        .await
        .unwrap();
    f.store
        .runner_command(
            cap.clone(),
            RunnerCommand::SubmitFinalReply(FinalReply {
                call_id: "final".into(),
                body: "Answer **verified** 中文".into(),
            }),
        )
        .await
        .unwrap();
    f.store
        .complete_dispatch(cap, json!({"observed":"fixture completed"}), now())
        .await
        .unwrap();
    f.store.claim_final_reply(60_000).await.unwrap().unwrap()
}
async fn preflight(fake: &mut common::Fake, encrypted: bool) {
    let r = fake.next().await;
    assert_eq!(r.method, "GET");
    assert_eq!(r.target, "/_matrix/client/v3/account/whoami");
    r.json(200, common::who());
    let r = fake.next().await;
    assert!(r.target.ends_with("/state"));
    r.json(200, room(encrypted));
}
async fn plain_wire(fake: &mut common::Fake) -> common::Request {
    preflight(fake, false).await;
    preflight(fake, false).await;
    let r = fake.next().await;
    assert_eq!(r.method, "PUT");
    assert!(r.target.contains("/send/m.room.message/"));
    assert_eq!(
        r.headers["authorization"],
        format!("Bearer {}", common::TOKEN)
    );
    r
}
fn state(f: &common::Fixture, id: &str) -> String {
    rusqlite::Connection::open(f.root.path().join("domain/domain.sqlite3"))
        .unwrap()
        .query_row("SELECT state FROM final_replies WHERE id=?1", [id], |r| {
            r.get(0)
        })
        .unwrap()
}
async fn stop_sdk(c: Collector) {
    c.inner
        .owner
        .lock()
        .await
        .take()
        .unwrap()
        .close()
        .await
        .unwrap();
    drop(c);
}
#[tokio::test]
async fn native_matrix_outgoing_plain_final_actual_https_formatted_and_idempotent_receipt() {
    let (f, mut fake, c) = ready(false, false).await;
    let claim = final_claim(&f).await;
    let cancel = CancellationToken::new();
    let (result, body) = common::scripted(c.send_final(claim.clone(), &cancel), async {
        let req = plain_wire(&mut fake).await;
        assert_eq!(state(&f, &claim.id), "sending");
        let body: Value = serde_json::from_slice(&req.body).unwrap();
        req.json(200, json!({"event_id":"$accepted"}));
        body
    })
    .await;
    assert_eq!(result.unwrap().state, OutgoingState::Delivered);
    assert_eq!(state(&f, &claim.id), "delivered");
    assert_eq!(body["body"], "Answer **verified** 中文");
    assert!(
        body["formatted_body"]
            .as_str()
            .unwrap()
            .contains("<strong>verified</strong>")
    );
    assert!(body.get("m.relates_to").is_none());
    assert!(c.send_final(claim, &cancel).await.unwrap().replayed);
    fake.no_request().await;
    c.close().await.unwrap();
    f.store.shutdown().await.unwrap();
    fake.close().await;
}
#[tokio::test]
async fn native_matrix_outgoing_recovery_accepted_response_survives_busy_lost_domain_reply_and_restart()
 {
    for fault in [1, 2] {
        let variant_label = Some(if fault == 1 {
            "busy"
        } else {
            "lost domain reply"
        });
        let (f, mut fake, c) =
            ready_named(false, false, "accepted recovery bootstrap", variant_label).await;
        let claim = final_claim(&f).await;
        let cancel = CancellationToken::new();
        c.inner.outgoing_fault.store(fault, Ordering::SeqCst);
        let (result, ()) = scripted(
            "accepted recovery send",
            variant_label,
            c.send_final(claim.clone(), &cancel),
            async {
                plain_wire(&mut fake)
                    .await
                    .json(200, json!({"event_id":"$journaled"}));
            },
        )
        .await;
        assert_eq!(
            result,
            Err(if fault == 1 {
                Error::Busy
            } else {
                Error::OutcomeUnknown
            })
        );
        assert!(f.available().await);
        assert_eq!(
            state(&f, &claim.id),
            if fault == 1 { "sending" } else { "delivered" }
        );
        stop_sdk(c).await;
        let c = Collector::new(
            config(&f, &fake.endpoint, f.identity.clone(), false),
            f.store.clone(),
        )
        .unwrap();
        let r = observed(
            Trace::new("accepted recovery resume", variant_label, None),
            c.resume_outgoing_custody(&cancel),
        )
        .await
        .unwrap();
        assert_eq!(r.state, OutgoingState::Delivered);
        assert!(r.replayed);
        fake.no_request().await;
        assert_eq!(state(&f, &claim.id), "delivered");
        observed(
            Trace::new("accepted recovery close", variant_label, None),
            c.close(),
        )
        .await
        .unwrap();
        f.store.shutdown().await.unwrap();
        fake.close().await;
    }
}
#[tokio::test]
async fn native_matrix_outgoing_recovery_lost_http_and_begin_do_not_replay() {
    for lost_begin in [false, true] {
        let variant_label = Some(if lost_begin {
            "lost begin"
        } else {
            "lost HTTP"
        });
        let (f, mut fake, c) =
            ready_named(false, false, "lost write recovery bootstrap", variant_label).await;
        let claim = final_claim(&f).await;
        let cancel = CancellationToken::new();
        if lost_begin {
            c.inner.outgoing_fault.store(4, Ordering::SeqCst);
        }
        let (result, ()) = scripted(
            "lost write recovery send",
            variant_label,
            c.send_final(claim.clone(), &cancel),
            async {
                if lost_begin {
                    preflight(&mut fake, false).await;
                } else {
                    drop(plain_wire(&mut fake).await);
                }
            },
        )
        .await;
        assert!(result.is_err());
        assert_eq!(state(&f, &claim.id), "sending");
        stop_sdk(c).await;
        let c = Collector::new(
            config(&f, &fake.endpoint, f.identity.clone(), false),
            f.store.clone(),
        )
        .unwrap();
        assert_eq!(
            observed(
                Trace::new("lost write recovery resume", variant_label, None),
                c.resume_outgoing_custody(&cancel)
            )
            .await
            .unwrap()
            .state,
            OutgoingState::Uncertain
        );
        fake.no_request().await;
        assert_eq!(
            c.send_final(claim, &cancel).await,
            Err(Error::OutcomeUnknown)
        );
        fake.no_request().await;
        observed(
            Trace::new("lost write recovery close", variant_label, None),
            c.close(),
        )
        .await
        .unwrap();
        f.store.shutdown().await.unwrap();
        fake.close().await;
    }
}
#[tokio::test]
async fn native_matrix_outgoing_domain_accepted_after_cancellation_records_history() {
    let (f, mut fake, c) = ready(false, false).await;
    let claim = final_claim(&f).await;
    let cancel = CancellationToken::new();
    c.inner.outgoing_fault.store(3, Ordering::SeqCst);
    let (result, ()) = common::scripted(c.send_final(claim.clone(), &cancel), async {
        plain_wire(&mut fake)
            .await
            .json(200, json!({"event_id":"$late"}));
        c.inner.outgoing_reached.notified().await;
        f.store.cancel_final_reply(claim.id.clone()).await.unwrap();
        cancel.cancel();
        c.inner.outgoing_continue.notify_one();
    })
    .await;
    assert_eq!(result.unwrap().state, OutgoingState::Delivered);
    assert_eq!(state(&f, &claim.id), "delivered");
    c.close().await.unwrap();
    f.store.shutdown().await.unwrap();
    fake.close().await;
}
async fn notice_claim(
    f: &common::Fixture,
    fake: &mut common::Fake,
    c: &Collector,
) -> (VerifiedNoticeClaim, String) {
    let cancel = CancellationToken::new();
    let event = json!({"event_id":"$question","sender":"@owner:example.test","type":"m.room.message","origin_server_ts":now(),"content":{"msgtype":"m.text","body":"Please implement","m.mentions":{"user_ids":["@worker:example.test"]}}});
    let sync = json!({"next_batch":"question","rooms":{"join":{"!project:example.test":{"timeline":{"limited":false,"events":[event]},"state":{"events":[]}}}},"to_device":{"events":[]}});
    let (r, ()) = scripted(
        "outgoing notice intake",
        None,
        c.intake(HostIntakePlan::new(vec!["root".into()]).unwrap(), &cancel),
        async {
            fake.next().await.json(200, common::who());
            fake.next().await.json(200, sync);
            fake.next().await.json(200, room(false));
        },
    )
    .await;
    r.unwrap();
    let scope = f.store.matrix_ingress_scope("root".into()).await.unwrap();
    let inbox = f.store.inbox("root".into(), 0, 10, None).await.unwrap();
    let intent = f
        .store
        .create_verified_task_intent(VerifiedTaskRequest {
            scope,
            request_key: "request".into(),
            source_sequence: inbox[0].message.sequence,
            definition: TaskDefinition {
                title: "Implement".into(),
                ..TaskDefinition::default()
            },
        })
        .await
        .unwrap();
    assert_eq!(intent.activation, "pending");
    let claim = f
        .store
        .claim_verified_task_notice(60_000)
        .await
        .unwrap()
        .unwrap();
    (claim, intent.task_id)
}
#[tokio::test]
async fn native_matrix_outgoing_plain_notice_activates_only_after_real_acceptance() {
    let (f, mut fake, c) = ready_named(false, false, "notice bootstrap", None).await;
    let cancel = CancellationToken::new();
    let (claim, task_id) = notice_claim(&f, &mut fake, &c).await;
    let (r, body) = scripted(
        "notice send",
        None,
        c.send_notice(claim.clone(), &cancel),
        async {
            let request = plain_wire(&mut fake).await;
            let body: Value = serde_json::from_slice(&request.body).unwrap();
            request.json(200, json!({"event_id":"$notice"}));
            body
        },
    )
    .await;
    assert_eq!(r.unwrap().state, OutgoingState::Delivered);
    assert_eq!(body["msgtype"], "m.notice");
    assert_eq!(body["m.relates_to"]["event_id"], "$question");
    let row: (String, String) =
        rusqlite::Connection::open(f.root.path().join("domain/domain.sqlite3"))
            .unwrap()
            .query_row(
                "SELECT state,anchor_event_id FROM task_intents WHERE task_id=?1",
                [task_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
    assert_eq!(row, ("active".into(), "$notice".into()));
    observed(Trace::new("notice close", None, None), c.close())
        .await
        .unwrap();
    f.store.shutdown().await.unwrap();
    fake.close().await;
}

#[tokio::test]
async fn native_matrix_outgoing_crypto_real_verified_dm_and_group_ciphertext() {
    for direct in [true, false] {
        let (f, mut fake, c) = ready(true, direct).await;
        let claim = final_claim(&f).await;
        let peer = c
            .inner
            .owner
            .lock()
            .await
            .as_ref()
            .unwrap()
            .outgoing_fixture(true)
            .await;
        let cancel = CancellationToken::new();
        let (r, plain) = common::scripted(c.send_final(claim, &cancel), async {
            preflight(&mut fake, true).await;
            let query = fake.next().await;
            assert_eq!(query.method, "POST");
            assert_eq!(query.target, "/_matrix/client/v3/keys/query");
            query.json(200, peer.query.clone());
            preflight(&mut fake, true).await;
            fake.next().await.json(200, peer.query.clone());
            let share = fake.next().await;
            assert_eq!(share.method, "PUT");
            assert!(share.target.contains("/sendToDevice/m.room.encrypted/"));
            peer.share(serde_json::from_slice(&share.body).unwrap())
                .await;
            share.json(200, json!({}));
            preflight(&mut fake, true).await;
            fake.next().await.json(200, peer.query.clone());
            let message = fake.next().await;
            assert_eq!(message.method, "PUT");
            assert!(message.target.contains("/send/m.room.encrypted/"));
            let value: Value = serde_json::from_slice(&message.body).unwrap();
            assert!(value.get("body").is_none());
            let plain = peer.decrypt(value).await;
            message.json(200, json!({"event_id":"$encrypted"}));
            plain
        })
        .await;
        assert_eq!(r.unwrap().state, OutgoingState::Delivered);
        assert_eq!(plain["content"]["body"], "Answer **verified** 中文");
        assert!(
            plain["content"]["formatted_body"]
                .as_str()
                .unwrap()
                .contains("<strong>verified</strong>")
        );
        c.close().await.unwrap();
        f.store.shutdown().await.unwrap();
        fake.close().await;
    }
}
#[tokio::test]
async fn native_matrix_outgoing_crypto_unverified_missing_or_changed_keys_never_fall_back() {
    for variant in [
        "unverified",
        "own_missing",
        "changed_before_share",
        "malformed_master",
        "malformed_signing",
        "bad_device_signature",
    ] {
        let (f, mut fake, c) =
            ready_named(true, true, "crypto refusal bootstrap", Some(variant)).await;
        let claim = final_claim(&f).await;
        let peer = c
            .inner
            .owner
            .lock()
            .await
            .as_ref()
            .unwrap()
            .outgoing_fixture(variant != "unverified")
            .await;
        let cancel = CancellationToken::new();
        let (r, ()) = scripted(
            "crypto refusal send",
            Some(variant),
            c.send_final(claim, &cancel),
            async {
                preflight(&mut fake, true).await;
                let mut keys = peer.query.clone();
                if variant == "own_missing" {
                    keys["device_keys"]["@worker:example.test"]
                        .as_object_mut()
                        .unwrap()
                        .clear();
                }
                if variant == "malformed_master" {
                    keys["master_keys"]["@owner:example.test"] = json!({});
                }
                if variant == "malformed_signing" {
                    keys["self_signing_keys"]["@owner:example.test"] = json!({});
                }
                if variant == "bad_device_signature" {
                    keys["device_keys"]["@owner:example.test"]["HUMAN"]["signatures"] = json!({});
                }
                fake.next().await.json(200, keys);
                if variant == "changed_before_share" {
                    preflight(&mut fake, true).await;
                    let mut changed = peer.query.clone();
                    changed["device_keys"]["@owner:example.test"]
                        .as_object_mut()
                        .unwrap()
                        .clear();
                    fake.next().await.json(200, changed);
                }
            },
        )
        .await;
        assert!(matches!(r, Err(Error::Recipients | Error::Identity)));
        fake.no_request().await;
        assert_eq!(
            observed(
                Trace::new("crypto refusal resume", Some(variant), None),
                c.resume_outgoing_custody(&cancel)
            )
            .await
            .unwrap()
            .state,
            OutgoingState::Uncertain
        );
        observed(
            Trace::new("crypto refusal close", Some(variant), None),
            c.close(),
        )
        .await
        .unwrap();
        common::shutdown_domain(&f.store, "outgoing crypto refusal cleanup").await;
        fake.close().await;
    }
}

#[tokio::test]
async fn native_matrix_outgoing_scope_unsafe_private_snapshot_stops_before_begin() {
    let (f, mut fake, c) = ready(true, true).await;
    let claim = final_claim(&f).await;
    let cancel = CancellationToken::new();
    let (r,())=common::scripted(c.send_final(claim.clone(),&cancel),async {
        fake.next().await.json(200,common::who());
        let mut unsafe_room=room(true);unsafe_room.as_array_mut().unwrap().push(json!({"type":"m.room.member","state_key":"@other:example.test","content":{"membership":"join"}}));
        fake.next().await.json(200,unsafe_room);
    }).await;
    assert!(r.is_err());
    fake.no_request().await;
    assert!(
        !f.store
            .matrix_room_state(
                f.identity.transport.engagement_id.clone(),
                "!project:example.test".into()
            )
            .await
            .unwrap()
            .unwrap()
            .available
    );
    assert_ne!(state(&f, &claim.id), "sending");
    c.close().await.unwrap();
    f.store.shutdown().await.unwrap();
    fake.close().await;
}
#[tokio::test]
async fn native_matrix_outgoing_scope_wrong_claim_secret_cannot_send() {
    let (f, mut fake, c) = ready(false, false).await;
    let mut claim = final_claim(&f).await;
    let cancel = CancellationToken::new();
    claim.secret = "wrong".into();
    assert!(c.send_final(claim, &cancel).await.is_err());
    fake.no_request().await;
    assert!(f.available().await);
    c.close().await.unwrap();
    f.store.shutdown().await.unwrap();
    fake.close().await;
}
#[tokio::test]
async fn native_matrix_outgoing_bounds_wire_failures_retain_possible_writes() {
    for kind in [
        "duplicate",
        "coalesced",
        "oversize",
        "slow_headers",
        "slow_body",
        "redirect",
        "forbidden",
        "server_error",
    ] {
        let (f, mut fake, c) =
            ready_named(false, false, "wire refusal bootstrap", Some(kind)).await;
        let claim = final_claim(&f).await;
        let cancel = CancellationToken::new();
        let (r,())=scripted("wire refusal send", Some(kind),c.send_final(claim.clone(),&cancel),async {
            let req=plain_wire(&mut fake).await;
            match kind {
                "duplicate"=>req.raw(common::response(200,b"{\"event_id\":\"$one\",\"event_id\":\"$two\"}")),
                "coalesced"=>req.raw(common::response(200,b"{\"event_id\":\"$one\"} {}")),
                "oversize"=>req.raw(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 1048577\r\nConnection: close\r\n\r\n".to_vec()),
                "slow_headers"=>req.chunks(vec![(Duration::from_millis(700),common::response(200,b"{\"event_id\":\"$one\"}"))]),
                "slow_body"=>req.chunks(vec![(Duration::ZERO,b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 19\r\nConnection: close\r\n\r\n".to_vec()),(Duration::from_millis(400),b"{\"event_id\":\"$one\"}".to_vec())]),
                "redirect"=>req.raw(b"HTTP/1.1 307 Temporary Redirect\r\nLocation: https://invalid.example/secret\r\nContent-Length: 0\r\n\r\n".to_vec()),
                "forbidden"=>req.json(403,json!({"errcode":"M_FORBIDDEN"})),
                _=>req.json(500,json!({"errcode":"M_UNKNOWN"})),
            }
        }).await;
        assert!(r.is_err(), "{kind}");
        assert_eq!(state(&f, &claim.id), "sending");
        assert_eq!(
            observed(
                Trace::new("wire refusal resume", Some(kind), None),
                c.resume_outgoing_custody(&cancel)
            )
            .await
            .unwrap()
            .state,
            OutgoingState::Uncertain
        );
        fake.no_request().await;
        observed(
            Trace::new("wire refusal close", Some(kind), None),
            c.close(),
        )
        .await
        .unwrap();
        common::shutdown_domain(&f.store, "outgoing wire refusal cleanup").await;
        fake.close().await;
    }
}
#[tokio::test]
async fn native_matrix_outgoing_recovery_actual_journal_rollback_does_not_publish_memory_only_acceptance()
 {
    let (f, mut fake, c) = ready(false, false).await;
    let claim = final_claim(&f).await;
    let cancel = CancellationToken::new();
    let sql =
        rusqlite::Connection::open(f.root.path().join("sdk/matrix-sdk-state.sqlite3")).unwrap();
    let (r,())=common::scripted(c.send_final(claim.clone(),&cancel),async {
        let req=plain_wire(&mut fake).await;
        sql.execute_batch("CREATE TRIGGER reject_outgoing_journal BEFORE INSERT ON kv_blob BEGIN SELECT RAISE(ABORT,'fixture rollback'); END;").unwrap();
        req.json(200,json!({"event_id":"$accepted_but_storage_failed"}));
    }).await;
    assert_eq!(r, Err(Error::OutcomeUnknown));
    assert_eq!(state(&f, &claim.id), "sending");
    assert_eq!(
        c.resume_outgoing_custody(&cancel).await,
        Err(Error::OutcomeUnknown)
    );
    sql.execute_batch("DROP TRIGGER reject_outgoing_journal")
        .unwrap();
    drop(sql);
    stop_sdk(c).await;
    let c = Collector::new(
        config(&f, &fake.endpoint, f.identity.clone(), false),
        f.store.clone(),
    )
    .unwrap();
    assert_eq!(
        c.resume_outgoing_custody(&cancel).await.unwrap().state,
        OutgoingState::Uncertain
    );
    fake.no_request().await;
    c.close().await.unwrap();
    f.store.shutdown().await.unwrap();
    fake.close().await;
}

#[tokio::test]
async fn native_matrix_outgoing_scope_retirement_after_possible_persist_prevents_http() {
    for retire in [false, true] {
        let (f, mut fake, c) = ready(false, false).await;
        let claim = final_claim(&f).await;
        let cancel = CancellationToken::new();
        c.inner.outgoing_fault.store(5, Ordering::SeqCst);
        let (r, ()) = common::scripted(c.send_final(claim.clone(), &cancel), async {
            preflight(&mut fake, false).await;
            preflight(&mut fake, false).await;
            c.inner.outgoing_reached.notified().await;
            assert_eq!(c.resume_outgoing_custody(&cancel).await, Err(Error::Busy));
            if retire {
                f.store
                    .invalidate_matrix_transport(MatrixTransportInvalidation {
                        expected: f.identity.transport.clone(),
                        reason: "controlled retirement".into(),
                    })
                    .await
                    .unwrap();
            } else {
                f.store.cancel_final_reply(claim.id.clone()).await.unwrap();
            }
            c.inner.outgoing_continue.notify_one();
        })
        .await;
        assert!(r.is_err());
        fake.no_request().await;
        assert_eq!(
            c.resume_outgoing_custody(&cancel).await.unwrap().state,
            OutgoingState::Uncertain
        );
        c.close().await.unwrap();
        f.store.shutdown().await.unwrap();
        fake.close().await;
    }
}
#[tokio::test]
async fn native_matrix_outgoing_scope_failed_whoami_retires_cached_route() {
    let (f, mut fake, c) = ready(false, false).await;
    let claim = final_claim(&f).await;
    let cancel = CancellationToken::new();
    let (r, ()) = common::scripted(c.send_final(claim, &cancel), async {
        fake.next().await.json(
            200,
            json!({"user_id":"@other:example.test","device_id":"OTHER","is_guest":false}),
        );
    })
    .await;
    assert_eq!(r, Err(Error::Identity));
    assert!(!f.available().await);
    fake.no_request().await;
    c.close().await.unwrap();
    f.store.shutdown().await.unwrap();
    fake.close().await;
}

#[tokio::test]
async fn native_matrix_outgoing_bounds_real_receipts_stop_capacity_without_eviction() {
    let (f, mut fake, c) = ready(false, false).await;
    let cancel = CancellationToken::new();
    let mut first = None;
    for i in 0..state::MAX_RECEIPTS {
        let claim = final_claim_named(&f, &format!("task{i}")).await;
        if first.is_none() {
            first = Some(claim.clone());
        }
        let (r, ()) = common::scripted(c.send_final(claim, &cancel), async {
            plain_wire(&mut fake)
                .await
                .json(200, json!({"event_id":format!("$accepted{i}")}));
        })
        .await;
        assert_eq!(r.unwrap().state, OutgoingState::Delivered);
    }
    let next = final_claim_named(&f, "full").await;
    assert_eq!(
        c.send_final(next.clone(), &cancel).await,
        Err(Error::Capacity)
    );
    fake.no_request().await;
    assert_eq!(state(&f, &next.id), "claimed");
    assert!(
        c.send_final(first.unwrap(), &cancel)
            .await
            .unwrap()
            .replayed
    );
    stop_sdk(c).await;
    let c = Collector::new(
        config(&f, &fake.endpoint, f.identity.clone(), false),
        f.store.clone(),
    )
    .unwrap();
    assert_eq!(
        c.resume_outgoing_custody(&cancel).await.unwrap().state,
        OutgoingState::Idle
    );
    assert_eq!(c.send_final(next, &cancel).await, Err(Error::Capacity));
    fake.no_request().await;
    c.close().await.unwrap();
    f.store.shutdown().await.unwrap();
    fake.close().await;
}

async fn encrypted_wire(fake: &mut common::Fake, query: &Value) -> common::Request {
    preflight(fake, true).await;
    fake.next().await.json(200, query.clone());
    preflight(fake, true).await;
    fake.next().await.json(200, query.clone());
    let share = fake.next().await;
    assert!(share.target.contains("/sendToDevice/m.room.encrypted/"));
    share.json(200, json!({}));
    preflight(fake, true).await;
    fake.next().await.json(200, query.clone());
    let message = fake.next().await;
    assert!(message.target.contains("/send/m.room.encrypted/"));
    message
}
#[tokio::test]
async fn native_matrix_outgoing_recovery_restore_rejects_inconsistent_protected_history() {
    for variant in 0..9 {
        let variant_label = Some(
            [
                "history 0",
                "history 1",
                "history 2",
                "history 3",
                "history 4",
                "history 5",
                "history 6",
                "history 7",
                "history 8",
            ][usize::from(variant)],
        );
        let (f, mut fake, c) =
            ready_named(true, false, "protected history bootstrap", variant_label).await;
        let claim = final_claim(&f).await;
        let peer = c
            .inner
            .owner
            .lock()
            .await
            .as_ref()
            .unwrap()
            .outgoing_fixture(true)
            .await;
        c.inner.outgoing_fault.store(1, Ordering::SeqCst);
        let cancel = CancellationToken::new();
        let (r, ()) = scripted(
            "protected history send",
            variant_label,
            c.send_final(claim.clone(), &cancel),
            async {
                encrypted_wire(&mut fake, &peer.query)
                    .await
                    .json(200, json!({"event_id":"$accepted"}));
            },
        )
        .await;
        assert_eq!(r, Err(Error::Busy));
        c.inner
            .owner
            .lock()
            .await
            .as_ref()
            .unwrap()
            .corrupt_outgoing_fixture(variant)
            .await;
        stop_sdk(c).await;
        let c = Collector::new(
            config(&f, &fake.endpoint, f.identity.clone(), false),
            f.store.clone(),
        )
        .unwrap();
        assert_eq!(
            observed(
                Trace::new("protected history resume", variant_label, None),
                c.resume_outgoing_custody(&cancel)
            )
            .await,
            Err(Error::Storage),
            "variant {variant}"
        );
        assert_eq!(state(&f, &claim.id), "sending");
        fake.no_request().await;
        observed(
            Trace::new("protected history close", variant_label, None),
            c.close(),
        )
        .await
        .unwrap();
        f.store.shutdown().await.unwrap();
        fake.close().await;
    }
}
#[tokio::test]
async fn native_matrix_outgoing_domain_late_notice_acceptance_never_activates_retired_task() {
    let (f, mut fake, c) = ready(false, false).await;
    let (claim, task_id) = notice_claim(&f, &mut fake, &c).await;
    let cancel = CancellationToken::new();
    c.inner.outgoing_fault.store(3, Ordering::SeqCst);
    let (r, ()) = common::scripted(c.send_notice(claim.clone(), &cancel), async {
        plain_wire(&mut fake)
            .await
            .json(200, json!({"event_id":"$late_notice"}));
        c.inner.outgoing_reached.notified().await;
        f.store
            .invalidate_matrix_transport(MatrixTransportInvalidation {
                expected: f.identity.transport.clone(),
                reason: "controlled retirement".into(),
            })
            .await
            .unwrap();
        cancel.cancel();
        c.inner.outgoing_continue.notify_one();
    })
    .await;
    assert_eq!(r.unwrap().state, OutgoingState::Delivered);
    let row: (String, Option<String>) =
        rusqlite::Connection::open(f.root.path().join("domain/domain.sqlite3"))
            .unwrap()
            .query_row(
                "SELECT state,anchor_event_id FROM task_intents WHERE task_id=?1",
                [task_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
    assert_ne!(row.0, "active");
    assert!(row.1.is_none());
    assert!(!f.available().await);
    assert_eq!(
        f.store
            .verified_notice_receipt(claim.claim.notice.id)
            .await
            .unwrap()
            .state,
        "delivered"
    );
    fake.no_request().await;
    c.close().await.unwrap();
    f.store.shutdown().await.unwrap();
    fake.close().await;
}
#[tokio::test]
async fn native_matrix_outgoing_crypto_lost_key_share_response_never_sends_room_ciphertext() {
    let (f, mut fake, c) = ready(true, true).await;
    let claim = final_claim(&f).await;
    let peer = c
        .inner
        .owner
        .lock()
        .await
        .as_ref()
        .unwrap()
        .outgoing_fixture(true)
        .await;
    let cancel = CancellationToken::new();
    let (r, ()) = common::scripted(c.send_final(claim.clone(), &cancel), async {
        preflight(&mut fake, true).await;
        fake.next().await.json(200, peer.query.clone());
        preflight(&mut fake, true).await;
        fake.next().await.json(200, peer.query.clone());
        let share = fake.next().await;
        assert!(share.target.contains("/sendToDevice/m.room.encrypted/"));
        peer.share(serde_json::from_slice(&share.body).unwrap())
            .await;
        drop(share);
    })
    .await;
    assert!(r.is_err());
    assert_eq!(state(&f, &claim.id), "sending");
    stop_sdk(c).await;
    let c = Collector::new(
        config(&f, &fake.endpoint, f.identity.clone(), true),
        f.store.clone(),
    )
    .unwrap();
    assert_eq!(
        c.resume_outgoing_custody(&cancel).await.unwrap().state,
        OutgoingState::Uncertain
    );
    fake.no_request().await;
    assert_eq!(
        c.send_final(claim, &cancel).await,
        Err(Error::OutcomeUnknown)
    );
    fake.no_request().await;
    c.close().await.unwrap();
    f.store.shutdown().await.unwrap();
    fake.close().await;
}

#[tokio::test]
async fn native_matrix_outgoing_recovery_sdk_accept_response_lost_still_settles_without_send() {
    let (f, mut fake, c) = ready(false, false).await;
    let claim = final_claim(&f).await;
    c.inner
        .owner
        .lock()
        .await
        .as_ref()
        .unwrap()
        .outgoing_reply_fault()
        .await;
    let cancel = CancellationToken::new();
    let (r, ()) = common::scripted(c.send_final(claim.clone(), &cancel), async {
        plain_wire(&mut fake)
            .await
            .json(200, json!({"event_id":"$accepted"}));
    })
    .await;
    assert_eq!(r, Err(Error::OutcomeUnknown));
    assert_eq!(state(&f, &claim.id), "sending");
    stop_sdk(c).await;
    let c = Collector::new(
        config(&f, &fake.endpoint, f.identity.clone(), false),
        f.store.clone(),
    )
    .unwrap();
    assert_eq!(
        c.resume_outgoing_custody(&cancel).await.unwrap().state,
        OutgoingState::Delivered
    );
    assert_eq!(state(&f, &claim.id), "delivered");
    fake.no_request().await;
    c.close().await.unwrap();
    f.store.shutdown().await.unwrap();
    fake.close().await;
}

#[tokio::test]
async fn native_matrix_outgoing_domain_changed_receipt_or_fence_cannot_settle() {
    let (f, mut fake, c) = ready(false, false).await;
    let claim = final_claim(&f).await;
    let cancel = CancellationToken::new();
    c.inner.outgoing_fault.store(1, Ordering::SeqCst);
    let (r, ()) = common::scripted(c.send_final(claim.clone(), &cancel), async {
        plain_wire(&mut fake)
            .await
            .json(200, json!({"event_id":"$accepted"}));
    })
    .await;
    assert_eq!(r, Err(Error::Busy));
    let actual = c
        .inner
        .owner
        .lock()
        .await
        .as_ref()
        .unwrap()
        .outgoing(Command::Read)
        .await
        .unwrap()
        .attempt
        .unwrap()
        .observation()
        .unwrap();
    assert!(
        f.store
            .reconcile_final_reply(
                claim.id.clone(),
                claim.fence + 1,
                ReplyReconciliation::Delivered(actual.clone())
            )
            .await
            .is_err()
    );
    assert_eq!(state(&f, &claim.id), "sending");
    let mut changed_route = actual.clone();
    changed_route.room_id = "!other:example.test".into();
    assert!(
        f.store
            .reconcile_final_reply(
                claim.id.clone(),
                claim.fence,
                ReplyReconciliation::Delivered(changed_route)
            )
            .await
            .is_err()
    );
    assert_eq!(state(&f, &claim.id), "sending");
    assert_eq!(
        c.resume_outgoing_custody(&cancel).await.unwrap().state,
        OutgoingState::Delivered
    );
    let mut changed_receipt = actual;
    changed_receipt.event_id = "$substitute".into();
    assert!(
        f.store
            .reconcile_final_reply(
                claim.id.clone(),
                claim.fence,
                ReplyReconciliation::Delivered(changed_receipt)
            )
            .await
            .is_err()
    );
    assert_eq!(state(&f, &claim.id), "delivered");
    assert!(f.available().await);
    fake.no_request().await;
    c.close().await.unwrap();
    f.store.shutdown().await.unwrap();
    fake.close().await;
}
