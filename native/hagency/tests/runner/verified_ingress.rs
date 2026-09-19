use super::*;
use hagency_core::{ingress::*, replies::*, task_intents::*};
use std::collections::BTreeSet;

#[tokio::test]
async fn native_runner_http_verified_ingress() {
    for direct in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("state");
        let custody = Store::start(Repository::open(&state).unwrap(), 16).unwrap();
        let mut db = DomainRepository::open(&state).unwrap();
        db.register(&registration()).unwrap();
        let pool = resource("pool", "seat", 1000);
        db.put_resource(&pool).unwrap();
        let proof = proof(&request("request", "Worker", &pool, 100));
        let e = db.admit(&proof, 1000).unwrap();
        db.approve("approve", &proof, 1000).unwrap();
        let effect = db.claim_effect().unwrap().unwrap();
        db.observe_effect(
            &effect.id,
            effect.fence,
            &EffectOutcome::Applied {
                receipt: "fixture account observation".into(),
            },
        )
        .unwrap();
        let room = if direct {
            "!dm:example.test"
        } else {
            "!project:example.test"
        };
        db.observe_matrix_transport(
            &MatrixTransportObservation {
                engagement_id: e.id.clone(),
                registration_generation: 1,
                generation: 1,
                sender_mxid: "@worker:example.test".into(),
                device_id: "DEVICE".into(),
            },
            now(),
        )
        .unwrap();
        db.observe_matrix_room(
            &MatrixRoomObservation {
                engagement_id: e.id.clone(),
                registration_generation: 1,
                transport_generation: 1,
                room_id: room.into(),
                generation: 1,
                privacy: if direct {
                    RoomPrivacy::Direct {
                        human_mxid: "@owner:example.test".into(),
                    }
                } else {
                    RoomPrivacy::Group {}
                },
                joined: BTreeSet::from([
                    "@owner:example.test".into(),
                    "@worker:example.test".into(),
                ]),
                invite_only: true,
                encrypted: direct,
            },
            now(),
        )
        .unwrap();
        db.resolve_verified_matrix_session(
            &SessionBinding {
                id: "main".into(),
                engagement_id: e.id,
                room_id: room.into(),
                thread_root: None,
            },
            now(),
        )
        .unwrap();
        let domain = DomainStore::start(db, 16).unwrap();
        let service = Service::new(
            App::new(
                custody.clone(),
                TOKEN.as_bytes(),
                "127.0.0.1:13300".parse().unwrap(),
            )
            .unwrap()
            .with_domain(domain.clone())
            .router(),
        );
        let scope = domain.matrix_ingress_scope("main".into()).await.unwrap();
        let event = MatrixEventObservation {
            scope: scope.clone(),
            event: InboundMessage {
                server_name: "example.test".into(),
                room_id: room.into(),
                event_id: "$actual_human_request".into(),
                sender_mxid: "@owner:example.test".into(),
                thread_root: None,
                body: "Implement and verify the requested work".into(),
                kind: "m.text".into(),
                origin_ts: now(),
            },
            mentions: if direct {
                BTreeSet::new()
            } else {
                BTreeSet::from(["@worker:example.test".into()])
            },
            encrypted: direct,
        };
        let source = domain.admit_matrix_event(event.clone()).await.unwrap();
        assert!(source.wake);
        let intent = domain
            .create_verified_task_intent(VerifiedTaskRequest {
                scope,
                request_key: "human_request".into(),
                source_sequence: source.sequence,
                definition: TaskDefinition {
                    title: "Actual host-admitted task".into(),
                    ..Default::default()
                },
            })
            .await
            .unwrap();
        assert_eq!(intent.activation, "pending");
        let input = DispatchInput {
            id: "work".into(),
            session_id: intent.session_id.clone(),
            task_id: Some(intent.task_id.clone()),
            resources: vec![],
            payload: json!({"instruction":"Use only the admitted task input"}),
        };
        assert!(
            domain
                .enqueue_inbox_dispatch(input.clone(), vec![source.sequence])
                .await
                .is_err()
        );
        let claim = domain
            .claim_verified_task_notice(60_000)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(claim.source_event_id, "$actual_human_request");
        assert_eq!(
            claim.route.thread_root,
            if direct {
                None
            } else {
                Some("$actual_human_request".into())
            }
        );
        let observation = ReplyDeliveryObservation {
            transaction_id: claim.claim.notice.transaction_id.clone(),
            digest: claim.digest.clone(),
            server_name: claim.route.server_name.clone(),
            room_id: claim.route.room_id.clone(),
            sender_mxid: claim.route.sender_mxid.clone(),
            device_id: claim.route.device_id.clone(),
            event_id: "$actual_anchor_observation".into(),
            encrypted: direct,
        };
        domain
            .begin_verified_task_notice_send(
                claim.claim.notice.id.clone(),
                claim.claim.token.clone(),
            )
            .await
            .unwrap();
        domain
            .deliver_verified_task_notice(claim.claim.notice.id, claim.claim.token, observation)
            .await
            .unwrap();
        domain
            .enqueue_inbox_dispatch(input, vec![source.sequence])
            .await
            .unwrap();
        let cap = domain
            .claim_dispatch("runner".into(), now(), 120_000, 120_000, 8)
            .await
            .unwrap()
            .unwrap();
        domain.start_dispatch(cap.clone(), now()).await.unwrap();
        let mut response = get("inbox?after=0&limit=100", &cap).send(&service).await;
        assert_eq!(response.status_code, Some(StatusCode::OK));
        let inbox: Value = response.take_json().await.unwrap();
        assert_eq!(inbox[0]["message"]["body"], event.event.body);
        for path in [
            "matrix/events",
            "matrix/ingress",
            "matrix/sessions",
            "matrix/mentions",
            "task-intents",
            "task-notices/claim",
            "task-notices/deliver",
            "task-notices/begin",
            "task-notices/inspect",
            "task-notices/cancel",
        ] {
            assert_eq!(auth(TestClient::post(format!("{BASE}/runner/{path}")),&cap).json(&json!({"verified":true,"wake":true,"session_id":"main","room_id":room,"device_id":"DEVICE"})).send(&service).await.status_code,Some(StatusCode::NOT_FOUND));
        }
        for field in [
            "wake",
            "target",
            "scope",
            "device_id",
            "room_id",
            "verified",
            "delivered",
        ] {
            let mut body = json!({"call_id":"forged","body":"Cannot assert transport"});
            body[field] = json!(true);
            assert_eq!(
                auth(
                    TestClient::post(format!("{BASE}/runner/final-replies")),
                    &cap
                )
                .json(&body)
                .send(&service)
                .await
                .status_code,
                Some(StatusCode::BAD_REQUEST)
            );
        }
        assert_eq!(
            auth(
                TestClient::post(format!("{BASE}/runner/final-replies")),
                &cap
            )
            .json(&json!({"call_id":"early","body":"Not canonical completion"}))
            .send(&service)
            .await
            .status_code,
            Some(StatusCode::CONFLICT)
        );
        let mut done = post(
            &intent.task_id,
            &cap,
            &json!({"call_id":"done","operation":{"action":"transition","status":"done"}}),
        )
        .send(&service)
        .await;
        assert_eq!(done.status_code, Some(StatusCode::OK));
        let completed: Value = done.take_json().await.unwrap();
        assert_eq!(completed["task"]["status"], "done");
        let mut reply = auth(
            TestClient::post(format!("{BASE}/runner/final-replies")),
            &cap,
        )
        .json(&json!({"call_id":"final","body":"Verified canonical result"}))
        .send(&service)
        .await;
        assert_eq!(reply.status_code, Some(StatusCode::OK));
        let result: Value = reply.take_json().await.unwrap();
        assert_eq!(result["task_id"], intent.task_id);
        assert_eq!(
            result["execution_epoch"],
            completed["task"]["execution_epoch"]
        );
        let transport = domain.claim_final_reply(60_000).await.unwrap().unwrap();
        let send = domain.begin_final_reply_send(transport).await.unwrap();
        assert_eq!(send.id, result["id"].as_str().unwrap());
        assert_eq!(send.route.session_id, intent.session_id);
        assert_eq!(send.route.room_id, room);
        assert_eq!(
            send.route.thread_root,
            if direct {
                None
            } else {
                Some("$actual_human_request".into())
            }
        );
        domain.shutdown().await.unwrap();
        custody.shutdown().await.unwrap();
    }
}
