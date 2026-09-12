mod common;
use common::*;
use hagency_core::{replies::*, tasks::*};
use hagency_store::{DomainRepository, EffectOutcome, OwnedClaimProfile, OwnedClaimRoom};
use serde_json::json;
use std::collections::BTreeSet;

struct Fixture {
    _root: tempfile::TempDir,
    db: DomainRepository,
    transport: MatrixTransportObservation,
}
impl Fixture {
    fn new() -> Self {
        Self::with_runtime("codex", None, "medium")
    }
    fn with_runtime(framework: &str, provider: Option<&str>, reasoning: &str) -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut db = DomainRepository::open(&root.path().join("state")).unwrap();
        db.register(&registration()).unwrap();
        let mut pool = resource("pool", "seat", 1000);
        pool.framework = framework.into();
        if framework == "claude" {
            pool.model = "claude-opus-5".into();
        }
        pool.provider = provider.map(str::to_owned);
        pool.reasoning = Some(reasoning.into());
        db.put_resource(&pool).unwrap();
        let proof = proof(&request("claim", "Worker", &pool, 100));
        let e = db.admit(&proof, 1000).unwrap();
        db.approve("approve", &proof, 1000).unwrap();
        let effect = db.claim_effect().unwrap().unwrap();
        db.observe_effect(
            &effect.id,
            effect.fence,
            &EffectOutcome::Applied {
                receipt: "fixture".into(),
            },
        )
        .unwrap();
        let transport = MatrixTransportObservation {
            engagement_id: e.id.clone(),
            registration_generation: 1,
            generation: 1,
            sender_mxid: "@worker:example.test".into(),
            device_id: "DEVICE_1".into(),
        };
        db.observe_matrix_transport(&transport, 1001).unwrap();
        db.observe_matrix_room(
            &MatrixRoomObservation {
                engagement_id: e.id,
                registration_generation: 1,
                transport_generation: 1,
                room_id: "!project:example.test".into(),
                generation: 1,
                privacy: RoomPrivacy::Group {},
                joined: BTreeSet::from([
                    "@worker:example.test".into(),
                    "@owner:example.test".into(),
                ]),
                invite_only: true,
                encrypted: true,
            },
            1002,
        )
        .unwrap();
        db.register_workspace("work").unwrap();
        db.register_workspace("other").unwrap();
        Self {
            _root: root,
            db,
            transport,
        }
    }
    fn queue(&mut self, id: &str, workspace: &str) {
        self.queue_with(id, workspace, true);
    }
    fn queue_with(&mut self, id: &str, workspace: &str, exclusive: bool) {
        self.db
            .resolve_verified_matrix_session(
                &SessionBinding {
                    id: id.into(),
                    engagement_id: self.transport.engagement_id.clone(),
                    room_id: "!project:example.test".into(),
                    thread_root: Some(format!("${id}")),
                },
                1003,
            )
            .unwrap();
        self.db
            .create_canonical_task(id, id, "Host claim task", 1004)
            .unwrap();
        self.db
            .enqueue_dispatch(&DispatchInput {
                id: id.into(),
                session_id: id.into(),
                task_id: Some(id.into()),
                resources: vec![ResourceLease {
                    id: workspace.into(),
                    exclusive,
                }],
                payload: json!({"instruction":"fixture"}),
            })
            .unwrap();
    }
    fn profile(&self, device: &str, privacy: RoomPrivacy) -> OwnedClaimProfile {
        let mut t = self.transport.clone();
        t.device_id = device.into();
        OwnedClaimProfile::new(
            t,
            vec![OwnedClaimRoom::new("!project:example.test".into(), 1, privacy).unwrap()],
            vec!["work".into()],
        )
        .unwrap()
    }
}
#[test]
fn native_owned_claim_profile() {
    let mut f = Fixture::new();
    f.queue("first_incompatible", "other");
    f.queue("selected", "work");
    let wrong = f.profile("OTHER", RoomPrivacy::Group {});
    assert!(
        f.db.claim_owned_dispatch_for_host(&wrong, "host", 2000, 60000, 60000, 1)
            .unwrap()
            .is_none()
    );
    let wrong = f.profile(
        "DEVICE_1",
        RoomPrivacy::Direct {
            human_mxid: "@owner:example.test".into(),
        },
    );
    assert!(
        f.db.claim_owned_dispatch_for_host(&wrong, "host", 2000, 60000, 60000, 1)
            .unwrap()
            .is_none()
    );
    let profile = f.profile("DEVICE_1", RoomPrivacy::Group {});
    let cap =
        f.db.claim_owned_dispatch_for_host(&profile, "host", 2000, 60000, 60000, 1)
            .unwrap()
            .unwrap();
    assert_eq!(cap.dispatch_id, "selected");
    assert!(
        f.db.claim_owned_dispatch_for_host(&profile, "host", 2001, 60000, 60000, 1)
            .unwrap()
            .is_none()
    );
    let scope = f.db.owned_dispatch_scope(&cap, 2001).unwrap();
    assert_eq!(scope.input().resources[0].id, "work");
    assert!(scope.input().resources[0].exclusive);
    f.db.invalidate_matrix_transport(
        &MatrixTransportInvalidation {
            expected: f.transport.clone(),
            reason: "actual negative host observation".into(),
        },
        2002,
    )
    .unwrap();
    assert!(
        f.db.start_owned_dispatch(&cap, scope.fingerprint(), 2003)
            .is_err()
    );
    assert!(
        f.db.claim_owned_dispatch_for_host(&profile, "host", 2003, 60000, 60000, 1)
            .unwrap()
            .is_none()
    );
}

#[test]
fn native_owned_claim_profile_bounds() {
    let f = Fixture::new();
    assert!(OwnedClaimProfile::new(f.transport.clone(), vec![], vec!["work".into()]).is_err());
    assert!(
        OwnedClaimProfile::new(
            f.transport.clone(),
            vec![
                OwnedClaimRoom::new("!project:example.test".into(), 1, RoomPrivacy::Group {})
                    .unwrap()
            ],
            vec!["work".into(), "work".into()]
        )
        .is_err()
    );
    assert!(OwnedClaimRoom::new("!project:example.test".into(), 0, RoomPrivacy::Group {}).is_err());
}

#[test]
fn native_owned_claim_profile_unsupported_and_shared_lease() {
    {
        let mut f = Fixture::with_runtime("claude", None, "medium");
        f.queue("unsupported", "work");
        let profile = f.profile("DEVICE_1", RoomPrivacy::Group {});
        assert!(
            f.db.claim_owned_dispatch_for_host(&profile, "host", 2000, 60_000, 60_000, 1)
                .unwrap()
                .is_none()
        );
    }
    let mut f = Fixture::new();
    f.queue_with("readonly", "work", false);
    f.queue("exclusive", "work");
    let profile = f.profile("DEVICE_1", RoomPrivacy::Group {});
    assert_eq!(
        f.db.claim_owned_dispatch_for_host(&profile, "host", 2000, 60_000, 60_000, 1)
            .unwrap()
            .unwrap()
            .dispatch_id,
        "exclusive"
    );
}

#[test]
fn native_owned_claim_profile_legacy_error_classification() {
    let mut f = Fixture::new();
    assert!(matches!(
        f.db.claim_dispatch("host", hagency_core::JSON_SAFE_MAX, 1, 1, 1),
        Err(hagency_store::Error::Invalid(hagency_core::InvalidInput(
            "invalid dispatch lease limits"
        )))
    ));
}

#[test]
fn native_owned_claim_profile_done_followup_stays_queued() {
    use hagency_core::{ingress::*, messages::InboundMessage, task_intents::TaskDefinition};
    let mut f = Fixture::new();
    f.db.resolve_verified_matrix_session(
        &SessionBinding {
            id: "main".into(),
            engagement_id: f.transport.engagement_id.clone(),
            room_id: "!project:example.test".into(),
            thread_root: None,
        },
        1010,
    )
    .unwrap();
    let event = |scope, id: &str, thread: Option<&str>, at| MatrixEventObservation {
        scope,
        event: InboundMessage {
            server_name: "example.test".into(),
            room_id: "!project:example.test".into(),
            event_id: format!("${id}"),
            sender_mxid: "@owner:example.test".into(),
            thread_root: thread.map(str::to_owned),
            body: "Owned follow-up".into(),
            kind: "m.text".into(),
            origin_ts: at,
        },
        mentions: BTreeSet::from(["@worker:example.test".into()]),
        encrypted: true,
    };
    let root =
        f.db.admit_matrix_event(
            &event(
                f.db.matrix_ingress_scope("main").unwrap(),
                "root",
                None,
                1011,
            ),
            1011,
        )
        .unwrap();
    let request = |scope, key: &str, sequence| VerifiedTaskRequest {
        scope,
        request_key: key.into(),
        source_sequence: sequence,
        definition: TaskDefinition {
            title: "Follow-up admission".into(),
            ..Default::default()
        },
    };
    let task =
        f.db.create_verified_task_intent(
            &request(
                f.db.matrix_ingress_scope("main").unwrap(),
                "original",
                root.sequence,
            ),
            1012,
        )
        .unwrap();
    let notice =
        f.db.claim_verified_task_notice(1013, 1000)
            .unwrap()
            .unwrap();
    f.db.begin_verified_task_notice_send(&notice.claim.notice.id, &notice.claim.token, 1013)
        .unwrap();
    f.db.deliver_verified_task_notice(
        &notice.claim.notice.id,
        &notice.claim.token,
        &ReplyDeliveryObservation {
            transaction_id: notice.claim.notice.transaction_id.clone(),
            digest: notice.digest.clone(),
            server_name: notice.route.server_name.clone(),
            room_id: notice.route.room_id.clone(),
            sender_mxid: notice.route.sender_mxid.clone(),
            device_id: notice.route.device_id.clone(),
            encrypted: notice.route.encrypted,
            event_id: "$anchor".into(),
        },
        1014,
    )
    .unwrap();
    let input = |id: &str| DispatchInput {
        id: id.into(),
        session_id: task.session_id.clone(),
        task_id: Some(task.task_id.clone()),
        resources: vec![ResourceLease {
            id: "work".into(),
            exclusive: true,
        }],
        payload: json!({"instruction":id}),
    };
    f.db.enqueue_inbox_dispatch(&input("initial"), &[root.sequence])
        .unwrap();
    let original =
        f.db.claim_dispatch("host", 1015, 60_000, 60_000, 1)
            .unwrap()
            .unwrap();
    f.db.start_dispatch(&original, 1016).unwrap();
    f.db.mutate_task(
        &original,
        &task.task_id,
        "done",
        &TaskMutation::Transition {
            status: TaskState::Done,
            waiting_reason: None,
            waiting_until: None,
        },
        1017,
    )
    .unwrap();
    f.db.complete_dispatch(&original, &json!({"done":true}), 1018)
        .unwrap();
    let next =
        f.db.admit_matrix_event(
            &event(
                f.db.matrix_ingress_scope(&task.session_id).unwrap(),
                "next",
                Some("$root"),
                1020,
            ),
            1021,
        )
        .unwrap();
    f.db.create_verified_task_intent(
        &request(
            f.db.matrix_ingress_scope(&task.session_id).unwrap(),
            "next",
            next.sequence,
        ),
        1022,
    )
    .unwrap();
    f.db.enqueue_inbox_dispatch(&input("followup"), &[next.sequence])
        .unwrap();
    f.queue("compatible", "work");
    let profile = f.profile("DEVICE_1", RoomPrivacy::Group {});
    let owned =
        f.db.claim_owned_dispatch_for_host(&profile, "owned", 2000, 60_000, 60_000, 1)
            .unwrap()
            .unwrap();
    assert_eq!(owned.dispatch_id, "compatible");
    f.db.fail_before_start(&owned, 2001, 10_000).unwrap();
    let general =
        f.db.claim_dispatch("general", 2002, 60_000, 60_000, 1)
            .unwrap()
            .unwrap();
    assert_eq!(general.dispatch_id, "followup");
    assert!(f.db.owned_dispatch_scope(&general, 2002).is_err());
}

#[test]
fn native_owned_claim_profile_recovery_report_stays_queued() {
    let mut f = Fixture::new();
    f.queue("original", "work");
    let original =
        f.db.claim_dispatch("host", 2000, 60_000, 60_000, 1)
            .unwrap()
            .unwrap();
    f.db.start_dispatch(&original, 2001).unwrap();
    f.db.mutate_task(
        &original,
        "original",
        "done",
        &TaskMutation::Transition {
            status: TaskState::Done,
            waiting_reason: None,
            waiting_until: None,
        },
        2002,
    )
    .unwrap();
    // Actual repository restart classifies an unacknowledged Started attempt;
    // explicit stop observations deliberately prohibit this recovery path.
    let Fixture {
        _root,
        db,
        transport,
    } = f;
    drop(db);
    let db = DomainRepository::open(&_root.path().join("state")).unwrap();
    let mut f = Fixture {
        _root,
        db,
        transport,
    };
    f.db.recover_dispatch(
        "original",
        &DispatchInput {
            id: "report".into(),
            session_id: "original".into(),
            task_id: None,
            resources: vec![ResourceLease {
                id: "work".into(),
                exclusive: true,
            }],
            payload: json!({"instruction":"Report the inspected completed artifact"}),
        },
        "Actual offline fixture inspection; no process was spawned",
        2004,
    )
    .unwrap();
    f.queue("compatible", "work");
    let profile = f.profile("DEVICE_1", RoomPrivacy::Group {});
    let owned =
        f.db.claim_owned_dispatch_for_host(&profile, "owned", 2005, 60_000, 60_000, 1)
            .unwrap()
            .unwrap();
    assert_eq!(owned.dispatch_id, "compatible");
    f.db.fail_before_start(&owned, 2006, 10_000).unwrap();
    let general =
        f.db.claim_dispatch("general", 2007, 60_000, 60_000, 1)
            .unwrap()
            .unwrap();
    assert_eq!(general.dispatch_id, "report");
}

#[test]
fn native_receive_inbox_claim_restriction() {
    let mut f = Fixture::new();
    f.queue("first", "work");
    f.queue("selected", "work");
    let absent = f
        .profile("DEVICE_1", RoomPrivacy::Group {})
        .restrict_dispatch("absent".into())
        .unwrap();
    assert!(
        f.db.claim_owned_dispatch_for_host(&absent, "host", 2000, 60_000, 60_000, 1)
            .unwrap()
            .is_none()
    );
    let wrong = f
        .profile("OTHER", RoomPrivacy::Group {})
        .restrict_dispatch("selected".into())
        .unwrap();
    assert!(
        f.db.claim_owned_dispatch_for_host(&wrong, "host", 2000, 60_000, 60_000, 1)
            .unwrap()
            .is_none()
    );
    let selected = f
        .profile("DEVICE_1", RoomPrivacy::Group {})
        .restrict_dispatch("selected".into())
        .unwrap();
    let cap =
        f.db.claim_owned_dispatch_for_host(&selected, "host", 2000, 60_000, 60_000, 1)
            .unwrap()
            .unwrap();
    assert_eq!(cap.dispatch_id, "selected");
    assert!(
        f.db.claim_owned_dispatch_for_host(&selected, "other", 2001, 60_000, 60_000, 2)
            .unwrap()
            .is_none()
    );
    let mut normal = Fixture::new();
    normal.queue("first", "work");
    normal.queue("selected", "work");
    let profile = normal.profile("DEVICE_1", RoomPrivacy::Group {});
    assert_eq!(
        normal
            .db
            .claim_owned_dispatch_for_host(&profile, "host", 2000, 60_000, 60_000, 1)
            .unwrap()
            .unwrap()
            .dispatch_id,
        "first"
    );
    assert!(
        normal
            .profile("DEVICE_1", RoomPrivacy::Group {})
            .restrict_dispatch("bad/selector".into())
            .is_err()
    );
    assert!(
        normal
            .profile("DEVICE_1", RoomPrivacy::Group {})
            .restrict_dispatch("first".into())
            .unwrap()
            .restrict_dispatch("changed".into())
            .is_err()
    );
}
