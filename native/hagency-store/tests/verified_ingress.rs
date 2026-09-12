#[path = "verified_ingress/attachments.rs"]
mod attachments;
mod common;
#[path = "verified_ingress/notice_custody.rs"]
mod notice_custody;
use common::*;
use hagency_core::{ingress::*, messages::*, replies::*, task_intents::*, tasks::*};
use hagency_store::{DomainRepository, EffectOutcome, Error};
use serde_json::json;
use std::collections::BTreeSet;

struct Fixture {
    root: tempfile::TempDir,
    db: DomainRepository,
    agents: Vec<String>,
    room: MatrixRoomObservation,
}
impl Fixture {
    fn new(direct: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut db = DomainRepository::open(&root.path().join("state")).unwrap();
        db.register(&registration()).unwrap();
        let pool = resource("pool", "seat", 1000);
        db.put_resource(&pool).unwrap();
        let mut agents = vec![];
        for name in if direct { vec!["a"] } else { vec!["a", "b"] } {
            let proof = proof(&request(name, name, &pool, 100));
            let agent = db.admit(&proof, 1000).unwrap();
            db.approve(&format!("approve_{name}"), &proof, 1000)
                .unwrap();
            let effect = db.claim_effect().unwrap().unwrap();
            db.observe_effect(
                &effect.id,
                effect.fence,
                &EffectOutcome::Applied {
                    receipt: "fixture account".into(),
                },
            )
            .unwrap();
            db.observe_matrix_transport(
                &MatrixTransportObservation {
                    engagement_id: agent.id.clone(),
                    registration_generation: 1,
                    generation: 1,
                    sender_mxid: format!("@{name}:example.test"),
                    device_id: format!("DEVICE_{name}"),
                },
                1001,
            )
            .unwrap();
            agents.push(agent.id);
        }
        let mut joined = BTreeSet::from(["@owner:example.test".into(), "@a:example.test".into()]);
        if !direct {
            joined.extend([
                "@b:example.test".into(),
                "@other:example.test".into(),
                registration().representative_mxid,
                registration().approval_bot_mxid,
            ]);
        }
        let room = MatrixRoomObservation {
            engagement_id: agents[0].clone(),
            registration_generation: 1,
            transport_generation: 1,
            room_id: if direct {
                "!dm:example.test"
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
            joined,
            invite_only: true,
            encrypted: direct,
        };
        for (index, agent) in agents.iter().enumerate() {
            let mut observation = room.clone();
            observation.engagement_id = agent.clone();
            db.observe_matrix_room(&observation, 1002).unwrap();
            db.resolve_verified_matrix_session(
                &SessionBinding {
                    id: if index == 0 { "a" } else { "b" }.into(),
                    engagement_id: agent.clone(),
                    room_id: room.room_id.clone(),
                    thread_root: None,
                },
                1003,
            )
            .unwrap();
        }
        Self {
            root,
            db,
            agents,
            room,
        }
    }
    fn event(
        &self,
        session: &str,
        id: &str,
        thread: Option<&str>,
        mentions: &[&str],
        at: u64,
    ) -> MatrixEventObservation {
        MatrixEventObservation {
            scope: self.db.matrix_ingress_scope(session).unwrap(),
            event: InboundMessage {
                server_name: "example.test".into(),
                room_id: self.room.room_id.clone(),
                event_id: format!("${id}"),
                sender_mxid: "@owner:example.test".into(),
                thread_root: thread.map(str::to_owned),
                body: format!("Message {id}"),
                kind: "m.text".into(),
                origin_ts: at,
            },
            mentions: mentions.iter().map(|s| (*s).into()).collect(),
            encrypted: self.room.encrypted,
        }
    }
    fn intent(&self, session: &str, key: &str, sequence: u64) -> VerifiedTaskRequest {
        VerifiedTaskRequest {
            scope: self.db.matrix_ingress_scope(session).unwrap(),
            request_key: key.into(),
            source_sequence: sequence,
            definition: TaskDefinition {
                title: "Verify canonical work".into(),
                ..Default::default()
            },
        }
    }
    fn sql(&self) -> rusqlite::Connection {
        rusqlite::Connection::open(self.root.path().join("state/domain.sqlite3")).unwrap()
    }
    fn activate(&mut self, at: u64) -> (VerifiedNoticeClaim, IntentResult) {
        let claim = self
            .db
            .claim_verified_task_notice(at, 1000)
            .unwrap()
            .unwrap();
        self.db
            .begin_verified_task_notice_send(&claim.claim.notice.id, &claim.claim.token, at)
            .unwrap();
        let task = self
            .db
            .deliver_verified_task_notice(
                &claim.claim.notice.id,
                &claim.claim.token,
                &notice_delivery(&claim),
                at + 1,
            )
            .unwrap();
        (claim, task)
    }
    fn start(
        &mut self,
        id: &str,
        task: &IntentResult,
        sequences: &[u64],
        at: u64,
    ) -> RunnerCapability {
        self.db
            .enqueue_inbox_dispatch(&dispatch(id, task), sequences)
            .unwrap();
        let cap = self
            .db
            .claim_dispatch("fixture_runner", at, 60000, 120000, 8)
            .unwrap()
            .unwrap();
        assert_eq!(cap.dispatch_id, id);
        self.db.start_dispatch(&cap, at + 1).unwrap();
        cap
    }
    fn done(&mut self, cap: &RunnerCapability, task: &IntentResult, at: u64) {
        self.db
            .mutate_task(
                cap,
                &task.task_id,
                "done",
                &TaskMutation::Transition {
                    status: TaskState::Done,
                    waiting_reason: None,
                    waiting_until: None,
                },
                at,
            )
            .unwrap();
    }
}
fn dispatch(id: &str, task: &IntentResult) -> DispatchInput {
    DispatchInput {
        id: id.into(),
        session_id: task.session_id.clone(),
        task_id: Some(task.task_id.clone()),
        resources: vec![],
        payload: json!({"instruction":"Handle the admitted task input"}),
    }
}
fn notice_delivery(claim: &VerifiedNoticeClaim) -> ReplyDeliveryObservation {
    ReplyDeliveryObservation {
        transaction_id: claim.claim.notice.transaction_id.clone(),
        digest: claim.digest.clone(),
        server_name: claim.route.server_name.clone(),
        room_id: claim.route.room_id.clone(),
        sender_mxid: claim.route.sender_mxid.clone(),
        device_id: claim.route.device_id.clone(),
        event_id: format!("$ack_{}", claim.claim.notice.id),
        encrypted: claim.route.encrypted,
    }
}
fn count(sql: &rusqlite::Connection, table: &str) -> u64 {
    sql.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}
fn setup_task(f: &mut Fixture) -> (MatrixEventObservation, u64, IntentResult) {
    let event = f.event("a", "root", None, &["@a:example.test"], 1010);
    let source = f.db.admit_matrix_event(&event, 1011).unwrap();
    let input = f.intent("a", "request", source.sequence);
    let task = f.db.create_verified_task_intent(&input, 1012).unwrap();
    (event, source.sequence, task)
}

#[test]
fn native_verified_ingress_policy() {
    trait Ambiguous<A> {
        fn check() {}
    }
    impl<T: ?Sized> Ambiguous<()> for T {}
    impl<T: serde::de::DeserializeOwned> Ambiguous<u8> for T {}
    let _ = <MatrixEventObservation as Ambiguous<_>>::check;
    let _ = <VerifiedTaskRequest as Ambiguous<_>>::check;
    let _ = <MatrixIngressScope as Ambiguous<_>>::check;
    let mut f = Fixture::new(false);
    for (id, mentions, expected) in [
        ("background", vec![], false),
        ("text_mention", vec![], false),
        ("wrong_server", vec!["@a:other.test"], false),
        ("actual", vec!["@a:example.test"], true),
    ] {
        let mut event = f.event("a", id, None, &mentions, 1010);
        event.event.body = "@a:example.test arbitrary body text".into();
        assert_eq!(
            f.db.admit_matrix_event(&event, 1011).unwrap().wake,
            expected
        );
    }
    for (index, sender) in [
        "@a:example.test".to_owned(),
        "@b:example.test".into(),
        registration().representative_mxid,
        registration().approval_bot_mxid,
    ]
    .into_iter()
    .enumerate()
    {
        let mut event = f.event(
            "a",
            &format!("service{index}"),
            None,
            &["@a:example.test"],
            1012,
        );
        event.event.sender_mxid = sender;
        assert!(!f.db.admit_matrix_event(&event, 1013).unwrap().wake);
    }
    for field in [
        "session",
        "incarnation",
        "registration",
        "room_generation",
        "device",
        "sender",
        "room",
        "server",
        "thread",
        "future",
        "old",
    ] {
        let mut event = f.event("a", "bad", None, &["@a:example.test"], 1010);
        match field {
            "session" => event.scope.session_id = "unknown".into(),
            "incarnation" => event.scope.session_generation += 1,
            "registration" => event.scope.registration_generation += 1,
            "room_generation" => event.scope.room_generation += 1,
            "device" => event.scope.transport_generation += 1,
            "sender" => event.event.sender_mxid = "@absent:example.test".into(),
            "room" => event.event.room_id = "!other:example.test".into(),
            "server" => event.event.server_name = "other.test".into(),
            "thread" => event.event.thread_root = Some("$unknown".into()),
            "future" => event.event.origin_ts = 9999,
            _ => event.event.origin_ts = 1,
        };
        assert!(f.db.admit_matrix_event(&event, 1011).is_err(), "{field}");
    }
    let source = f.event("a", "legacy", None, &[], 1010);
    assert!(
        f.db.ingest_message(
            &source.event,
            &[MessageTarget {
                session_id: "a".into(),
                wake: true
            }],
            1011
        )
        .is_err()
    );
    let mut dm = Fixture::new(true);
    let event = dm.event("a", "dm", None, &[], 1010);
    assert!(dm.db.admit_matrix_event(&event, 1011).unwrap().wake);
    let mut bad = dm.event("a", "unencrypted", None, &[], 1012);
    bad.encrypted = false;
    assert!(dm.db.admit_matrix_event(&bad, 1013).is_err());
}

#[test]
fn native_verified_ingress_task_activation() {
    for direct in [false, true] {
        let mut f = Fixture::new(direct);
        let event = f.event("a", "root", None, &["@a:example.test"], 1010);
        let sql = f.sql();
        sql.execute_batch("CREATE TRIGGER fail_projection BEFORE INSERT ON session_inputs BEGIN SELECT RAISE(ABORT,'fixture projection failure'); END;").unwrap();
        assert!(f.db.admit_matrix_event(&event, 1011).is_err());
        assert_eq!(count(&sql, "admitted_messages"), 0);
        assert_eq!(count(&sql, "matrix_ingress_events"), 0);
        sql.execute_batch("DROP TRIGGER fail_projection").unwrap();
        let source = f.db.admit_matrix_event(&event, 1011).unwrap();
        let input = f.intent("a", "create", source.sequence);
        let sessions = count(&sql, "runner_sessions");
        sql.execute_batch("CREATE TRIGGER fail_anchor BEFORE INSERT ON task_notices BEGIN SELECT RAISE(ABORT,'fixture anchor failure'); END;").unwrap();
        assert!(f.db.create_verified_task_intent(&input, 1012).is_err());
        assert_eq!(count(&sql, "canonical_tasks"), 0);
        assert_eq!(count(&sql, "task_intents"), 0);
        assert_eq!(count(&sql, "runner_sessions"), sessions);
        sql.execute_batch("DROP TRIGGER fail_anchor").unwrap();
        let task = f.db.create_verified_task_intent(&input, 1012).unwrap();
        assert_eq!(task.activation, "pending");
        assert!(
            f.db.create_verified_task_intent(&input, 1013)
                .unwrap()
                .replayed
        );
        let mut changed = input.clone();
        changed.definition.title = "Different".into();
        assert!(matches!(
            f.db.create_verified_task_intent(&changed, 1013),
            Err(Error::Conflict)
        ));
        assert!(
            f.db.enqueue_inbox_dispatch(&dispatch("early", &task), &[source.sequence])
                .is_err()
        );
        assert!(f.db.claim_task_notice(1014, 1000).unwrap().is_none());
        let claim =
            f.db.claim_verified_task_notice(1014, 1000)
                .unwrap()
                .unwrap();
        assert_eq!(claim.source_event_id, "$root");
        assert_eq!(
            claim.claim.notice.thread_root,
            if direct { None } else { Some("$root".into()) }
        );
        assert_eq!(claim.route.thread_root, claim.claim.notice.thread_root);
        f.db.begin_verified_task_notice_send(&claim.claim.notice.id, &claim.claim.token, 1015)
            .unwrap();
        for field in ["sender", "device", "digest", "room", "encryption"] {
            let mut bad = notice_delivery(&claim);
            match field {
                "sender" => bad.sender_mxid = "@other:example.test".into(),
                "device" => bad.device_id = "OTHER".into(),
                "digest" => bad.digest = "0".repeat(64),
                "room" => bad.room_id = "!other:example.test".into(),
                _ => {
                    if !direct {
                        continue;
                    }
                    bad.encrypted = false
                }
            };
            assert!(
                f.db.deliver_verified_task_notice(
                    &claim.claim.notice.id,
                    &claim.claim.token,
                    &bad,
                    1015
                )
                .is_err(),
                "{field}"
            );
        }
        let old = NoticeDelivery {
            server_name: claim.route.server_name.clone(),
            room_id: claim.route.room_id.clone(),
            transaction_id: claim.claim.notice.transaction_id.clone(),
            event_id: "$old_receipt".into(),
        };
        assert!(
            f.db.deliver_task_notice(&claim.claim.notice.id, &claim.claim.token, &old, 1015)
                .is_err()
        );
        sql.execute_batch("CREATE TRIGGER fail_activation BEFORE UPDATE ON task_intents WHEN NEW.state='active' BEGIN SELECT RAISE(ABORT,'fixture activation failure'); END;").unwrap();
        assert!(
            f.db.deliver_verified_task_notice(
                &claim.claim.notice.id,
                &claim.claim.token,
                &notice_delivery(&claim),
                1015
            )
            .is_err()
        );
        assert_eq!(
            sql.query_row(
                "SELECT state FROM task_notices WHERE id=?1",
                [&claim.claim.notice.id],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            "sending"
        );
        sql.execute_batch("DROP TRIGGER fail_activation").unwrap();
        f.db.deliver_verified_task_notice(
            &claim.claim.notice.id,
            &claim.claim.token,
            &notice_delivery(&claim),
            1016,
        )
        .unwrap();
        let cap = f.start("work", &task, &[source.sequence], 1017);
        f.done(&cap, &task, 1019);
        let reply =
            f.db.submit_final_reply(
                &cap,
                &FinalReply {
                    call_id: "final".into(),
                    body: "Actual intent completed".into(),
                },
                1020,
            )
            .unwrap();
        assert_eq!(reply.execution_epoch, 1);
        let send = f.db.claim_final_reply(1021, 1000).unwrap().unwrap();
        let output = f.db.begin_final_reply_send(&send, 1022).unwrap();
        assert_eq!(
            output.route.thread_root,
            if direct { None } else { Some("$root".into()) }
        );
    }
}

#[test]
fn native_verified_ingress_followup() {
    for direct in [false, true] {
        let mut f = Fixture::new(direct);
        let (_, seq, task) = setup_task(&mut f);
        f.activate(1013);
        let cap = f.start("first", &task, &[seq], 1015);
        f.done(&cap, &task, 1017);
        let first =
            f.db.submit_final_reply(
                &cap,
                &FinalReply {
                    call_id: "first".into(),
                    body: "First answer".into(),
                },
                1018,
            )
            .unwrap();
        f.db.complete_dispatch(&cap, &json!({"done":true}), 1019)
            .unwrap();
        let mut event = f.event(
            &task.session_id,
            "next",
            if direct { None } else { Some("$root") },
            if direct { &[] } else { &["@a:example.test"] },
            1020,
        );
        let next = f.db.admit_matrix_event(&event, 1021).unwrap();
        assert!(next.wake);
        let same =
            f.db.create_verified_task_intent(
                &f.intent(&task.session_id, "continue", next.sequence),
                1022,
            )
            .unwrap();
        assert_eq!(same.task_id, task.task_id);
        assert_eq!(count(&f.sql(), "canonical_tasks"), 1);
        f.db.enqueue_inbox_dispatch(&dispatch("next", &task), &[next.sequence])
            .unwrap();
        assert_eq!(
            f.db.canonical_task(&task.task_id).unwrap().status,
            TaskState::Done
        );
        let nextcap =
            f.db.claim_dispatch("next_runner", 1023, 60000, 120000, 8)
                .unwrap()
                .unwrap();
        assert_eq!(
            f.db.canonical_task(&task.task_id).unwrap().execution_epoch,
            1
        );
        f.db.start_dispatch(&nextcap, 1024).unwrap();
        assert_eq!(
            f.db.canonical_task(&task.task_id).unwrap().execution_epoch,
            2
        );
        assert!(
            f.db.submit_final_reply(
                &cap,
                &FinalReply {
                    call_id: "old".into(),
                    body: "Old epoch".into()
                },
                1025
            )
            .is_err()
        );
        assert!(f.db.claim_final_reply(1025, 1000).unwrap().is_none());
        assert_eq!(
            f.sql()
                .query_row(
                    "SELECT state FROM final_replies WHERE id=?1",
                    [first.id],
                    |r| r.get::<_, String>(0)
                )
                .unwrap(),
            "cancelled"
        );
        f.done(&nextcap, &task, 1026);
        let reply =
            f.db.submit_final_reply(
                &nextcap,
                &FinalReply {
                    call_id: "second".into(),
                    body: "Second answer".into(),
                },
                1027,
            )
            .unwrap();
        // Reopening and the later Done transition each advance canonical authority.
        assert_eq!(reply.execution_epoch, 3);
        f.db.complete_dispatch(&nextcap, &json!({"done":true}), 1028)
            .unwrap();
        event.event.event_id = "$stale".into();
        event.event.origin_ts = 1010;
        assert!(!f.db.admit_matrix_event(&event, 1030).unwrap().wake);
        if !direct {
            event.event.event_id = "$foreign".into();
            event.event.origin_ts = 1031;
            event.event.sender_mxid = "@other:example.test".into();
            assert!(!f.db.admit_matrix_event(&event, 1032).unwrap().wake);
            event.event.sender_mxid = "@owner:example.test".into();
            event.event.event_id = "$unmentioned".into();
            event.mentions.clear();
            assert!(!f.db.admit_matrix_event(&event, 1032).unwrap().wake);
        }
    }
}

#[test]
fn native_verified_ingress_copies() {
    let mut f = Fixture::new(false);
    let event = f.event(
        "a",
        "shared",
        None,
        &["@a:example.test", "@b:example.test"],
        1010,
    );
    let a = f.db.admit_matrix_event(&event, 1011).unwrap();
    let mut second = event.clone();
    second.scope = f.db.matrix_ingress_scope("b").unwrap();
    let b = f.db.admit_matrix_event(&second, 1012).unwrap();
    assert_eq!(a.sequence, b.sequence);
    let at =
        f.db.create_verified_task_intent(&f.intent("a", "a_task", a.sequence), 1013)
            .unwrap();
    let bt =
        f.db.create_verified_task_intent(&f.intent("b", "b_task", b.sequence), 1013)
            .unwrap();
    assert_ne!(at.task_id, bt.task_id);
    assert_ne!(at.session_id, bt.session_id);
    f.activate(1014);
    f.activate(1016);
    let cap = f.start("a_work", &at, &[a.sequence], 1018);
    let before = f.db.runner_inbox(&cap, 0, 100, 1020).unwrap();
    assert_eq!(before[0].message.body, "Message shared");
    // Copied verified input remains independent of the shared source projection.
    f.sql().execute("UPDATE admitted_messages SET config=json_set(config,'$.body','changed shared projection') WHERE sequence=?1",[a.sequence]).unwrap();
    assert_eq!(
        f.db.runner_inbox(&cap, 0, 100, 1021).unwrap()[0]
            .message
            .body,
        "Message shared"
    );
    assert_eq!(
        f.db.inbox(&bt.session_id, 0, 100, None).unwrap()[0]
            .message
            .body,
        "Message shared"
    );
    let later = f.event(&at.session_id, "later", Some("$shared"), &[], 1022);
    f.db.admit_matrix_event(&later, 1023).unwrap();
    assert_eq!(f.db.runner_inbox(&cap, 0, 100, 1024).unwrap().len(), 1);
    f.done(&cap, &at, 1025);
    f.db.complete_dispatch(&cap, &json!({"done":true}), 1026)
        .unwrap();
    assert_eq!(f.db.inbox(&bt.session_id, 0, 100, None).unwrap().len(), 1);
    assert_eq!(
        f.db.inbox(&at.session_id, 0, 100, None).unwrap()[0]
            .message
            .event_id,
        "$later"
    );
    let bcap = f.start("b_work", &bt, &[b.sequence], 1027);
    assert_eq!(
        f.db.runner_inbox(&bcap, 0, 100, 1029).unwrap()[0]
            .message
            .body,
        "Message shared"
    );
}

#[test]
fn native_verified_ingress_task_activation_existing_thread() {
    for direct in [false, true] {
        let mut f = Fixture::new(direct);
        let root = f.event("a", "root", None, &[], 1010);
        let source = f.db.admit_matrix_event(&root, 1011).unwrap();
        f.db.resolve_verified_matrix_session(
            &SessionBinding {
                id: "thread".into(),
                engagement_id: f.agents[0].clone(),
                room_id: f.room.room_id.clone(),
                thread_root: Some("$root".into()),
            },
            1012,
        )
        .unwrap();
        let mention = f.event(
            "thread",
            "mention",
            Some("$root"),
            &["@a:example.test"],
            1013,
        );
        let admitted = f.db.admit_matrix_event(&mention, 1014).unwrap();
        let task =
            f.db.create_verified_task_intent(
                &f.intent("thread", "thread_task", admitted.sequence),
                1015,
            )
            .unwrap();
        assert_eq!(task.session_id, "thread");
        let more = f.event(
            "thread",
            "before_ack",
            Some("$root"),
            &["@a:example.test"],
            1016,
        );
        let more = f.db.admit_matrix_event(&more, 1017).unwrap();
        assert!(
            f.db.enqueue_inbox_dispatch(
                &dispatch("early", &task),
                &[admitted.sequence, more.sequence]
            )
            .is_err()
        );
        let (claim, _) = f.activate(1018);
        assert_eq!(claim.source_event_id, "$root");
        assert_ne!(claim.source_event_id, notice_delivery(&claim).event_id);
        assert_eq!(claim.claim.notice.thread_root.as_deref(), Some("$root"));
        let cap = f.start(
            "thread_work",
            &task,
            &[source.sequence, admitted.sequence, more.sequence],
            1020,
        );
        let inbox = f.db.runner_inbox(&cap, 0, 100, 1022).unwrap();
        assert_eq!(
            inbox
                .iter()
                .map(|i| i.message.event_id.as_str())
                .collect::<Vec<_>>(),
            vec!["$root", "$mention", "$before_ack"]
        );
        assert!(!inbox[0].wake);
        assert!(inbox[1].wake && inbox[2].wake);
        f.sql()
            .execute(
                "UPDATE matrix_session_routes SET retired=1 WHERE session_id='a'",
                [],
            )
            .unwrap();
        assert!(f.db.matrix_ingress_scope("thread").is_err());
        assert!(f.db.runner_inbox(&cap, 0, 100, 1023).is_err());
        assert!(
            f.db.deliver_verified_task_notice(
                &claim.claim.notice.id,
                &claim.claim.token,
                &notice_delivery(&claim),
                1023
            )
            .is_err()
        );
    }
    let mut f = Fixture::new(false);
    f.db.resolve_verified_matrix_session(
        &SessionBinding {
            id: "unknown_thread".into(),
            engagement_id: f.agents[0].clone(),
            room_id: f.room.room_id.clone(),
            thread_root: Some("$missing".into()),
        },
        1010,
    )
    .unwrap();
    let event = f.event(
        "unknown_thread",
        "mention",
        Some("$missing"),
        &["@a:example.test"],
        1011,
    );
    let input = f.db.admit_matrix_event(&event, 1012).unwrap();
    assert!(
        f.db.create_verified_task_intent(&f.intent("unknown_thread", "bad", input.sequence), 1013)
            .is_err()
    );
    assert_eq!(count(&f.sql(), "canonical_tasks"), 0);
    // A stored reply cannot be substituted for the authenticated top-level root.
    f.db.resolve_verified_matrix_session(
        &SessionBinding {
            id: "nested".into(),
            engagement_id: f.agents[0].clone(),
            room_id: f.room.room_id.clone(),
            thread_root: Some("$mention".into()),
        },
        1014,
    )
    .unwrap();
    let nested = f.event(
        "nested",
        "nested_request",
        Some("$mention"),
        &["@a:example.test"],
        1015,
    );
    let nested = f.db.admit_matrix_event(&nested, 1016).unwrap();
    assert!(
        f.db.create_verified_task_intent(&f.intent("nested", "nested", nested.sequence), 1017)
            .is_err()
    );
}

#[test]
fn native_verified_ingress_fencing() {
    for change in [
        "promotion",
        "negative",
        "device",
        "allocation",
        "registration",
        "parent",
    ] {
        let direct = change != "parent";
        let mut f = Fixture::new(direct);
        let (event, seq, task) = setup_task(&mut f);
        let source_request = f.intent("a", "retry", seq);
        let claim =
            f.db.claim_verified_task_notice(1013, 1000)
                .unwrap()
                .unwrap();
        match change {
            "promotion" => {
                f.room.generation = 2;
                f.room.privacy = RoomPrivacy::Group {};
                f.room.joined.insert("@other:example.test".into());
                f.db.observe_matrix_room(&f.room, 1020).unwrap();
            }
            "negative" => {
                let mut bad = f.room.clone();
                bad.generation = 2;
                bad.joined.remove("@owner:example.test");
                f.db.observe_matrix_room(&bad, 1020).unwrap();
            }
            "device" => {
                f.db.observe_matrix_transport(
                    &MatrixTransportObservation {
                        engagement_id: f.agents[0].clone(),
                        registration_generation: 1,
                        generation: 2,
                        sender_mxid: "@a:example.test".into(),
                        device_id: "ROTATED".into(),
                    },
                    1020,
                )
                .unwrap();
            }
            "allocation" => {
                f.db.revoke("revoke", &f.agents[0]).unwrap();
            }
            "registration" => {
                let mut reg = registration();
                reg.generation = 2;
                f.db.register(&reg).unwrap();
            }
            _ => {
                f.sql()
                    .execute(
                        "UPDATE matrix_session_routes SET retired=1 WHERE session_id='a'",
                        [],
                    )
                    .unwrap();
            }
        }
        assert!(f.db.admit_matrix_event(&event, 1021).is_err(), "{change}");
        assert!(
            f.db.create_verified_task_intent(&source_request, 1021)
                .is_err()
        );
        assert!(
            f.db.deliver_verified_task_notice(
                &claim.claim.notice.id,
                &claim.claim.token,
                &notice_delivery(&claim),
                1021
            )
            .is_err()
        );
        assert!(
            f.db.enqueue_inbox_dispatch(&dispatch("old", &task), &[seq])
                .is_err()
        );
        assert!(
            f.db.claim_verified_task_notice(1022, 1000)
                .unwrap()
                .is_none()
        );
        if change == "promotion" || change == "parent" {
            f.db.resolve_verified_matrix_session(
                &SessionBinding {
                    id: "fresh".into(),
                    engagement_id: f.agents[0].clone(),
                    room_id: f.room.room_id.clone(),
                    thread_root: None,
                },
                1023,
            )
            .unwrap();
            let mut old = event.clone();
            old.scope = f.db.matrix_ingress_scope("fresh").unwrap();
            assert!(f.db.admit_matrix_event(&old, 1024).is_err());
            assert!(
                f.db.create_verified_task_intent(&f.intent("fresh", "copy_old", seq), 1024)
                    .is_err()
            );
            let fresh = f.event("fresh", "public_new", None, &["@a:example.test"], 1025);
            let fresh = f.db.admit_matrix_event(&fresh, 1026).unwrap();
            let next = f
                .db
                .create_verified_task_intent(&f.intent("fresh", "new_task", fresh.sequence), 1027)
                .unwrap();
            f.activate(1028);
            let cap = f.start("new", &next, &[fresh.sequence], 1030);
            assert_eq!(
                f.db.runner_inbox(&cap, 0, 100, 1032)
                    .unwrap()
                    .iter()
                    .map(|i| i.message.event_id.as_str())
                    .collect::<Vec<_>>(),
                vec!["$public_new"]
            );
        }
    }
}

#[test]
fn native_verified_ingress_recovery() {
    let mut f = Fixture::new(true);
    let event = f.event("a", "delayed", None, &[], 1010);
    let first = f.db.admit_matrix_event(&event, 1050).unwrap();
    let request = f.intent("a", "task", first.sequence);
    let task = f.db.create_verified_task_intent(&request, 1051).unwrap();
    let old = f.db.claim_verified_task_notice(1052, 5).unwrap().unwrap();
    f.db.observe_matrix_room(&f.room, 2000).unwrap();
    f.db.observe_matrix_transport(
        &MatrixTransportObservation {
            engagement_id: f.agents[0].clone(),
            registration_generation: 1,
            generation: 1,
            sender_mxid: "@a:example.test".into(),
            device_id: "DEVICE_a".into(),
        },
        2000,
    )
    .unwrap();
    assert_eq!(
        f.sql()
            .query_row(
                "SELECT ingress_since FROM matrix_session_routes WHERE session_id='a'",
                [],
                |r| r.get::<_, u64>(0)
            )
            .unwrap(),
        1002
    );
    drop(f.db);
    f.db = DomainRepository::open(&f.root.path().join("state")).unwrap();
    let replay = f.db.admit_matrix_event(&event, 2001).unwrap();
    assert_eq!(replay.sequence, first.sequence);
    assert!(!replay.created && !replay.projected);
    assert!(
        f.db.create_verified_task_intent(&request, 2002)
            .unwrap()
            .replayed
    );
    let offline = f.event("a", "offline", None, &[], 1011);
    let offline = f.db.admit_matrix_event(&offline, 2003).unwrap();
    assert!(offline.wake);
    let mut conflict = event.clone();
    conflict.event.body = "Changed body".into();
    assert!(matches!(
        f.db.admit_matrix_event(&conflict, 2004),
        Err(Error::Conflict)
    ));
    let mut conflict = event.clone();
    conflict.mentions.insert("@a:example.test".into());
    assert!(matches!(
        f.db.admit_matrix_event(&conflict, 2004),
        Err(Error::Conflict)
    ));
    assert!(
        f.db.deliver_verified_task_notice(
            &old.claim.notice.id,
            &old.claim.token,
            &notice_delivery(&old),
            2004
        )
        .is_err()
    );
    let next =
        f.db.claim_verified_task_notice(2004, 1000)
            .unwrap()
            .unwrap();
    assert_eq!(
        next.claim.notice.transaction_id,
        old.claim.notice.transaction_id
    );
    assert_ne!(next.claim.token, old.claim.token);
    f.db.begin_verified_task_notice_send(&next.claim.notice.id, &next.claim.token, 2005)
        .unwrap();
    f.db.deliver_verified_task_notice(
        &next.claim.notice.id,
        &next.claim.token,
        &notice_delivery(&next),
        2005,
    )
    .unwrap();
    assert!(
        f.db.deliver_verified_task_notice(
            &next.claim.notice.id,
            &next.claim.token,
            &notice_delivery(&next),
            2006
        )
        .unwrap()
        .replayed
    );
    let cap = f.start(
        "after_restart",
        &task,
        &[first.sequence, offline.sequence],
        2007,
    );
    assert_eq!(f.db.runner_inbox(&cap, 0, 100, 2009).unwrap().len(), 2);
    f.done(&cap, &task, 2010);
    let reply =
        f.db.submit_final_reply(
            &cap,
            &FinalReply {
                call_id: "result".into(),
                body: "Recovered activation".into(),
            },
            2011,
        )
        .unwrap();
    f.db.complete_dispatch(&cap, &json!({"done":true}), 2012)
        .unwrap();
    drop(f.db);
    f.db = DomainRepository::open(&f.root.path().join("state")).unwrap();
    assert!(f.db.inbox("a", 0, 100, None).unwrap().is_empty());
    assert!(f.db.claim_final_reply(2013, 1000).unwrap().is_some());
    assert_eq!(
        f.db.canonical_task(&task.task_id).unwrap().execution_epoch,
        reply.execution_epoch
    );
}

#[test]
fn native_verified_ingress_recovery_schema_eleven_has_unknown_boundary() {
    let mut f = Fixture::new(true);
    drop(f.db);
    let sql = rusqlite::Connection::open(f.root.path().join("state/domain.sqlite3")).unwrap();
    remove_ingress_schema(&sql);
    sql.pragma_update(None, "user_version", 11).unwrap();
    drop(sql);
    f.db = DomainRepository::open(&f.root.path().join("state")).unwrap();
    assert!(f.db.matrix_ingress_scope("a").is_err());
    f.db.observe_matrix_room(&f.room, 2000).unwrap();
    assert!(f.db.matrix_ingress_scope("a").is_err());
    f.db.observe_matrix_transport(
        &MatrixTransportObservation {
            engagement_id: f.agents[0].clone(),
            registration_generation: 1,
            generation: 2,
            sender_mxid: "@a:example.test".into(),
            device_id: "NEW".into(),
        },
        2001,
    )
    .unwrap();
    f.room.transport_generation = 2;
    f.room.generation = 2;
    f.db.observe_matrix_room(&f.room, 2002).unwrap();
    f.db.resolve_verified_matrix_session(
        &SessionBinding {
            id: "fresh".into(),
            engagement_id: f.agents[0].clone(),
            room_id: f.room.room_id.clone(),
            thread_root: None,
        },
        2003,
    )
    .unwrap();
    let fresh = f.event("fresh", "fresh", None, &[], 2004);
    assert!(f.db.admit_matrix_event(&fresh, 2005).unwrap().wake);
    assert!(f.db.matrix_ingress_scope("a").is_err());
}

#[test]
fn native_matrix_intake_rotation_historical_receipt_is_content_bound_read_only() {
    let mut f = Fixture::new(false);
    let event = f.event("a", "receipt", None, &["@a:example.test"], 1004);
    assert!(f.db.matrix_ingress_receipt(&event).unwrap().is_none());
    let receipt = f.db.admit_matrix_event(&event, 1004).unwrap();
    let found = f.db.matrix_ingress_receipt(&event).unwrap().unwrap();
    assert_eq!(found.sequence, receipt.sequence);
    assert!(!found.created && !found.projected);
    let mut changed = event.clone();
    changed.event.body.push('!');
    assert!(matches!(
        f.db.matrix_ingress_receipt(&changed),
        Err(Error::Conflict)
    ));
    let mut foreign = event.clone();
    foreign.scope.transport_generation += 1;
    assert!(matches!(
        f.db.matrix_ingress_receipt(&foreign),
        Err(Error::RunnerAuthority)
    ));
    f.db.invalidate_matrix_transport(
        &MatrixTransportInvalidation {
            expected: MatrixTransportObservation {
                engagement_id: f.agents[0].clone(),
                registration_generation: 1,
                generation: 1,
                sender_mxid: "@a:example.test".into(),
                device_id: "DEVICE_a".into(),
            },
            reason: "Retired authenticated source".into(),
        },
        1005,
    )
    .unwrap();
    assert!(f.db.admit_matrix_event(&event, 1006).is_err());
    assert_eq!(
        f.db.matrix_ingress_receipt(&event)
            .unwrap()
            .unwrap()
            .sequence,
        receipt.sequence
    );
    let mut absent = event.clone();
    absent.event.event_id = "$absent".into();
    assert!(f.db.matrix_ingress_receipt(&absent).unwrap().is_none());
    let n: u64 = f
        .sql()
        .query_row("SELECT COUNT(*) FROM admitted_messages", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 1);
}
