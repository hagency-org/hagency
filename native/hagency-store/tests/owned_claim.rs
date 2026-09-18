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
        Self::with_runtime_and_room(framework, provider, reasoning, true)
    }
    fn with_runtime_and_room(
        framework: &str,
        provider: Option<&str>,
        reasoning: &str,
        encrypted: bool,
    ) -> Self {
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
                encrypted,
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
fn native_owned_claim_plaintext_project() {
    let mut f = Fixture::with_runtime_and_room("codex", None, "medium", false);
    f.queue("other_workspace", "other");
    f.queue("selected", "work");
    let ordinary = f.profile("DEVICE_1", RoomPrivacy::Group {});
    assert!(
        f.db.claim_owned_dispatch_for_host(&ordinary, "default", 2000, 60_000, 60_000, 1)
            .unwrap()
            .is_none()
    );
    assert!(
        OwnedClaimRoom::new(
            "!direct:example.test".into(),
            1,
            RoomPrivacy::Direct {
                human_mxid: "@owner:example.test".into()
            }
        )
        .unwrap()
        .with_plaintext_project()
        .is_err()
    );
    let profile = |transport: MatrixTransportObservation, generation| {
        OwnedClaimProfile::new(
            transport,
            vec![
                OwnedClaimRoom::new(
                    "!project:example.test".into(),
                    generation,
                    RoomPrivacy::Group {},
                )
                .unwrap()
                .with_plaintext_project()
                .unwrap(),
            ],
            vec!["work".into()],
        )
        .unwrap()
    };
    let stale = profile(f.transport.clone(), 2);
    assert!(
        f.db.claim_owned_dispatch_for_host(&stale, "wrong_generation", 2000, 60_000, 60_000, 1)
            .unwrap()
            .is_none()
    );
    let mut foreign = f.transport.clone();
    foreign.device_id = "OTHER".into();
    assert!(
        f.db.claim_owned_dispatch_for_host(
            &profile(foreign, 1),
            "wrong_device",
            2000,
            60_000,
            60_000,
            1
        )
        .unwrap()
        .is_none()
    );
    let rooms = || {
        vec![
            OwnedClaimRoom::new("!project:example.test".into(), 1, RoomPrivacy::Group {})
                .unwrap()
                .with_plaintext_project()
                .unwrap(),
        ]
    };
    let bound = ordinary
        .restrict_resource("pool".into(), "seat".into())
        .unwrap();
    let mut foreign = f.transport.clone();
    foreign.generation = 2;
    assert!(
        bound
            .clone()
            .refresh_matrix_rooms(foreign, rooms())
            .is_err()
    );
    assert!(
        bound
            .clone()
            .refresh_matrix_rooms(
                f.transport.clone(),
                vec![
                    OwnedClaimRoom::new("!other:example.test".into(), 1, RoomPrivacy::Group {})
                        .unwrap()
                ]
            )
            .is_err()
    );
    assert!(
        bound
            .clone()
            .refresh_matrix_rooms(
                f.transport.clone(),
                vec![
                    OwnedClaimRoom::new(
                        "!project:example.test".into(),
                        1,
                        RoomPrivacy::Direct {
                            human_mxid: "@owner:example.test".into()
                        }
                    )
                    .unwrap()
                ]
            )
            .is_err()
    );
    let selected = bound
        .refresh_matrix_rooms(f.transport.clone(), rooms())
        .unwrap();
    assert!(
        selected
            .clone()
            .restrict_resource("other".into(), "seat".into())
            .is_err(),
        "room refresh cannot replace original provider binding"
    );
    let cap =
        f.db.claim_owned_dispatch_for_host(&selected, "project", 2000, 60_000, 60_000, 1)
            .unwrap()
            .unwrap();
    assert_eq!(cap.dispatch_id, "selected");
    let scope = f.db.owned_dispatch_scope(&cap, 2001).unwrap();
    f.db.invalidate_matrix_room(
        &MatrixRoomInvalidation {
            engagement_id: f.transport.engagement_id.clone(),
            registration_generation: 1,
            transport_generation: 1,
            room_id: "!project:example.test".into(),
            generation: 2,
            reason: "original project authority revoked".into(),
        },
        2002,
    )
    .unwrap();
    assert!(
        f.db.start_owned_dispatch(&cap, scope.fingerprint(), 2003)
            .is_err()
    );
}
#[test]
fn native_owned_claim_stopped_capacity() {
    let mut f = Fixture::new();
    f.queue("original", "work");
    f.queue("same_workspace", "work");
    f.queue("independent", "other");
    f.db.register_workspace("third").unwrap();
    let profile = OwnedClaimProfile::new(
        f.transport.clone(),
        vec![
            OwnedClaimRoom::new("!project:example.test".into(), 1, RoomPrivacy::Group {}).unwrap(),
        ],
        vec!["work".into(), "other".into(), "third".into()],
    )
    .unwrap();
    let cap =
        f.db.claim_owned_dispatch_for_host(&profile, "original_host", 2000, 60_000, 60_000, 1)
            .unwrap()
            .unwrap();
    assert_eq!(cap.dispatch_id, "original");
    let admission = f.db.owned_dispatch_scope(&cap, 2001).unwrap();
    let started =
        f.db.start_owned_dispatch(&cap, admission.fingerprint(), 2001)
            .unwrap();
    f.db.enqueue_dispatch(&DispatchInput {
        id: "same_session".into(),
        session_id: "original".into(),
        task_id: Some("original".into()),
        resources: vec![ResourceLease {
            id: "third".into(),
            exclusive: true,
        }],
        payload: json!({"instruction":"must stay blocked"}),
    })
    .unwrap();
    f.db.observe_owned_failure(&cap, hagency_store::OwnedFailure::Protocol, 2002)
        .unwrap();
    assert!(
        f.db.claim_owned_dispatch_for_host(&profile, "missing_proof", 2003, 60_000, 60_000, 1)
            .unwrap()
            .is_none()
    );
    // Store contract fixture only; actual-process proof is exercised in execution.
    f.db.record_owned_stop_inspection(
        &cap,
        &started,
        &json!({"profile":"stopped-content-inventory-v1","root":{"fixture":true},"entries":[]}),
        2004,
    )
    .unwrap();
    let sql = rusqlite::Connection::open(f._root.path().join("state/domain.sqlite3")).unwrap();
    // Negative corruption control: another attempt's receipt cannot free this one.
    sql.execute("UPDATE owned_stop_inspections SET fence=fence+1", [])
        .unwrap();
    assert!(
        f.db.claim_owned_dispatch_for_host(&profile, "wrong_fence", 2005, 60_000, 60_000, 1)
            .unwrap()
            .is_none()
    );
    sql.execute("UPDATE owned_stop_inspections SET fence=fence-1", [])
        .unwrap();
    drop(f.db);
    f.db = DomainRepository::open(&f._root.path().join("state")).unwrap();
    let next =
        f.db.claim_owned_dispatch_for_host(&profile, "next_host", 2006, 60_000, 60_000, 1)
            .unwrap()
            .expect("proven stopped owner is not a live process");
    assert_eq!(next.dispatch_id, "independent");
    assert!(
        f.db.claim_owned_dispatch_for_host(&profile, "same_scope", 2007, 60_000, 60_000, 128)
            .unwrap()
            .is_none()
    );
    let state:(String,u64,u64,u64,u64)=sql.query_row("SELECT d.state,s.quarantined,w.dirty,(SELECT COUNT(*) FROM resource_leases WHERE dispatch_id=d.id),(SELECT COUNT(*) FROM dispatch_stops WHERE dispatch_id=d.id AND settled_at IS NULL) FROM runner_dispatches d JOIN runner_sessions s ON s.id=d.session_id JOIN workspace_resources w ON w.id='work' WHERE d.id='original'",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).unwrap();
    assert_eq!(state, ("outcome_unknown".into(), 1, 1, 1, 1));
    assert_eq!(
        f.db.canonical_task("original").unwrap().status,
        TaskState::InProgress
    );
    f.queue("third_independent", "third");
    assert!(
        f.db.claim_owned_dispatch_for_host(&profile, "cap_is_still_one", 2008, 60_000, 60_000, 1)
            .unwrap()
            .is_none()
    );
}

#[test]
fn native_owned_claim_long_budget() {
    let mut f = Fixture::new();
    f.queue("selected", "work");
    let profile = f.profile("DEVICE_1", RoomPrivacy::Group {});
    let max = hagency_core::tasks::MAX_OWNED_CAPABILITY_MS;
    assert!(
        f.db.claim_dispatch("generic", 2000, 60_000, 300_001, 1)
            .is_err()
    );
    for capability_ms in [0, max + 1, u64::MAX] {
        assert!(
            f.db.claim_owned_dispatch_for_host(&profile, "host", 2000, 60_000, capability_ms, 1)
                .is_err()
        );
    }
    assert!(
        f.db.claim_owned_dispatch_for_host(
            &profile,
            "host",
            hagency_core::JSON_SAFE_MAX - max + 1,
            60_000,
            max,
            1
        )
        .is_err()
    );
    assert!(
        f.db.claim_owned_dispatch_for_host(&profile, "host", 2000, 300_001, max, 1)
            .is_err()
    );
    let sql = rusqlite::Connection::open(f._root.path().join("state/domain.sqlite3")).unwrap();
    assert_eq!(
        sql.query_row("SELECT COUNT(*) FROM runner_attempts", [], |r| r
            .get::<_, u64>(0))
            .unwrap(),
        0
    );
    let cap =
        f.db.claim_owned_dispatch_for_host(&profile, "host", 2000, 60_000, max, 1)
            .unwrap()
            .unwrap();
    let before = f.db.owned_dispatch_scope(&cap, 2001).unwrap();
    let started =
        f.db.start_owned_dispatch(&cap, before.fingerprint(), 2001)
            .unwrap();
    let expiry = 2000 + max;
    for at in (2002..expiry).step_by(1000) {
        f.db.renew_dispatch(&cap, at, 5000).unwrap();
        f.db.check_owned_dispatch(&cap, started.fingerprint(), at)
            .unwrap();
        let (lease, until) = sql
            .query_row(
                "SELECT lease_until,capability_until FROM runner_dispatches WHERE id='selected'",
                [],
                |r| Ok((r.get::<_, u64>(0)?, r.get::<_, u64>(1)?)),
            )
            .unwrap();
        assert_eq!(until, expiry);
        assert_eq!(lease, (at + 5000).min(expiry));
    }
    assert!(f.db.renew_dispatch(&cap, expiry, 5000).is_err());
    assert!(
        f.db.check_owned_dispatch(&cap, started.fingerprint(), expiry)
            .is_err()
    );
    assert_eq!(
        sql.query_row("SELECT COUNT(*) FROM runner_attempts", [], |r| r
            .get::<_, u64>(0))
            .unwrap(),
        1
    );

    // A longer capability never overrides revocation or an unrenewed lease.
    for revoke in [false, true] {
        let mut f = Fixture::new();
        f.queue("selected", "work");
        let profile = f.profile("DEVICE_1", RoomPrivacy::Group {});
        let cap =
            f.db.claim_owned_dispatch_for_host(&profile, "host", 2000, 60_000, max, 1)
                .unwrap()
                .unwrap();
        let scope = f.db.owned_dispatch_scope(&cap, 2001).unwrap();
        f.db.start_owned_dispatch(&cap, scope.fingerprint(), 2001)
            .unwrap();
        let at = if revoke {
            f.db.revoke("revoked", &f.transport.engagement_id).unwrap();
            2002
        } else {
            62_000
        };
        assert!(f.db.renew_dispatch(&cap, at, 5000).is_err());
        assert!(
            f.db.check_owned_dispatch(&cap, scope.fingerprint(), at)
                .is_err()
        );
    }
}

#[test]
fn native_owned_claim_resource_binding() {
    let mut f = Fixture::new();
    f.queue("selected", "work");
    let profile = f.profile("DEVICE_1", RoomPrivacy::Group {});
    for (preset, seat) in [("foreign", "seat"), ("pool", "foreign")] {
        let wrong = profile
            .clone()
            .restrict_resource(preset.into(), seat.into())
            .unwrap();
        assert!(
            f.db.claim_owned_dispatch_for_host(&wrong, "host", 2000, 60_000, 60_000, 1)
                .unwrap()
                .is_none()
        );
    }
    let sql = rusqlite::Connection::open(f._root.path().join("state/domain.sqlite3")).unwrap();
    assert_eq!(
        sql.query_row("SELECT COUNT(*) FROM runner_attempts", [], |row| row
            .get::<_, u64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        sql.query_row(
            "SELECT state FROM runner_dispatches WHERE id='selected'",
            [],
            |row| row.get::<_, String>(0)
        )
        .unwrap(),
        "queued"
    );
    let selected = profile
        .restrict_resource("pool".into(), "seat".into())
        .unwrap();
    assert!(
        selected
            .clone()
            .restrict_resource("other".into(), "seat".into())
            .is_err()
    );
    let selected = selected
        .restrict_resource("pool".into(), "seat".into())
        .unwrap();
    let cap =
        f.db.claim_owned_dispatch_for_host(&selected, "host", 2000, 60_000, 60_000, 1)
            .unwrap()
            .unwrap();
    assert_eq!(cap.dispatch_id, "selected");
    let scope = f.db.owned_dispatch_scope(&cap, 2001).unwrap();
    assert_eq!(scope.resource().preset_id, "pool");
    assert_eq!(scope.resource().seat_id, "seat");
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

/// G5 (wiring audit): a dispatch left `started` by a killed host is reconciled
/// at the next claim — no separate `reconcile_dispatches`/`recover_dispatch`
/// caller is needed because the owned claim path already expires abandoned
/// leases. `claim_owned_dispatch_for_host` → `claim_owned_clock`
/// (execution.rs:765) → `claim_clock` (:783) calls `expire(&tx, now)?` at
/// :810 before selecting candidates; `expire` (:227) routes a `started` row
/// through `lose` (:181) → `outcome_unknown` (the host's inspectable unknown),
/// and never re-claims it — no duplicate effect.
///
/// Lease gate: reconciliation fires only at the first claim AFTER the lease
/// has expired (the production driver grants 60 s — driver.rs:257). A restart
/// that claims again WITHIN the lease still sees the row `started` and does not
/// expire it; the test below models the expiry with a 1 s lease and a host B
/// claim at 10 s, so the gate has provably passed before reconciliation runs.
#[test]
fn native_owned_claim_reconciles_started_dispatch_after_host_death() {
    let mut f = Fixture::new();
    f.queue("first", "work");
    let profile = f.profile("DEVICE_1", RoomPrivacy::Group {});
    // Host A claims and starts, then dies: the wall clock advances past its
    // lease with no completion or observe call (the "killed mid-dispatch" case).
    let cap =
        f.db.claim_owned_dispatch_for_host(&profile, "host_a", 2000, 1_000, 1_000, 1)
            .unwrap()
            .unwrap();
    assert_eq!(cap.dispatch_id, "first");
    let scope = f.db.owned_dispatch_scope(&cap, 2001).unwrap();
    f.db.start_owned_dispatch(&cap, scope.fingerprint(), 2002)
        .unwrap();
    let db_path = f._root.path().join("state/domain.sqlite3");
    let sql = rusqlite::Connection::open(&db_path).unwrap();
    let started: String = sql
        .query_row(
            "SELECT state FROM runner_dispatches WHERE id='first'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(started, "started", "host A left the dispatch started");
    drop(sql);
    // Host B claims well after the lease expired. The claim path must expire
    // the abandoned `started` row first, never re-claim it and never mint a
    // second attempt row.
    let next =
        f.db.claim_owned_dispatch_for_host(&profile, "host_b", 10_000, 1_000, 1_000, 1)
            .unwrap();
    assert!(
        next.is_none(),
        "a started dispatch is reconciled, not re-claimed"
    );
    let sql = rusqlite::Connection::open(&db_path).unwrap();
    let settled: String = sql
        .query_row(
            "SELECT state FROM runner_dispatches WHERE id='first'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        settled, "outcome_unknown",
        "the abandoned started dispatch settles to the host's inspectable unknown"
    );
    let attempts: u64 = sql
        .query_row(
            "SELECT COUNT(*) FROM runner_attempts WHERE dispatch_id='first'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(attempts, 1, "no duplicate effect — one attempt row");
    let outcome: String = sql
        .query_row(
            "SELECT outcome FROM runner_attempts WHERE dispatch_id='first'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(outcome, "outcome_unknown");
}
