mod common;
use common::*;
use hagency_core::{messages::*, task_intents::*, tasks::*};
use hagency_store::{DomainRepository, EffectOutcome, Error};
use serde_json::json;

fn setup() -> (tempfile::TempDir, DomainRepository, Vec<String>, u64) {
    let root = tempfile::tempdir().unwrap();
    let mut db = DomainRepository::open(&root.path().join("state")).unwrap();
    db.register(&registration()).unwrap();
    let pool = resource("pool", "seat", 1000);
    db.put_resource(&pool).unwrap();
    let mut ids = Vec::new();
    for (id, name) in [("a", "小白"), ("b", "Edison"), ("c", "Other")] {
        let mut req = request(id, name, &pool, 100);
        if id == "c" {
            req.target_project_id = "project_other".into();
            req.target_room_id = "!other:example.test".into();
        }
        let p = proof(&req);
        let e = db.admit(&p, 1000).unwrap();
        db.approve(&format!("approve_{id}"), &p, 1000).unwrap();
        let effect = db.claim_effect().unwrap().unwrap();
        db.observe_effect(
            &effect.id,
            effect.fence,
            &EffectOutcome::Applied {
                receipt: format!("fixture_{id}"),
            },
        )
        .unwrap();
        db.register_session(&SessionBinding {
            id: id.into(),
            engagement_id: e.id.clone(),
            room_id: req.target_room_id,
            thread_root: None,
        })
        .unwrap();
        ids.push(e.id);
    }
    let seq = db
        .ingest_message(&message("root", None, 1000), &[target("a")], 1000)
        .unwrap()
        .sequence;
    (root, db, ids, seq)
}
fn message(id: &str, thread: Option<&str>, now: u64) -> InboundMessage {
    InboundMessage {
        server_name: "example.test".into(),
        room_id: "!project:example.test".into(),
        event_id: format!("${id}"),
        sender_mxid: "@owner:example.test".into(),
        thread_root: thread.map(str::to_owned),
        body: format!("Input {id}"),
        kind: "m.text".into(),
        origin_ts: now,
    }
}
fn target(session: &str) -> MessageTarget {
    MessageTarget {
        session_id: session.into(),
        wake: true,
    }
}
fn intent(agent: &str, root: u64) -> TaskIntent {
    TaskIntent {
        request_scope: "matrix_source".into(),
        request_key: "original".into(),
        assignee_engagement: agent.into(),
        root_sequence: root,
        input_sequences: vec![root],
        definition: TaskDefinition {
            title: "实现求和".into(),
            description: "正数、负数和零".into(),
            priority: Priority::P1,
            granularity: Granularity::Task,
            labels: vec!["测试".into()],
            parent_id: None,
        },
    }
}
fn input(id: &str, task: &IntentResult) -> DispatchInput {
    DispatchInput {
        id: id.into(),
        session_id: task.session_id.clone(),
        task_id: Some(task.task_id.clone()),
        resources: vec![],
        payload: json!({"instruction":"Handle the admitted task"}),
    }
}
fn sql(root: &tempfile::TempDir) -> rusqlite::Connection {
    rusqlite::Connection::open(root.path().join("state/domain.sqlite3")).unwrap()
}
fn count(db: &rusqlite::Connection, table: &str) -> u64 {
    db.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}
fn claim(db: &mut DomainRepository, now: u64) -> RunnerCapability {
    db.claim_dispatch("runner", now, 60_000, 120_000, 8)
        .unwrap()
        .unwrap()
}
fn delivery(c: &NoticeClaim) -> NoticeDelivery {
    NoticeDelivery {
        server_name: c.notice.server_name.clone(),
        room_id: c.notice.room_id.clone(),
        transaction_id: c.notice.transaction_id.clone(),
        event_id: format!("$receipt_{}", c.notice.id),
    }
}
fn activate(db: &mut DomainRepository, now: u64) -> IntentResult {
    let c = db.claim_task_notice(now, 1000).unwrap().unwrap();
    db.deliver_task_notice(&c.notice.id, &c.token, &delivery(&c), now + 1)
        .unwrap()
}
fn done(db: &mut DomainRepository, cap: &RunnerCapability, task: &IntentResult, now: u64) {
    db.mutate_task(
        cap,
        &task.task_id,
        "done",
        &TaskMutation::Transition {
            status: TaskState::Done,
            waiting_reason: None,
            waiting_until: None,
        },
        now,
    )
    .unwrap();
}

struct CompletedIntent {
    root: tempfile::TempDir,
    db: DomainRepository,
    agents: Vec<String>,
    task: IntentResult,
    seq: u64,
    group: hagency_core::conversations::Conversation,
    old: RunnerCapability,
}
fn completed_intent(peer: bool) -> CompletedIntent {
    use hagency_core::conversations::ConversationRequest;
    let (root, mut db, agents, seq) = setup();
    let task = db
        .create_task_intent(&intent(&agents[0], seq), 1001)
        .unwrap();
    activate(&mut db, 1002);
    db.enqueue_inbox_dispatch(
        &input(if peer { "matrix_initial" } else { "original" }, &task),
        &[seq],
    )
    .unwrap();
    let mut old = claim(&mut db, 1004);
    db.start_dispatch(&old, 1005).unwrap();
    let group = db
        .create_internal_conversation(
            &old,
            &ConversationRequest {
                call_id: "group".into(),
                label: "Completed task collaboration".into(),
                participant_engagements: vec![agents[1].clone()],
            },
            1006,
        )
        .unwrap()
        .conversation;
    db.create_coordinator_task(&old, "child", "b", "Independent child task", 1007)
        .unwrap();
    let seq = if peer {
        use hagency_core::peers::*;
        let receipt = db
            .send_peer(
                &old,
                &PeerSend {
                    call_id: "continuation".into(),
                    conversation_id: group.id.clone(),
                    recipient_session_ids: vec![task.session_id.clone()],
                    kind: PeerKind::Response,
                    priority: PeerPriority::Normal,
                    summary: "Peer result".into(),
                    body: String::new(),
                    data: json!({"score":0.5}),
                },
                1007,
            )
            .unwrap();
        db.complete_dispatch(&old, &json!({"waiting_for_peer":true}), 1007)
            .unwrap();
        db.enqueue_peer_dispatch(&input("original", &task), &[receipt.sequence])
            .unwrap();
        old = claim(&mut db, 1007);
        db.start_dispatch(&old, 1007).unwrap();
        receipt.sequence
    } else {
        seq
    };
    done(&mut db, &old, &task, 1008);
    drop(db);
    let db = DomainRepository::open(&root.path().join("state")).unwrap();
    CompletedIntent {
        root,
        db,
        agents,
        task,
        seq,
        group,
        old,
    }
}
fn report_input(id: &str, task: &IntentResult) -> DispatchInput {
    let mut value = input(id, task);
    value.task_id = None;
    value.payload = json!({"instruction":format!("Report inspected results for {id}")});
    value
}

#[test]
fn native_completed_intent_report() {
    use hagency_core::{conversations::ConversationRequest, peers::*};
    let CompletedIntent {
        root,
        mut db,
        agents,
        task,
        seq,
        group,
        old,
    } = completed_intent(false);
    let epoch = db.canonical_task(&task.task_id).unwrap().execution_epoch;
    let fresh = db
        .ingest_message(
            &message("followup", Some("$root"), 1010),
            &[target(&task.session_id)],
            1010,
        )
        .unwrap()
        .sequence;
    db.attach_task_inputs(&task.task_id, "fixture", "fresh", &[fresh])
        .unwrap();
    let replacement = report_input("report", &task);
    assert!(
        db.recover_dispatch(
            "original",
            &input("rerun_done", &task),
            "Inspected result",
            1012
        )
        .is_err()
    );
    let mut wrong = replacement.clone();
    wrong.session_id = "b".into();
    assert!(
        db.recover_dispatch("original", &wrong, "Inspected result", 1012)
            .is_err()
    );
    let inspect = sql(&root);
    inspect.execute_batch("CREATE TRIGGER fail_report BEFORE INSERT ON dispatch_recovery_reports BEGIN SELECT RAISE(ABORT,'injected grant failure'); END;").unwrap();
    assert!(
        db.recover_dispatch(
            "original",
            &replacement,
            "Inspected stopped runner and completed artifact",
            1012
        )
        .is_err()
    );
    assert_eq!(count(&inspect, "dispatch_recoveries"), 0);
    assert_eq!(count(&inspect, "dispatch_recovery_reports"), 0);
    assert_eq!(count(&inspect, "runner_dispatches"), 1);
    assert!(
        inspect
            .query_row(
                "SELECT quarantined FROM runner_sessions WHERE id=?1",
                [&task.session_id],
                |r| r.get::<_, bool>(0)
            )
            .unwrap()
    );
    inspect.execute_batch("DROP TRIGGER fail_report").unwrap();
    db.recover_dispatch(
        "original",
        &replacement,
        "Inspected stopped runner and completed artifact",
        1013,
    )
    .unwrap();
    let mut forged = report_input("forged", &task);
    forged.payload["report"] = json!(true);
    assert!(db.enqueue_dispatch(&forged).is_err());
    let report = claim(&mut db, 1014);
    let payload = db.start_dispatch(&report, 1015).unwrap();
    assert_eq!(payload["recoveryInbox"][0]["message"]["sequence"], seq);
    assert_eq!(
        db.runner_task(&report, &task.task_id, 1016).unwrap().status,
        TaskState::Done
    );
    assert!(db.runner_task(&report, "child", 1016).is_err());
    assert_eq!(db.runner_tasks(&report, "", 100, 1016).unwrap().len(), 1);
    assert!(
        db.mutate_task(
            &report,
            &task.task_id,
            "reopen",
            &TaskMutation::Transition {
                status: TaskState::InProgress,
                waiting_reason: None,
                waiting_until: None,
            },
            1016
        )
        .is_err()
    );
    assert!(
        db.create_coordinator_task(&report, "new_child", "b", "Forbidden new work", 1016)
            .is_err()
    );
    assert!(
        db.create_internal_conversation(
            &report,
            &ConversationRequest {
                call_id: "forbidden_group".into(),
                label: "New group".into(),
                participant_engagements: vec![agents[1].clone()],
            },
            1016
        )
        .is_err()
    );
    assert!(
        db.send_peer(
            &report,
            &PeerSend {
                call_id: "forbidden_send".into(),
                conversation_id: group.id,
                recipient_session_ids: vec![group.participants[0].id.clone()],
                kind: PeerKind::Request,
                priority: PeerPriority::Normal,
                summary: "New work".into(),
                body: String::new(),
                data: json!({}),
            },
            1016
        )
        .is_err()
    );
    assert!(
        db.delegate_task(
            &report,
            &Delegation {
                call_id: "forbidden_delegate".into(),
                assignee_engagement: agents[1].clone(),
                root_sequence: Some(seq),
                input_sequences: vec![seq],
                definition: intent(&agents[1], seq).definition,
            },
            1016
        )
        .is_err()
    );
    db.complete_dispatch(&report, &json!({"report":"Inspected completion"}), 1017)
        .unwrap();
    assert_eq!(
        db.canonical_task(&task.task_id).unwrap().execution_epoch,
        epoch
    );
    assert_eq!(
        db.canonical_task(&task.task_id).unwrap().status,
        TaskState::Done
    );
    let pending = db.inbox(&task.session_id, 0, 100, None).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].message.sequence, fresh);
    assert!(db.check_runner(&old, 1018).is_err());
    // Upgrade prior native report rows only using the original attempt's durable
    // done receipt, not an assertion in the replacement payload.
    remove_graph_schema(&inspect);
    inspect.execute_batch("DROP VIEW unresolved_dispatches; DROP TABLE dispatch_stops; DROP TABLE conversation_operations; ALTER TABLE internal_conversations DROP COLUMN revision; DROP VIEW current_recovery_reports; DROP TABLE dispatch_recovery_reports; PRAGMA user_version=7;").unwrap();
    drop(db);
    let db = DomainRepository::open(&root.path().join("state")).unwrap();
    assert_eq!(count(&inspect, "dispatch_recovery_reports"), 1);
    assert_eq!(count(&inspect, "current_recovery_reports"), 1);
    assert_eq!(
        db.canonical_task(&task.task_id).unwrap().execution_epoch,
        epoch
    );
}

#[test]
fn native_report_recovery_epoch() {
    let CompletedIntent {
        root,
        mut db,
        task,
        seq,
        ..
    } = completed_intent(true);
    db.recover_dispatch(
        "original",
        &report_input("report_one", &task),
        "Inspected original result",
        1010,
    )
    .unwrap();
    let first = claim(&mut db, 1011);
    db.start_dispatch(&first, 1012).unwrap();
    drop(db);
    let mut db = DomainRepository::open(&root.path().join("state")).unwrap();
    assert!(db.check_runner(&first, 1013).is_err());
    db.recover_dispatch(
        "report_one",
        &report_input("report_two", &task),
        "Inspected interrupted report",
        1014,
    )
    .unwrap();
    let second = claim(&mut db, 1015);
    db.start_dispatch(&second, 1016).unwrap();
    assert_eq!(
        db.runner_peer_inbox(&second, 0, 100, 1017).unwrap()[0]
            .message
            .sequence,
        seq
    );
    drop(db);
    let mut db = DomainRepository::open(&root.path().join("state")).unwrap();
    db.recover_dispatch(
        "report_two",
        &report_input("report_three", &task),
        "Inspected interrupted report again",
        1018,
    )
    .unwrap();
    let third = claim(&mut db, 1019);
    db.fail_before_start(&third, 1020, 1000).unwrap();
    // A genuine new human turn may progress while the report's launch is backed off.
    let fresh = db
        .ingest_message(
            &message("new_epoch", Some("$root"), 1021),
            &[target(&task.session_id)],
            1021,
        )
        .unwrap()
        .sequence;
    db.attach_task_inputs(&task.task_id, "fixture", "new_epoch", &[fresh])
        .unwrap();
    db.enqueue_inbox_dispatch(&input("new_human", &task), &[fresh])
        .unwrap();
    let human = claim(&mut db, 1023);
    assert_eq!(human.dispatch_id, "new_human");
    db.start_dispatch(&human, 1024).unwrap();
    done(&mut db, &human, &task, 1025);
    db.complete_dispatch(&human, &json!({}), 1026).unwrap();
    assert!(
        db.claim_dispatch("runner", 4000, 60_000, 120_000, 8)
            .unwrap()
            .is_none()
    );
    let inspect = sql(&root);
    assert_eq!(count(&inspect, "current_recovery_reports"), 0);
    assert_eq!(count(&inspect, "dispatch_recovery_reports"), 3);
    assert_eq!(
        inspect
            .query_row(
                "SELECT state FROM runner_dispatches WHERE id='report_three'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
        "queued"
    );
    // A separate report loses all current access as soon as its allocation is revoked.
    let CompletedIntent {
        mut db,
        agents,
        task,
        ..
    } = completed_intent(false);
    db.recover_dispatch(
        "original",
        &report_input("report", &task),
        "Inspected result",
        1010,
    )
    .unwrap();
    let report = claim(&mut db, 1011);
    db.start_dispatch(&report, 1012).unwrap();
    db.revoke("revoke_report_owner", &agents[0]).unwrap();
    assert!(db.runner_task(&report, &task.task_id, 1013).is_err());
    assert!(db.complete_dispatch(&report, &json!({}), 1013).is_err());
}

#[test]
fn native_peer_matrix_continuation() {
    use hagency_core::{conversations::ConversationRequest, peers::*};
    let (_root, mut db, agents, seq) = setup();
    let session = db
        .resolve_session(&SessionBinding {
            id: "future_task_session".into(),
            engagement_id: agents[0].clone(),
            room_id: "!project:example.test".into(),
            thread_root: Some("$root".into()),
        })
        .unwrap();
    let dispatch = |id: &str, session: &str, task: Option<&str>| DispatchInput {
        id: id.into(),
        session_id: session.into(),
        task_id: task.map(str::to_owned),
        resources: vec![],
        payload: json!({"instruction":"Continue scoped work"}),
    };
    db.enqueue_dispatch(&dispatch("creator", &session.id, None))
        .unwrap();
    let creator = claim(&mut db, 1001);
    db.start_dispatch(&creator, 1002).unwrap();
    let group = db
        .create_internal_conversation(
            &creator,
            &ConversationRequest {
                call_id: "group".into(),
                label: "协作".into(),
                participant_engagements: vec![agents[1].clone()],
            },
            1003,
        )
        .unwrap()
        .conversation;
    let b = &group
        .participants
        .iter()
        .find(|p| p.engagement_id == agents[1])
        .unwrap()
        .id;
    db.enqueue_dispatch(&dispatch("child", b, None)).unwrap();
    let child = claim(&mut db, 1004);
    db.start_dispatch(&child, 1005).unwrap();
    db.complete_dispatch(&creator, &json!({}), 1006).unwrap();
    let task = db
        .create_task_intent(&intent(&agents[0], seq), 1007)
        .unwrap();
    assert_eq!(task.session_id, session.id);
    let reply = |id: &str| PeerSend {
        call_id: id.into(),
        conversation_id: group.id.clone(),
        recipient_session_ids: vec![session.id.clone()],
        kind: PeerKind::Response,
        priority: PeerPriority::Normal,
        summary: "检查完成".into(),
        body: String::new(),
        data: json!({"answer":1.5}),
    };
    let first = db.send_peer(&child, &reply("first"), 1008).unwrap();
    assert!(
        db.enqueue_peer_dispatch(&input("pending_peer", &task), &[first.sequence])
            .is_err()
    );
    activate(&mut db, 1009);
    db.enqueue_peer_dispatch(&input("peer", &task), &[first.sequence])
        .unwrap();
    // Initial activation still requires the original admitted Matrix input.
    assert!(
        db.claim_dispatch("runner", 1011, 60_000, 120_000, 8)
            .unwrap()
            .is_none()
    );
    db.enqueue_inbox_dispatch(&input("human", &task), &[seq])
        .unwrap();
    let human = claim(&mut db, 1012);
    assert_eq!(human.dispatch_id, "human");
    db.start_dispatch(&human, 1013).unwrap();
    db.complete_dispatch(&human, &json!({"waiting_for_peer":true}), 1014)
        .unwrap();
    let continued = claim(&mut db, 1015);
    assert_eq!(continued.dispatch_id, "peer");
    db.start_dispatch(&continued, 1016).unwrap();
    assert_eq!(
        db.runner_peer_inbox(&continued, 0, 100, 1017).unwrap()[0]
            .message
            .sequence,
        first.sequence
    );
    assert_eq!(
        db.canonical_task(&task.task_id).unwrap().status,
        TaskState::InProgress
    );
    done(&mut db, &continued, &task, 1018);
    db.complete_dispatch(&continued, &json!({}), 1019).unwrap();
    let second = db.send_peer(&child, &reply("after_done"), 1020).unwrap();
    db.enqueue_peer_dispatch(&input("cannot_reopen", &task), &[second.sequence])
        .unwrap();
    assert!(
        db.claim_dispatch("runner", 1021, 60_000, 120_000, 8)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        db.canonical_task(&task.task_id).unwrap().status,
        TaskState::Done
    );
}

#[test]
fn native_task_intent_activation() {
    // Runtime JSON cannot claim that Matrix delivery or source admission happened.
    trait Ambiguous<A> {
        fn check() {}
    }
    impl<T: ?Sized> Ambiguous<()> for T {}
    impl<T: serde::de::DeserializeOwned> Ambiguous<u8> for T {}
    let _ = <TaskIntent as Ambiguous<_>>::check;
    let _ = <NoticeDelivery as Ambiguous<_>>::check;
    let (root, mut db, agents, seq) = setup();
    let inspect = sql(&root);
    let request = intent(&agents[0], seq);
    inspect.execute_batch("CREATE TRIGGER fail_notice BEFORE INSERT ON task_notices BEGIN SELECT RAISE(ABORT,'injected notice failure'); END;").unwrap();
    assert!(db.create_task_intent(&request, 1001).is_err());
    for table in [
        "canonical_tasks",
        "task_intents",
        "task_inputs",
        "task_notices",
        "task_outbox",
    ] {
        assert_eq!(count(&inspect, table), 0, "{table}");
    }
    assert_eq!(count(&inspect, "runner_sessions"), 3);
    inspect.execute_batch("DROP TRIGGER fail_notice").unwrap();
    let pending = db.create_task_intent(&request, 1001).unwrap();
    assert_eq!(pending.activation, "pending");
    assert!(
        db.inbox(&pending.session_id, 0, 100, None)
            .unwrap()
            .is_empty()
    );
    let task = db.canonical_task(&pending.task_id).unwrap();
    assert_eq!(task.title, "实现求和");
    assert_eq!(task.description, "正数、负数和零");
    assert_eq!(task.priority, Priority::P1);
    assert_eq!(task.labels, vec!["测试"]);
    assert_eq!(task.status, TaskState::Created);
    assert!(db.create_task_intent(&request, 1002).unwrap().replayed);
    let mut changed = request.clone();
    changed.definition.title = "different".into();
    assert!(matches!(
        db.create_task_intent(&changed, 1002),
        Err(Error::Conflict)
    ));
    assert!(db.enqueue_dispatch(&input("early", &pending)).is_err());
    let mut bypass = input("taskless", &pending);
    bypass.task_id = None;
    assert!(db.enqueue_dispatch(&bypass).is_err());
    let c = db.claim_task_notice(1002, 1000).unwrap().unwrap();
    let mut wrong = delivery(&c);
    wrong.room_id = "!other:example.test".into();
    assert!(
        db.deliver_task_notice(&c.notice.id, &c.token, &wrong, 1003)
            .is_err()
    );
    wrong = delivery(&c);
    wrong.transaction_id = "different".into();
    assert!(
        db.deliver_task_notice(&c.notice.id, &c.token, &wrong, 1003)
            .is_err()
    );
    inspect.execute_batch("CREATE TRIGGER fail_input BEFORE INSERT ON session_inputs BEGIN SELECT RAISE(ABORT,'injected input failure'); END;").unwrap();
    assert!(
        db.deliver_task_notice(&c.notice.id, &c.token, &delivery(&c), 1003)
            .is_err()
    );
    assert_eq!(
        db.create_task_intent(&request, 1003).unwrap().activation,
        "pending"
    );
    inspect.execute_batch("DROP TRIGGER fail_input").unwrap();
    let active = db
        .deliver_task_notice(&c.notice.id, &c.token, &delivery(&c), 1003)
        .unwrap();
    assert_eq!(active.activation, "active");
    assert_eq!(db.inbox(&active.session_id, 0, 100, None).unwrap().len(), 1);
    // Empty host dispatches cannot circumvent the input ownership gate either.
    db.enqueue_dispatch(&input("empty", &active)).unwrap();
    assert!(
        db.claim_dispatch("runner", 1004, 1000, 2000, 8)
            .unwrap()
            .is_none()
    );
    db.enqueue_inbox_dispatch(&input("run", &active), &[seq])
        .unwrap();
    let cap = claim(&mut db, 1004);
    assert_eq!(cap.dispatch_id, "run");
    db.start_dispatch(&cap, 1005).unwrap();
    db.complete_dispatch(&cap, &json!({"result":"inner result"}), 1006)
        .unwrap();
    assert_eq!(
        db.canonical_task(&active.task_id).unwrap().status,
        TaskState::InProgress
    );
    // A lease issued before the intent was created must obey the new binding at
    // start, and returning it to the queue must not bypass activation at claim.
    db.register_session(&SessionBinding {
        id: "prebound".into(),
        engagement_id: agents[1].clone(),
        room_id: "!project:example.test".into(),
        thread_root: Some("$root".into()),
    })
    .unwrap();
    db.enqueue_dispatch(&DispatchInput {
        id: "pre_intent".into(),
        session_id: "prebound".into(),
        task_id: None,
        resources: vec![],
        payload: json!({"instruction":"old"}),
    })
    .unwrap();
    let old = claim(&mut db, 1007);
    let mut new_intent = intent(&agents[1], seq);
    new_intent.request_key = "new_binding".into();
    db.create_task_intent(&new_intent, 1008).unwrap();
    assert!(db.start_dispatch(&old, 1009).is_err());
    db.fail_before_start(&old, 1010, 1000).unwrap();
    assert!(
        db.claim_dispatch("runner", 3000, 1000, 2000, 8)
            .unwrap()
            .is_none()
    );
    // A process that already started must also lose authority when a new task
    // binding is admitted; it cannot use peer/MCP commands before activation.
    let next = db
        .ingest_message(&message("next_root", None, 3001), &[target("b")], 3001)
        .unwrap()
        .sequence;
    db.register_session(&SessionBinding {
        id: "pre_started".into(),
        engagement_id: agents[1].clone(),
        room_id: "!project:example.test".into(),
        thread_root: Some("$next_root".into()),
    })
    .unwrap();
    db.enqueue_dispatch(&DispatchInput {
        id: "old_started".into(),
        session_id: "pre_started".into(),
        task_id: None,
        resources: vec![],
        payload: json!({"instruction":"Old taskless scope"}),
    })
    .unwrap();
    let started = claim(&mut db, 3002);
    db.start_dispatch(&started, 3003).unwrap();
    db.check_runner(&started, 3004).unwrap();
    let mut pending = intent(&agents[1], next);
    pending.request_key = "new_started_binding".into();
    db.create_task_intent(&pending, 3005).unwrap();
    assert!(db.check_runner(&started, 3006).is_err());
}

#[test]
fn native_task_outbox_recovery() {
    let (root, mut db, agents, seq) = setup();
    let request = intent(&agents[0], seq);
    let task = db.create_task_intent(&request, 1001).unwrap();
    let first = db.claim_task_notice(1002, 100).unwrap().unwrap();
    assert!(!format!("{first:?}").contains(&first.token));
    drop(db);
    let mut db = DomainRepository::open(&root.path().join("state")).unwrap();
    assert!(db.claim_task_notice(1101, 100).unwrap().is_none());
    let second = db.claim_task_notice(1102, 100).unwrap().unwrap();
    assert_eq!(first.notice.transaction_id, second.notice.transaction_id);
    assert_ne!(first.token, second.token);
    assert!(
        db.deliver_task_notice(&first.notice.id, &first.token, &delivery(&first), 1103)
            .is_err()
    );
    assert!(
        db.fail_task_notice(&first.notice.id, &first.token, "late", false, 1103)
            .is_err()
    );
    db.fail_task_notice(
        &second.notice.id,
        &second.token,
        "retryable_network",
        false,
        1103,
    )
    .unwrap();
    assert!(db.claim_task_notice(2102, 100).unwrap().is_none());
    let third = db.claim_task_notice(2103, 100).unwrap().unwrap();
    assert_eq!(third.notice.transaction_id, task.transaction_id);
    db.fail_task_notice(
        &third.notice.id,
        &third.token,
        "membership_refused",
        true,
        2104,
    )
    .unwrap();
    assert!(db.claim_task_notice(4000, 100).unwrap().is_none());
    assert_eq!(
        db.create_task_intent(&request, 4000).unwrap().activation,
        "pending"
    );
    db.retry_task_notice(&task.command_id, 4001).unwrap();
    let c = db.claim_task_notice(4001, 100).unwrap().unwrap();
    let receipt = delivery(&c);
    assert_eq!(
        db.deliver_task_notice(&c.notice.id, &c.token, &receipt, 4002)
            .unwrap()
            .activation,
        "active"
    );
    assert!(
        db.deliver_task_notice(&c.notice.id, &c.token, &receipt, 4003)
            .unwrap()
            .replayed
    );
    let mut conflicting = receipt;
    conflicting.event_id = "$another".into();
    assert!(matches!(
        db.deliver_task_notice(&c.notice.id, &c.token, &conflicting, 4003),
        Err(Error::Conflict)
    ));
    assert!(db.claim_task_notice(5000, 100).unwrap().is_none());
    let mut revoked = intent(&agents[1], seq);
    revoked.request_key = "revoked".into();
    let pending = db.create_task_intent(&revoked, 5001).unwrap();
    let c = db.claim_task_notice(5002, 100).unwrap().unwrap();
    db.revoke("revoke", &agents[1]).unwrap();
    assert!(
        db.deliver_task_notice(&c.notice.id, &c.token, &delivery(&c), 5003)
            .is_err()
    );
    assert!(db.claim_task_notice(5200, 100).unwrap().is_none());
    let inspect = sql(&root);
    assert_eq!(
        inspect
            .query_row(
                "SELECT state FROM task_notices WHERE id=?1",
                [pending.command_id],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
        "cancelled"
    );
}

#[test]
fn native_task_delegation_scope() {
    let (root, mut db, agents, seq) = setup();
    let parent = db
        .create_task_intent(&intent(&agents[0], seq), 1001)
        .unwrap();
    activate(&mut db, 1002);
    db.enqueue_inbox_dispatch(&input("parent", &parent), &[seq])
        .unwrap();
    let cap = claim(&mut db, 1004);
    let delegation = Delegation {
        call_id: "delegate".into(),
        assignee_engagement: agents[1].clone(),
        root_sequence: None,
        input_sequences: vec![seq],
        definition: TaskDefinition {
            title: "编写测试".into(),
            parent_id: Some(parent.task_id.clone()),
            ..Default::default()
        },
    };
    assert!(db.delegate_task(&cap, &delegation, 1004).is_err()); // leased is not started
    db.start_dispatch(&cap, 1005).unwrap();
    let mut bad = delegation.clone();
    bad.assignee_engagement = agents[2].clone();
    assert!(db.delegate_task(&cap, &bad, 1006).is_err());
    bad = delegation.clone();
    bad.definition.parent_id = Some("other_task".into());
    assert!(db.delegate_task(&cap, &bad, 1006).is_err());
    let extra = db
        .ingest_message(&message("unadmitted", None, 1006), &[target("b")], 1006)
        .unwrap()
        .sequence;
    bad = delegation.clone();
    bad.root_sequence = Some(extra);
    bad.input_sequences.clear();
    assert!(db.delegate_task(&cap, &bad, 1007).is_err());
    assert_eq!(count(&sql(&root), "task_intents"), 1);
    let child = db.delegate_task(&cap, &delegation, 1007).unwrap();
    assert!(db.delegate_task(&cap, &delegation, 1008).unwrap().replayed);
    bad = delegation.clone();
    bad.definition.title = "changed".into();
    assert!(matches!(
        db.delegate_task(&cap, &bad, 1008),
        Err(Error::Conflict)
    ));
    let record = db.runner_task(&cap, &child.task_id, 1008).unwrap();
    assert_eq!(record.parent_id, Some(parent.task_id.clone()));
    assert_eq!(record.creator_session_id, Some(parent.session_id.clone()));
    assert!(
        db.mutate_task(
            &cap,
            &child.task_id,
            "foreign",
            &TaskMutation::Comment { text: "no".into() },
            1008
        )
        .is_err()
    );
    activate(&mut db, 1009);
    db.enqueue_inbox_dispatch(&input("child", &child), &[seq])
        .unwrap();
    let child_cap = claim(&mut db, 1011);
    db.start_dispatch(&child_cap, 1012).unwrap();
    done(&mut db, &child_cap, &child, 1013);
    db.complete_dispatch(&child_cap, &json!({"result":"tests pass"}), 1014)
        .unwrap();
    assert_eq!(
        db.canonical_task(&parent.task_id).unwrap().status,
        TaskState::InProgress
    );
    let inspect = sql(&root);
    assert_eq!(inspect.query_row("SELECT processed_at FROM session_inputs WHERE session_id=?1 AND message_sequence=?2",rusqlite::params![parent.session_id,seq],|r|r.get::<_,Option<u64>>(0)).unwrap(),None);
    assert!(inspect.query_row("SELECT processed_at FROM session_inputs WHERE session_id=?1 AND message_sequence=?2",rusqlite::params![child.session_id,seq],|r|r.get::<_,Option<u64>>(0)).unwrap().is_some());
    db.park_dispatch(&cap, true, 1015).unwrap();
    assert!(db.delegate_task(&cap, &delegation, 1016).is_err());
    db.park_dispatch(&cap, false, 1017).unwrap();
    done(&mut db, &cap, &parent, 1018);
    assert!(db.delegate_task(&cap, &delegation, 1019).is_err());
}

#[test]
fn native_task_human_followup() {
    for case in [
        "fresh",
        "foreign",
        "peer",
        "old_origin",
        "old_receipt",
        "processed",
        "unattached",
        "wrong_thread",
    ] {
        let (root, mut db, agents, seq) = setup();
        let task = db
            .create_task_intent(&intent(&agents[0], seq), 1001)
            .unwrap();
        activate(&mut db, 1002);
        db.enqueue_inbox_dispatch(&input("first", &task), &[seq])
            .unwrap();
        let old = claim(&mut db, 1004);
        db.start_dispatch(&old, 1005).unwrap();
        // A message received before completion cannot become a later follow-up.
        let mut m = message("next", Some("$root"), 2000);
        let received = if case == "old_receipt" { 1006 } else { 2000 };
        match case {
            "foreign" => m.sender_mxid = "@different:example.test".into(),
            "peer" => m.kind = "peer".into(),
            "old_origin" => m.origin_ts = 1000,
            "wrong_thread" => m.thread_root = Some("$other".into()),
            _ => {}
        }
        let admitted = db.ingest_message(&m, &[target(&task.session_id)], received);
        if case == "wrong_thread" {
            assert!(admitted.is_err());
            continue;
        }
        let next = admitted.unwrap().sequence;
        done(&mut db, &old, &task, 1100);
        let epoch = db.canonical_task(&task.task_id).unwrap().execution_epoch;
        let inspect = sql(&root);
        if case == "processed" {
            inspect.execute("UPDATE session_inputs SET processed_at=2001 WHERE session_id=?1 AND message_sequence=?2",rusqlite::params![task.session_id,next]).unwrap();
        }
        if case != "unattached" {
            db.attach_task_inputs(&task.task_id, "human", "next", &[next])
                .unwrap();
            db.attach_task_inputs(&task.task_id, "human", "next", &[next])
                .unwrap();
            assert!(matches!(
                db.attach_task_inputs(&task.task_id, "human", "next", &[seq]),
                Err(Error::Conflict)
            ));
        }
        let queued = db.enqueue_inbox_dispatch(&input("followup", &task), &[next]);
        if ["processed", "unattached"].contains(&case) {
            assert!(queued.is_err(), "{case}");
            continue;
        }
        queued.unwrap();
        assert_eq!(
            db.canonical_task(&task.task_id).unwrap().status,
            TaskState::Done
        );
        assert!(
            db.claim_dispatch("runner", 2002, 1000, 2000, 8)
                .unwrap()
                .is_none()
        ); // Previous dispatch still live.
        db.complete_dispatch(&old, &json!({"result":"finished"}), 2003)
            .unwrap();
        let claimed = db.claim_dispatch("runner", 2004, 1000, 2000, 8).unwrap();
        if case != "fresh" {
            assert!(claimed.is_none(), "{case}");
            assert_eq!(
                db.canonical_task(&task.task_id).unwrap().status,
                TaskState::Done
            );
            continue;
        }
        let current = claimed.unwrap();
        assert_eq!(
            db.canonical_task(&task.task_id).unwrap().status,
            TaskState::Done
        );
        inspect.execute_batch("CREATE TRIGGER fail_followup BEFORE INSERT ON task_notices BEGIN SELECT RAISE(ABORT,'injected notice failure'); END;").unwrap();
        assert!(db.start_dispatch(&current, 2005).is_err());
        assert_eq!(
            db.canonical_task(&task.task_id).unwrap().execution_epoch,
            epoch
        );
        inspect.execute_batch("DROP TRIGGER fail_followup").unwrap();
        db.start_dispatch(&current, 2006).unwrap();
        let reopened = db.canonical_task(&task.task_id).unwrap();
        assert_eq!(reopened.status, TaskState::InProgress);
        assert_eq!(reopened.execution_epoch, epoch + 1);
        assert_eq!(reopened.completed_at, None);
        assert!(db.start_dispatch(&current, 2007).is_err());
        assert_eq!(count(&inspect, "task_notices"), 2);
        assert!(
            db.mutate_task(
                &old,
                &task.task_id,
                "stale",
                &TaskMutation::Comment {
                    text: "late".into()
                },
                2007
            )
            .is_err()
        );
        assert_eq!(
            db.runner_inbox(&current, 0, 100, 2007).unwrap()[0]
                .message
                .sequence,
            next
        );
    }
}
