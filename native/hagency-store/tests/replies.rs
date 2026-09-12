mod common;
use common::*;
use hagency_core::{replies::*, tasks::*};
use hagency_store::{DomainRepository, EffectOutcome, Error};
use serde_json::json;
use std::collections::BTreeSet;

struct Fixture {
    root: tempfile::TempDir,
    db: DomainRepository,
    engagement: String,
    binding: SessionBinding,
    room: MatrixRoomObservation,
    transport: MatrixTransportObservation,
    cap: RunnerCapability,
}
fn dispatch(id: &str, session: &str, task: Option<&str>) -> DispatchInput {
    DispatchInput {
        id: id.into(),
        session_id: session.into(),
        task_id: task.map(str::to_owned),
        resources: vec![],
        payload: json!({"instruction":"Verify result"}),
    }
}
impl Fixture {
    fn new(direct: bool, root: Option<&str>) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let mut db = DomainRepository::open(&temp.path().join("state")).unwrap();
        db.register(&registration()).unwrap();
        let pool = resource("pool", "seat", 1000);
        db.put_resource(&pool).unwrap();
        let p = proof(&request("one", "Worker", &pool, 100));
        let e = db.admit(&p, 1000).unwrap();
        db.approve("approve", &p, 1000).unwrap();
        let effect = db.claim_effect().unwrap().unwrap();
        db.observe_effect(
            &effect.id,
            effect.fence,
            &EffectOutcome::Applied {
                receipt: "fixture provision".into(),
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
        let room = MatrixRoomObservation {
            engagement_id: e.id.clone(),
            registration_generation: 1,
            transport_generation: 1,
            room_id: if direct {
                "!direct:example.test"
            } else {
                "!project:example.test"
            }
            .into(),
            generation: 1,
            privacy: if direct {
                RoomPrivacy::Direct {
                    human_mxid: "@owner:example.test".into(),
                }
            } else {
                RoomPrivacy::Group {}
            },
            joined: BTreeSet::from(["@worker:example.test".into(), "@owner:example.test".into()]),
            invite_only: true,
            encrypted: direct,
        };
        db.observe_matrix_room(&room, 1002).unwrap();
        let binding = SessionBinding {
            id: "session".into(),
            engagement_id: e.id.clone(),
            room_id: room.room_id.clone(),
            thread_root: root.map(str::to_owned),
        };
        db.resolve_verified_matrix_session(&binding, 1003).unwrap();
        db.create_canonical_task("task", &binding.id, "Verify task", 1004)
            .unwrap();
        db.enqueue_dispatch(&dispatch("dispatch", &binding.id, Some("task")))
            .unwrap();
        let cap = db
            .claim_dispatch("runner", 1005, 60_000, 120_000, 8)
            .unwrap()
            .unwrap();
        db.start_dispatch(&cap, 1006).unwrap();
        Self {
            root: temp,
            db,
            engagement: e.id,
            binding,
            room,
            transport,
            cap,
        }
    }
    fn done(&mut self) {
        self.db
            .mutate_task(
                &self.cap,
                "task",
                "done",
                &TaskMutation::Transition {
                    status: TaskState::Done,
                    waiting_reason: None,
                    waiting_until: None,
                },
                1007,
            )
            .unwrap();
    }
    fn reply(&mut self) -> ReplyReceipt {
        self.db
            .submit_final_reply(&self.cap, &content(), 1008)
            .unwrap()
    }
    fn sql(&self) -> rusqlite::Connection {
        rusqlite::Connection::open(self.root.path().join("state/domain.sqlite3")).unwrap()
    }
    fn state(&self, id: &str) -> String {
        self.sql()
            .query_row("SELECT state FROM final_replies WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .unwrap()
    }
}
fn content() -> FinalReply {
    FinalReply {
        call_id: "result".into(),
        body: "Verified **private result**".into(),
    }
}
fn delivered(send: &ReplySend) -> ReplyDeliveryObservation {
    ReplyDeliveryObservation {
        transaction_id: send.transaction_id.clone(),
        digest: send.digest.clone(),
        server_name: send.route.server_name.clone(),
        room_id: send.route.room_id.clone(),
        sender_mxid: send.route.sender_mxid.clone(),
        device_id: send.route.device_id.clone(),
        event_id: "$delivered".into(),
        encrypted: send.route.encrypted,
    }
}
fn count(db: &rusqlite::Connection, table: &str) -> u64 {
    db.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

#[test]
fn native_reply_routes() {
    trait Ambiguous<A> {
        fn check() {}
    }
    impl<T: ?Sized> Ambiguous<()> for T {}
    impl<T: serde::de::DeserializeOwned> Ambiguous<u8> for T {}
    let _ = <MatrixRoomObservation as Ambiguous<_>>::check;
    let _ = <MatrixTransportObservation as Ambiguous<_>>::check;
    let _ = <ReplyDeliveryObservation as Ambiguous<_>>::check;
    let _ = <ReplyClaim as Ambiguous<_>>::check;
    let mut f = Fixture::new(true, None);
    let resolved =
        f.db.resolve_verified_matrix_session(
            &SessionBinding {
                id: "another".into(),
                ..f.binding.clone()
            },
            1007,
        )
        .unwrap();
    assert_eq!(resolved.id, "session");
    assert!(f.db.resolve_session(&f.binding).is_err());
    for changed in ["server", "owner", "device", "generation"] {
        let mut bad = f.transport.clone();
        match changed {
            "server" => bad.sender_mxid = "@worker:other.test".into(),
            "owner" => bad.sender_mxid = "@owner:example.test".into(),
            "device" => bad.device_id = "OTHER".into(),
            _ => bad.generation = 3,
        };
        assert!(
            f.db.observe_matrix_transport(&bad, 1007).is_err(),
            "{changed}"
        );
    }
    let mut approval = f.room.clone();
    approval.room_id = "!private:example.test".into();
    assert!(f.db.observe_matrix_room(&approval, 1007).is_err());
    // Existing runtime history must never be upgraded from today's room state.
    let legacy = SessionBinding {
        id: "legacy".into(),
        thread_root: Some("$legacy".into()),
        ..f.binding.clone()
    };
    f.db.register_session(&legacy).unwrap();
    assert!(f.db.resolve_verified_matrix_session(&legacy, 1007).is_err());
    f.db.create_canonical_task("legacy_task", "legacy", "Legacy", 1007)
        .unwrap();
    f.db.enqueue_dispatch(&dispatch("legacy", "legacy", Some("legacy_task")))
        .unwrap();
    let legacy_cap =
        f.db.claim_dispatch("other", 1008, 60_000, 120_000, 8)
            .unwrap()
            .unwrap();
    f.db.start_dispatch(&legacy_cap, 1009).unwrap();
    f.db.mutate_task(
        &legacy_cap,
        "legacy_task",
        "done",
        &TaskMutation::Transition {
            status: TaskState::Done,
            waiting_reason: None,
            waiting_until: None,
        },
        1010,
    )
    .unwrap();
    assert!(
        f.db.submit_final_reply(&legacy_cap, &content(), 1011)
            .is_err()
    );
    f.transport.generation = 2;
    f.transport.device_id = "DEVICE_2".into();
    f.db.observe_matrix_transport(&f.transport, 1012).unwrap();
    assert!(f.db.check_runner(&f.cap, 1013).is_err());
    let fresh = SessionBinding {
        id: "fresh".into(),
        ..f.binding.clone()
    };
    assert!(f.db.resolve_verified_matrix_session(&fresh, 1013).is_err());
    assert!(f.db.observe_matrix_room(&f.room, 1013).is_err());
    f.room.transport_generation = 2;
    f.db.observe_matrix_room(&f.room, 1013).unwrap();
    f.db.resolve_verified_matrix_session(&fresh, 1013).unwrap();
    assert_ne!(fresh.id, f.binding.id);
    assert_eq!(
        f.sql()
            .query_row(
                "SELECT matrix_generation FROM runner_sessions WHERE id='fresh'",
                [],
                |r| r.get::<_, u64>(0)
            )
            .unwrap(),
        2
    );
}

#[test]
fn native_reply_intents() {
    let mut f = Fixture::new(false, Some("$root"));
    assert!(f.db.submit_final_reply(&f.cap, &content(), 1007).is_err());
    f.done();
    let sql = f.sql();
    sql.execute_batch("CREATE TRIGGER fail_call BEFORE INSERT ON final_reply_calls BEGIN SELECT RAISE(ABORT,'injected reply failure'); END;").unwrap();
    assert!(f.db.submit_final_reply(&f.cap, &content(), 1008).is_err());
    assert_eq!(count(&sql, "final_replies"), 0);
    sql.execute_batch("DROP TRIGGER fail_call").unwrap();
    let r = f.reply();
    assert_eq!(r.state, ReplyState::Pending);
    assert!(!r.replayed);
    assert!(
        f.db.submit_final_reply(&f.cap, &content(), 1009)
            .unwrap()
            .replayed
    );
    let mut next = content();
    next.call_id = "same_epoch".into();
    assert!(
        f.db.submit_final_reply(&f.cap, &next, 1009)
            .unwrap()
            .replayed
    );
    next.body = "Changed answer".into();
    assert!(matches!(
        f.db.submit_final_reply(&f.cap, &next, 1009),
        Err(Error::Conflict)
    ));
    assert_eq!(count(&sql, "final_replies"), 1);
    assert_eq!(count(&sql, "final_reply_calls"), 2);
    assert_eq!(f.db.canonical_task("task").unwrap().execution_epoch, 1);
    let view = value(f.db.runner_final_reply(&f.cap, &r.id, 1009).unwrap());
    for key in ["route", "room_id", "device_id", "owner_mxid", "body"] {
        assert!(view.get(key).is_none())
    }
    for field in [
        "room_id",
        "owner_mxid",
        "task_id",
        "device_id",
        "delivered",
        "execution_epoch",
    ] {
        let mut bad = value(content());
        bad[field] = json!("forged");
        assert!(serde_json::from_value::<FinalReply>(bad).is_err());
    }
    next.body = "x".repeat(32 * 1024 + 1);
    assert!(f.db.submit_final_reply(&f.cap, &next, 1009).is_err());
    f.db.park_dispatch(&f.cap, true, 1010).unwrap();
    assert!(f.db.submit_final_reply(&f.cap, &content(), 1011).is_err());
    f.db.park_dispatch(&f.cap, false, 1012).unwrap();
    let mut bad = f.cap.clone();
    bad.fence += 1;
    assert!(f.db.submit_final_reply(&bad, &content(), 1013).is_err());
    f.db.complete_dispatch(&f.cap, &json!({"final_intent":r.id}), 1014)
        .unwrap();
    assert_eq!(f.state(&r.id), "pending");
}

#[test]
fn native_reply_promotion() {
    for root in [None, Some("$private_root")] {
        for phase in ["pending", "claimed", "sending"] {
            let mut f = Fixture::new(true, root);
            f.done();
            let r = f.reply();
            let claim = if phase != "pending" {
                Some(f.db.claim_final_reply(1009, 60_000).unwrap().unwrap())
            } else {
                None
            };
            let send = if phase == "sending" {
                Some(
                    f.db.begin_final_reply_send(claim.as_ref().unwrap(), 1010)
                        .unwrap(),
                )
            } else {
                None
            };
            f.room.generation = 2;
            f.room.privacy = RoomPrivacy::Group {};
            f.room.joined.insert("@second:example.test".into());
            f.db.observe_matrix_room(&f.room, 1011).unwrap();
            assert_eq!(
                f.state(&r.id),
                if phase == "sending" {
                    "uncertain"
                } else {
                    "cancelled"
                }
            );
            assert!(f.db.check_runner(&f.cap, 1012).is_err());
            assert!(f.db.claim_final_reply(1012, 1000).unwrap().is_none());
            if let Some(claim) = &claim {
                assert!(f.db.begin_final_reply_send(claim, 1012).is_err());
                if let Some(send) = &send {
                    assert!(
                        f.db.observe_final_reply(claim, &delivered(send), 1012)
                            .is_err()
                    );
                }
            }
            let fresh = SessionBinding {
                id: "fresh_group".into(),
                ..f.binding.clone()
            };
            f.db.resolve_verified_matrix_session(&fresh, 1013).unwrap();
            f.db.create_canonical_task("new_task", &fresh.id, "Public work", 1014)
                .unwrap();
            f.db.enqueue_dispatch(&dispatch("new_dispatch", &fresh.id, Some("new_task")))
                .unwrap();
            let cap =
                f.db.claim_dispatch("new_runner", 1015, 60_000, 120_000, 8)
                    .unwrap()
                    .unwrap();
            assert_eq!(cap.dispatch_id, "new_dispatch");
            f.db.start_dispatch(&cap, 1016).unwrap();
            f.db.mutate_task(
                &cap,
                "new_task",
                "done",
                &TaskMutation::Transition {
                    status: TaskState::Done,
                    waiting_reason: None,
                    waiting_until: None,
                },
                1017,
            )
            .unwrap();
            let new =
                f.db.submit_final_reply(
                    &cap,
                    &FinalReply {
                        call_id: "new".into(),
                        body: "Fresh group context".into(),
                    },
                    1018,
                )
                .unwrap();
            assert_ne!(new.id, r.id);
            let claim = f.db.claim_final_reply(1019, 1000).unwrap().unwrap();
            let send = f.db.begin_final_reply_send(&claim, 1020).unwrap();
            assert!(matches!(send.route.privacy, RoomPrivacy::Group {}));
            assert_eq!(send.route.session_id, "fresh_group");
            assert_eq!(send.body, "Fresh group context");
        }
    }
    for trigger in ["revocation", "registration", "owner"] {
        let mut f = Fixture::new(true, None);
        f.done();
        let r = f.reply();
        match trigger {
            "revocation" => {
                f.db.revoke("revoke", &f.engagement).unwrap();
            }
            "registration" => {
                let mut reg = registration();
                reg.generation = 2;
                f.db.register(&reg).unwrap();
            }
            _ => {
                f.sql()
                    .execute("UPDATE projects SET owner_mxid='@other:example.test'", [])
                    .unwrap();
            }
        };
        assert!(f.db.claim_final_reply(1009, 1000).unwrap().is_none());
        assert_eq!(f.state(&r.id), "cancelled");
        assert!(f.db.submit_final_reply(&f.cap, &content(), 1010).is_err());
    }
}

#[test]
fn native_reply_transport() {
    let mut f = Fixture::new(true, Some("$thread"));
    f.done();
    let r = f.reply();
    let old = f.db.claim_final_reply(1009, 2).unwrap().unwrap();
    let claim = f.db.claim_final_reply(1011, 1000).unwrap().unwrap();
    assert!(claim.fence > old.fence);
    assert!(f.db.begin_final_reply_send(&old, 1012).is_err());
    let send = f.db.begin_final_reply_send(&claim, 1012).unwrap();
    assert_eq!(send.route.thread_root.as_deref(), Some("$thread"));
    assert_eq!(f.state(&r.id), "sending");
    assert!(f.db.begin_final_reply_send(&claim, 1013).is_err());
    for field in [
        "transaction",
        "digest",
        "server",
        "room",
        "sender",
        "device",
        "encryption",
    ] {
        let mut bad = delivered(&send);
        match field {
            "transaction" => bad.transaction_id = "other".into(),
            "digest" => bad.digest = "other".into(),
            "server" => bad.server_name = "other.test".into(),
            "room" => bad.room_id = "!other:example.test".into(),
            "sender" => bad.sender_mxid = "@other:example.test".into(),
            "device" => bad.device_id = "OTHER".into(),
            _ => bad.encrypted = false,
        };
        assert!(
            f.db.observe_final_reply(&claim, &bad, 1013).is_err(),
            "{field}"
        );
    }
    let sql = f.sql();
    sql.execute_batch("CREATE TRIGGER fail_observation BEFORE UPDATE ON final_replies WHEN NEW.state='delivered' BEGIN SELECT RAISE(ABORT,'injected delivery failure'); END;").unwrap();
    assert!(
        f.db.observe_final_reply(&claim, &delivered(&send), 1013)
            .is_err()
    );
    assert_eq!(f.state(&r.id), "sending");
    sql.execute_batch("DROP TRIGGER fail_observation").unwrap();
    assert_eq!(
        f.db.observe_final_reply(&claim, &delivered(&send), 1014)
            .unwrap()
            .state,
        ReplyState::Delivered
    );
    assert!(
        f.db.observe_final_reply(&claim, &delivered(&send), 2000)
            .unwrap()
            .replayed
    );
    let mut changed = delivered(&send);
    changed.event_id = "$different".into();
    assert!(matches!(
        f.db.observe_final_reply(&claim, &changed, 2000),
        Err(Error::Conflict)
    ));
    assert!(f.db.cancel_final_reply(&r.id, 2000).is_err());
    assert_eq!(f.db.canonical_task("task").unwrap().status, TaskState::Done);
}

#[test]
fn native_reply_recovery() {
    for started in [false, true] {
        let mut f = Fixture::new(true, None);
        f.done();
        let r = f.reply();
        let old = f.db.claim_final_reply(1009, 1000).unwrap().unwrap();
        let send = if started {
            Some(f.db.begin_final_reply_send(&old, 1010).unwrap())
        } else {
            None
        };
        drop(f.db);
        f.db = DomainRepository::open(&f.root.path().join("state")).unwrap();
        assert_eq!(
            f.state(&r.id),
            if started { "uncertain" } else { "pending" }
        );
        assert!(f.db.begin_final_reply_send(&old, 1011).is_err());
        if started {
            assert!(f.db.claim_final_reply(1012, 1000).unwrap().is_none());
            assert!(
                f.db.reconcile_final_reply(
                    &r.id,
                    old.fence + 1,
                    &ReplyReconciliation::NotSent {
                        evidence: "fixture inspected transport journal".into()
                    },
                    1013
                )
                .is_err()
            );
            f.db.reconcile_final_reply(
                &r.id,
                old.fence,
                &ReplyReconciliation::NotSent {
                    evidence: "fixture proved no send was accepted".into(),
                },
                1013,
            )
            .unwrap();
        }
        let claim = f.db.claim_final_reply(1014, 1000).unwrap().unwrap();
        let retry = f.db.begin_final_reply_send(&claim, 1015).unwrap();
        if let Some(send) = send {
            assert_eq!(retry.transaction_id, send.transaction_id);
            assert_eq!(retry.digest, send.digest);
        }
        assert_eq!(
            f.db.cancel_final_reply(&r.id, 1016).unwrap().state,
            ReplyState::Uncertain
        );
        assert!(
            f.db.observe_final_reply(&claim, &delivered(&retry), 1017)
                .is_err()
        );
        assert_eq!(
            f.db.reconcile_final_reply(
                &r.id,
                claim.fence,
                &ReplyReconciliation::Delivered(delivered(&retry)),
                1017
            )
            .unwrap()
            .state,
            ReplyState::Delivered
        );
        assert_eq!(f.db.canonical_task("task").unwrap().execution_epoch, 1);
    }
    // An inspected runner report can recover intent admission without rerunning
    // completed work, and repeated content still owns the same task-epoch intent.
    let mut f = Fixture::new(false, None);
    f.done();
    let first = f.reply();
    drop(f.db);
    f.db = DomainRepository::open(&f.root.path().join("state")).unwrap();
    let mut input = dispatch("report", "session", None);
    input.payload = json!({"instruction":"Report inspected completion only"});
    f.db.recover_dispatch("dispatch", &input, "Fixture inspected stopped runner", 1010)
        .unwrap();
    let report =
        f.db.claim_dispatch("reporter", 1011, 60_000, 120_000, 8)
            .unwrap()
            .unwrap();
    f.db.start_dispatch(&report, 1012).unwrap();
    let replay = f.db.submit_final_reply(&report, &content(), 1013).unwrap();
    assert_eq!(replay.id, first.id);
    assert!(replay.replayed);
    f.sql().execute("UPDATE canonical_tasks SET config=json_set(config,'$.execution_epoch',2) WHERE id='task'",[]).unwrap();
    assert!(f.db.submit_final_reply(&report, &content(), 1014).is_err());
    assert!(f.db.claim_final_reply(1014, 1000).unwrap().is_none());
    assert_eq!(f.state(&first.id), "cancelled");
}

#[test]
fn native_reply_recovery_cancelled_send_stays_cancelled() {
    for restart in [false, true] {
        let mut f = Fixture::new(true, None);
        f.done();
        let reply = f.reply();
        let claim = f.db.claim_final_reply(1009, 1000).unwrap().unwrap();
        f.db.begin_final_reply_send(&claim, 1010).unwrap();
        assert_eq!(
            f.db.cancel_final_reply(&reply.id, 1011).unwrap().state,
            ReplyState::Uncertain
        );
        if restart {
            drop(f.db);
            f.db = DomainRepository::open(&f.root.path().join("state")).unwrap();
        }
        let observation = ReplyReconciliation::NotSent {
            evidence: "Inspected transport transaction; no accepted send".into(),
        };
        assert_eq!(
            f.db.reconcile_final_reply(&reply.id, claim.fence, &observation, 1012)
                .unwrap()
                .state,
            ReplyState::Cancelled
        );
        let replay =
            f.db.reconcile_final_reply(&reply.id, claim.fence, &observation, 1013)
                .unwrap();
        assert!(replay.replayed);
        assert_eq!(replay.state, ReplyState::Cancelled);
        assert!(matches!(
            f.db.reconcile_final_reply(
                &reply.id,
                claim.fence,
                &ReplyReconciliation::NotSent {
                    evidence: "Different evidence".into()
                },
                1014
            ),
            Err(Error::Conflict)
        ));
        assert!(f.db.claim_final_reply(1015, 1000).unwrap().is_none());
        if !restart {
            assert_eq!(
                f.db.submit_final_reply(&f.cap, &content(), 1016)
                    .unwrap()
                    .state,
                ReplyState::Cancelled
            );
        }
        drop(f.db);
        f.db = DomainRepository::open(&f.root.path().join("state")).unwrap();
        assert_eq!(f.state(&reply.id), "cancelled");
        assert!(f.db.claim_final_reply(1017, 1000).unwrap().is_none());
    }
    let mut f = Fixture::new(true, None);
    f.done();
    let reply = f.reply();
    let claim = f.db.claim_final_reply(1009, 2).unwrap().unwrap();
    f.db.begin_final_reply_send(&claim, 1010).unwrap();
    assert!(f.db.claim_final_reply(1011, 1000).unwrap().is_none());
    assert_eq!(f.state(&reply.id), "uncertain");
    assert!(
        f.db.reconcile_final_reply(
            &reply.id,
            claim.fence,
            &ReplyReconciliation::NotSent {
                evidence: "x".repeat(4001)
            },
            1012
        )
        .is_err()
    );
    assert_eq!(f.state(&reply.id), "uncertain");
}

#[test]
fn native_reply_routes_negative_observations() {
    for changed in [
        "human",
        "third_member",
        "encryption",
        "invitation",
        "agent_left",
        "owner_left",
        "missing",
        "unknown",
    ] {
        for sending in [false, true] {
            let mut f = Fixture::new(true, None);
            f.done();
            let reply = f.reply();
            let claim = f.db.claim_final_reply(1009, 1000).unwrap().unwrap();
            if sending {
                f.db.begin_final_reply_send(&claim, 1010).unwrap();
            }
            let mut bad = f.room.clone();
            bad.generation = 2;
            match changed {
                "missing" | "unknown" => {
                    let observation = MatrixRoomInvalidation {
                        engagement_id: f.engagement.clone(),
                        registration_generation: 1,
                        transport_generation: 1,
                        room_id: f.room.room_id.clone(),
                        generation: 2,
                        reason: changed.into(),
                    };
                    f.db.invalidate_matrix_room(&observation, 1011).unwrap();
                    f.db.invalidate_matrix_room(&observation, 1011).unwrap();
                    let mut stale = observation.clone();
                    stale.transport_generation = 2;
                    assert!(f.db.invalidate_matrix_room(&stale, 1011).is_err());
                }
                _ => {
                    match changed {
                        "human" => {
                            bad.privacy = RoomPrivacy::Direct {
                                human_mxid: "@other:example.test".into(),
                            }
                        }
                        "third_member" => {
                            bad.joined.insert("@other:example.test".into());
                        }
                        "encryption" => bad.encrypted = false,
                        "invitation" => bad.invite_only = false,
                        "agent_left" => {
                            bad.joined.remove("@worker:example.test");
                        }
                        _ => {
                            bad.joined.remove("@owner:example.test");
                        }
                    };
                    f.db.observe_matrix_room(&bad, 1011).unwrap();
                    f.db.observe_matrix_room(&bad, 1011).unwrap();
                }
            }
            assert_eq!(
                f.state(&reply.id),
                if sending { "uncertain" } else { "cancelled" },
                "{changed}"
            );
            assert!(f.db.check_runner(&f.cap, 1012).is_err());
            assert!(f.db.begin_final_reply_send(&claim, 1012).is_err());
            assert!(f.db.claim_final_reply(1012, 1000).unwrap().is_none());
            assert!(f.db.observe_matrix_room(&f.room, 1012).is_err());
            let mut same = f.room.clone();
            same.generation = 2;
            assert!(f.db.observe_matrix_room(&same, 1012).is_err());
            drop(f.db);
            f.db = DomainRepository::open(&f.root.path().join("state")).unwrap();
            assert!(f.db.claim_final_reply(1013, 1000).unwrap().is_none());
            f.room.generation = 3;
            f.db.observe_matrix_room(&f.room, 1014).unwrap();
            assert!(
                f.db.resolve_verified_matrix_session(&f.binding, 1015)
                    .is_err()
            );
            f.db.resolve_verified_matrix_session(
                &SessionBinding {
                    id: "restored".into(),
                    ..f.binding.clone()
                },
                1015,
            )
            .unwrap();
            assert!(f.db.claim_final_reply(1016, 1000).unwrap().is_none());
            if sending {
                assert_eq!(
                    f.db.reconcile_final_reply(
                        &reply.id,
                        claim.fence,
                        &ReplyReconciliation::NotSent {
                            evidence: "No accepted send after invalidation".into()
                        },
                        1017
                    )
                    .unwrap()
                    .state,
                    ReplyState::Cancelled
                );
            }
        }
    }
}

fn second_agent(f: &mut Fixture) -> String {
    let pool = resource("pool", "seat", 1000);
    let p = proof(&request("two", "Second", &pool, 100));
    let e = f.db.admit(&p, 1000).unwrap();
    f.db.approve("approve_second", &p, 1000).unwrap();
    let effect = f.db.claim_effect().unwrap().unwrap();
    f.db.observe_effect(
        &effect.id,
        effect.fence,
        &EffectOutcome::Applied {
            receipt: "fixture second agent".into(),
        },
    )
    .unwrap();
    f.db.observe_matrix_transport(
        &MatrixTransportObservation {
            engagement_id: e.id.clone(),
            registration_generation: 1,
            generation: 1,
            sender_mxid: "@second:example.test".into(),
            device_id: "SECOND".into(),
        },
        1007,
    )
    .unwrap();
    e.id
}
#[test]
fn native_reply_routes_full_group_snapshot_retires_absent_agent() {
    let mut f = Fixture::new(false, None);
    let second = second_agent(&mut f);
    let b = SessionBinding {
        id: "second_session".into(),
        engagement_id: second.clone(),
        ..f.binding.clone()
    };
    assert!(f.db.resolve_verified_matrix_session(&b, 1008).is_err());
    let mut joint = f.room.clone();
    joint.generation = 2;
    joint.joined.insert("@second:example.test".into());
    // A changed member set cannot reuse the old generation.
    let mut stale = joint.clone();
    stale.generation = 1;
    assert!(f.db.observe_matrix_room(&stale, 1008).is_err());
    f.db.observe_matrix_room(&joint, 1009).unwrap();
    let a = SessionBinding {
        id: "new_a".into(),
        ..f.binding.clone()
    };
    f.db.resolve_verified_matrix_session(&a, 1010).unwrap();
    // A's observed membership cannot become B's account/device proof.
    assert!(f.db.resolve_verified_matrix_session(&b, 1010).is_err());
    joint.engagement_id = second;
    f.db.observe_matrix_room(&joint, 1011).unwrap();
    f.db.resolve_verified_matrix_session(&b, 1012).unwrap();
    f.db.create_canonical_task("new_a_task", &a.id, "A output", 1013)
        .unwrap();
    f.db.enqueue_dispatch(&dispatch("new_a_dispatch", &a.id, Some("new_a_task")))
        .unwrap();
    let cap =
        f.db.claim_dispatch("new_a_runner", 1014, 60000, 120000, 8)
            .unwrap()
            .unwrap();
    f.db.start_dispatch(&cap, 1015).unwrap();
    f.db.mutate_task(
        &cap,
        "new_a_task",
        "done",
        &TaskMutation::Transition {
            status: TaskState::Done,
            waiting_reason: None,
            waiting_until: None,
        },
        1016,
    )
    .unwrap();
    let reply = f.db.submit_final_reply(&cap, &content(), 1017).unwrap();
    joint.generation = 3;
    joint.joined.remove("@worker:example.test");
    f.db.observe_matrix_room(&joint, 1018).unwrap();
    assert_eq!(f.state(&reply.id), "cancelled");
    assert!(f.db.check_runner(&cap, 1019).is_err());
    assert!(
        f.db.resolve_verified_matrix_session(
            &SessionBinding {
                id: "absent_a".into(),
                ..a
            },
            1019
        )
        .is_err()
    );
    // B also needs a fresh session after the full room generation advances.
    assert!(f.db.resolve_verified_matrix_session(&b, 1019).is_err());
    f.db.resolve_verified_matrix_session(
        &SessionBinding {
            id: "fresh_b".into(),
            ..b
        },
        1020,
    )
    .unwrap();
}

#[test]
fn native_reply_routes_internal_and_taskless_are_not_destinations() {
    let mut f = Fixture::new(false, None);
    let second = second_agent(&mut f);
    let group =
        f.db.create_internal_conversation(
            &f.cap,
            &hagency_core::conversations::ConversationRequest {
                call_id: "internal".into(),
                label: "Independent internal work".into(),
                participant_engagements: vec![second.clone()],
            },
            1008,
        )
        .unwrap()
        .conversation;
    let sid = group
        .participants
        .iter()
        .find(|p| p.engagement_id == second)
        .unwrap()
        .id
        .clone();
    f.db.create_canonical_task("internal_task", &sid, "Internal result", 1009)
        .unwrap();
    f.db.enqueue_dispatch(&dispatch("internal_dispatch", &sid, Some("internal_task")))
        .unwrap();
    let cap =
        f.db.claim_dispatch("internal_runner", 1010, 60_000, 120_000, 8)
            .unwrap()
            .unwrap();
    f.db.start_dispatch(&cap, 1011).unwrap();
    f.db.mutate_task(
        &cap,
        "internal_task",
        "done",
        &TaskMutation::Transition {
            status: TaskState::Done,
            waiting_reason: None,
            waiting_until: None,
        },
        1012,
    )
    .unwrap();
    assert!(f.db.submit_final_reply(&cap, &content(), 1013).is_err());
    assert_eq!(count(&f.sql(), "final_replies"), 0);
    f.db.complete_dispatch(&cap, &json!({"internal":"finished"}), 1014)
        .unwrap();
    // Retiring the verified parent also closes internal descendants.
    f.room.generation = 2;
    f.room.joined.remove("@owner:example.test");
    f.db.observe_matrix_room(&f.room, 1015).unwrap();
    assert_eq!(
        f.sql()
            .query_row(
                "SELECT state FROM internal_conversations WHERE id=?1",
                [&group.id],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
        "closed"
    );

    let mut f = Fixture::new(true, None);
    f.done();
    f.db.complete_dispatch(&f.cap, &json!({"finished":true}), 1008)
        .unwrap();
    f.db.enqueue_dispatch(&dispatch("front_desk", "session", None))
        .unwrap();
    let cap =
        f.db.claim_dispatch("desk", 1009, 60_000, 120_000, 8)
            .unwrap()
            .unwrap();
    f.db.start_dispatch(&cap, 1010).unwrap();
    assert!(f.db.submit_final_reply(&cap, &content(), 1011).is_err());
    assert_eq!(count(&f.sql(), "final_replies"), 0);
}

#[test]
fn native_reply_routes_schema_ten_does_not_invent_legacy_privacy() {
    let mut f = Fixture::new(true, None);
    f.done();
    drop(f.db);
    let sql = rusqlite::Connection::open(f.root.path().join("state/domain.sqlite3")).unwrap();
    remove_reply_schema(&sql);
    sql.pragma_update(None, "user_version", 10).unwrap();
    drop(sql);
    f.db = DomainRepository::open(&f.root.path().join("state")).unwrap();
    let sql = f.sql();
    assert_eq!(
        sql.query_row("PRAGMA user_version", [], |r| r.get::<_, u64>(0))
            .unwrap(),
        23
    );
    assert_eq!(
        sql.query_row(
            "SELECT matrix_generation FROM runner_sessions WHERE id='session'",
            [],
            |r| r.get::<_, u64>(0)
        )
        .unwrap(),
        0
    );
    assert_eq!(count(&sql, "matrix_session_routes"), 0);
    assert_eq!(count(&sql, "final_replies"), 0);
    assert_eq!(f.db.canonical_task("task").unwrap().status, TaskState::Done);
    f.db.observe_matrix_transport(&f.transport, 1010).unwrap();
    f.db.observe_matrix_room(&f.room, 1011).unwrap();
    assert!(
        f.db.resolve_verified_matrix_session(&f.binding, 1012)
            .is_err()
    );
    let mut input = dispatch("legacy_report", "session", None);
    input.payload = json!({"instruction":"Report inspected legacy completion only"});
    f.db.recover_dispatch(
        "dispatch",
        &input,
        "Fixture inspected stopped legacy runner",
        1013,
    )
    .unwrap();
    let cap =
        f.db.claim_dispatch("legacy_reporter", 1014, 60_000, 120_000, 8)
            .unwrap()
            .unwrap();
    f.db.start_dispatch(&cap, 1015).unwrap();
    assert!(f.db.submit_final_reply(&cap, &content(), 1016).is_err());
    f.db.resolve_verified_matrix_session(
        &SessionBinding {
            id: "fresh".into(),
            ..f.binding.clone()
        },
        1017,
    )
    .unwrap();
    drop(f.db);
    f.db = DomainRepository::open(&f.root.path().join("state")).unwrap();
    assert_eq!(count(&f.sql(), "matrix_session_routes"), 1);
    assert!(
        f.db.resolve_verified_matrix_session(&f.binding, 1018)
            .unwrap()
            .id
            == "fresh"
    );
}

#[test]
fn native_matrix_transport_negative_routes_and_send_custody() {
    for direct in [false, true] {
        for sending in [false, true] {
            let mut f = Fixture::new(direct, None);
            f.done();
            let reply = f.reply();
            let claim = f.db.claim_final_reply(1009, 1000).unwrap().unwrap();
            if sending {
                f.db.begin_final_reply_send(&claim, 1010).unwrap();
            }
            let negative = MatrixTransportInvalidation {
                expected: f.transport.clone(),
                reason: "Authenticated whoami failed".into(),
            };
            f.sql().execute_batch("CREATE TRIGGER fail_retire BEFORE UPDATE ON matrix_session_routes WHEN NEW.retired=1 BEGIN SELECT RAISE(ABORT,'fixture rollback'); END;").unwrap();
            assert!(f.db.invalidate_matrix_transport(&negative, 1011).is_err());
            assert!(
                f.db.matrix_transport_state(&f.engagement)
                    .unwrap()
                    .unwrap()
                    .available
            );
            assert_eq!(
                f.state(&reply.id),
                if sending { "sending" } else { "claimed" }
            );
            f.sql().execute_batch("DROP TRIGGER fail_retire").unwrap();
            f.db.invalidate_matrix_transport(&negative, 1011).unwrap();
            f.db.invalidate_matrix_transport(&negative, 1012).unwrap();
            assert_eq!(
                f.state(&reply.id),
                if sending { "uncertain" } else { "cancelled" }
            );
            assert_eq!(count(&f.sql(), "current_matrix_routes"), 0);
            assert!(
                f.db.resolve_verified_matrix_session(&f.binding, 1013)
                    .is_err()
            );
            assert!(f.db.begin_final_reply_send(&claim, 1013).is_err());
            assert!(f.db.observe_matrix_transport(&f.transport, 1013).is_err());
            drop(f.db);
            f.db = DomainRepository::open(&f.root.path().join("state")).unwrap();
            assert!(
                !f.db
                    .matrix_transport_state(&f.engagement)
                    .unwrap()
                    .unwrap()
                    .available
            );
            assert!(f.db.observe_matrix_transport(&f.transport, 1014).is_err());
        }
    }
}
#[test]
fn native_matrix_transport_generation_stale_negative_cannot_retire_replacement() {
    let mut f = Fixture::new(true, None);
    let old = MatrixTransportInvalidation {
        expected: f.transport.clone(),
        reason: "old failure".into(),
    };
    f.db.invalidate_matrix_transport(&old, 1007).unwrap();
    f.transport.generation = 2;
    f.db.observe_matrix_transport(&f.transport, 1008).unwrap();
    f.room.transport_generation = 2;
    f.db.observe_matrix_room(&f.room, 1009).unwrap();
    let binding = SessionBinding {
        id: "fresh".into(),
        ..f.binding.clone()
    };
    f.db.resolve_verified_matrix_session(&binding, 1010)
        .unwrap();
    assert!(f.db.invalidate_matrix_transport(&old, 1011).is_err());
    assert!(
        f.db.matrix_transport_state(&f.engagement)
            .unwrap()
            .unwrap()
            .available
    );
    assert_eq!(count(&f.sql(), "current_matrix_routes"), 1);
    for field in ["device", "sender", "registration"] {
        let mut bad = MatrixTransportInvalidation {
            expected: f.transport.clone(),
            reason: "failure".into(),
        };
        match field {
            "device" => bad.expected.device_id = "OTHER".into(),
            "sender" => bad.expected.sender_mxid = "@other:example.test".into(),
            _ => bad.expected.registration_generation = 2,
        };
        assert!(f.db.invalidate_matrix_transport(&bad, 1012).is_err());
    }
    assert_eq!(count(&f.sql(), "current_matrix_routes"), 1);
}
#[test]
fn native_matrix_transport_migration_preserves_verified_routes() {
    let mut f = Fixture::new(true, None);
    drop(f.db);
    let sql = rusqlite::Connection::open(f.root.path().join("state/domain.sqlite3")).unwrap();
    remove_matrix_transport_schema(&sql);
    sql.pragma_update(None, "user_version", 14).unwrap();
    drop(sql);
    f.db = DomainRepository::open(&f.root.path().join("state")).unwrap();
    assert_eq!(count(&f.sql(), "current_matrix_routes"), 1);
    assert!(
        f.db.matrix_transport_state(&f.engagement)
            .unwrap()
            .unwrap()
            .available
    );
    f.db.invalidate_matrix_transport(
        &MatrixTransportInvalidation {
            expected: f.transport.clone(),
            reason: "migration fixture failure".into(),
        },
        1010,
    )
    .unwrap();
    assert_eq!(count(&f.sql(), "current_matrix_routes"), 0);
}

#[test]
fn native_matrix_transport_migration_missing_structure_and_rollback() {
    for damaged in ["missing_trigger", "partial_upgrade"] {
        let f = Fixture::new(true, None);
        let root = f.root;
        drop(f.db);
        let sql = rusqlite::Connection::open(root.path().join("state/domain.sqlite3")).unwrap();
        if damaged == "missing_trigger" {
            sql.execute_batch("DROP TRIGGER matrix_transport_retire_approvals")
                .unwrap();
        } else {
            remove_matrix_transport_schema(&sql);
            sql.execute_batch("ALTER TABLE matrix_transports ADD COLUMN invalidation TEXT; PRAGMA user_version=14;").unwrap();
        }
        assert!(DomainRepository::open(&root.path().join("state")).is_err());
        if damaged == "partial_upgrade" {
            assert_eq!(
                sql.pragma_query_value(None, "user_version", |r| r.get::<_, u64>(0))
                    .unwrap(),
                14
            );
            assert!(
                sql.prepare("SELECT available FROM matrix_transports")
                    .is_err()
            );
        }
        assert_eq!(
            sql.query_row("SELECT device_id FROM matrix_transports", [], |r| r
                .get::<_, String>(0))
                .unwrap(),
            "DEVICE_1"
        );
    }
}

#[test]
fn native_reply_send_readiness() {
    let mut f = Fixture::new(true, None);
    f.done();
    let intent = f.reply();
    let claim = f.db.claim_final_reply(1010, 100).unwrap().unwrap();
    let preview = f.db.preview_final_reply(&claim, 1011).unwrap();
    assert_eq!(preview.id, intent.id);
    assert_eq!(preview.body, content().body);
    assert_eq!(f.state(&intent.id), "claimed");
    assert!(f.db.validate_final_reply_send(&claim, 1011).is_err());
    for bad in [
        ReplyClaim {
            secret: "0".repeat(64),
            ..claim.clone()
        },
        ReplyClaim {
            fence: claim.fence + 1,
            ..claim.clone()
        },
        ReplyClaim {
            id: "other".into(),
            ..claim.clone()
        },
    ] {
        assert!(f.db.preview_final_reply(&bad, 1011).is_err());
        assert!(f.db.validate_final_reply_send(&bad, 1011).is_err());
    }
    assert!(f.db.preview_final_reply(&claim, 1110).is_err());
    let send = f.db.begin_final_reply_send(&claim, 1012).unwrap();
    assert_eq!(send.digest, preview.digest);
    assert!(send.route == preview.route);
    assert_eq!(f.state(&intent.id), "sending");
    assert!(f.db.preview_final_reply(&claim, 1013).is_err());
    f.db.validate_final_reply_send(&claim, 1013).unwrap();
    assert!(f.db.validate_final_reply_send(&claim, 1110).is_err());
    f.room.generation += 1;
    f.room.privacy = RoomPrivacy::Group {};
    f.db.observe_matrix_room(&f.room, 1014).unwrap();
    assert!(f.db.validate_final_reply_send(&claim, 1015).is_err());
    assert_eq!(f.state(&intent.id), "uncertain");
}
#[test]
fn native_reply_sending_inspection() {
    let mut f = Fixture::new(true, Some("$root"));
    f.done();
    let intent = f.reply();
    let claim = f.db.claim_final_reply(1010, 100).unwrap().unwrap();
    let send = f.db.begin_final_reply_send(&claim, 1011).unwrap();
    let not_sent = ReplyReconciliation::NotSent {
        evidence: "inspection receipt".into(),
    };
    assert!(
        f.db.reconcile_final_reply(&intent.id, claim.fence, &not_sent, 1012)
            .is_err()
    );
    let observation = delivered(&send);
    for changed in ["digest", "room", "device", "transaction", "encryption"] {
        let mut bad = observation.clone();
        match changed {
            "digest" => bad.digest = "a".repeat(64),
            "room" => bad.room_id = "!other:example.test".into(),
            "device" => bad.device_id = "OTHER".into(),
            "transaction" => bad.transaction_id = "other".into(),
            _ => bad.encrypted = false,
        }
        assert!(
            f.db.reconcile_final_reply(
                &intent.id,
                claim.fence,
                &ReplyReconciliation::Delivered(bad),
                1012
            )
            .is_err()
        );
    }
    assert!(
        f.db.reconcile_final_reply(
            &intent.id,
            claim.fence + 1,
            &ReplyReconciliation::Delivered(observation.clone()),
            1012
        )
        .is_err()
    );
    assert_eq!(f.state(&intent.id), "sending");
    assert_eq!(count(&f.sql(), "final_reply_inspections"), 0);
    // The positive journal describes the old send even after its claim expires;
    // no claim secret or new send permission is manufactured by reconciliation.
    let proof = ReplyReconciliation::Delivered(observation);
    assert_eq!(
        f.db.reconcile_final_reply(&intent.id, claim.fence, &proof, 2000)
            .unwrap()
            .state,
        ReplyState::Delivered
    );
    assert!(
        f.db.reconcile_final_reply(&intent.id, claim.fence, &proof, 2001)
            .unwrap()
            .replayed
    );
    assert!(
        f.db.reconcile_final_reply(&intent.id, claim.fence, &not_sent, 2002)
            .is_err()
    );
    assert_eq!(count(&f.sql(), "final_reply_inspections"), 1);
    assert!(f.db.validate_final_reply_send(&claim, 2003).is_err());
}
