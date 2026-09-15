mod common;
use common::*;
use hagency_core::{conversations::*, peers::*, tasks::*};
use hagency_store::{DomainRepository, EffectOutcome, Error};
use serde_json::json;

fn dispatch(id: &str, session: &str, task: Option<&str>) -> DispatchInput {
    DispatchInput {
        id: id.into(),
        session_id: session.into(),
        task_id: task.map(str::to_owned),
        resources: vec![],
        payload: json!({"instruction":"Verify internal task ownership"}),
    }
}
fn claim(db: &mut DomainRepository, now: u64) -> RunnerCapability {
    db.claim_dispatch("runner", now, 60_000, 120_000, 8)
        .unwrap()
        .unwrap()
}
fn setup() -> (
    tempfile::TempDir,
    DomainRepository,
    Vec<String>,
    RunnerCapability,
) {
    let root = tempfile::tempdir().unwrap();
    let mut db = DomainRepository::open(&root.path().join("state")).unwrap();
    db.register(&registration()).unwrap();
    let pool = resource("pool", "seat", 1000);
    db.put_resource(&pool).unwrap();
    let mut agents = Vec::new();
    for (id, name) in [("a", "小白"), ("b", "Edison"), ("c", "Other")] {
        let mut req = request(id, name, &pool, 100);
        if id == "c" {
            req.target_project_id = "other_project".into();
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
        agents.push(e.id);
    }
    db.enqueue_dispatch(&dispatch("creator", "a", None))
        .unwrap();
    let cap = claim(&mut db, 1001);
    db.start_dispatch(&cap, 1002).unwrap();
    (root, db, agents, cap)
}
fn request_group(key: &str, target: &str) -> ConversationRequest {
    ConversationRequest {
        call_id: key.into(),
        label: "协作任务".into(),
        participant_engagements: vec![target.into()],
    }
}
fn participant<'a>(group: &'a Conversation, agent: &str) -> &'a str {
    &group
        .participants
        .iter()
        .find(|p| p.engagement_id == agent)
        .unwrap()
        .id
}
fn sql(root: &tempfile::TempDir) -> rusqlite::Connection {
    rusqlite::Connection::open(root.path().join("state/domain.sqlite3")).unwrap()
}
fn count(db: &rusqlite::Connection, table: &str) -> u64 {
    db.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

fn send(group: &Conversation, key: &str, targets: &[&str], kind: PeerKind) -> PeerSend {
    PeerSend {
        call_id: key.into(),
        conversation_id: group.id.clone(),
        recipient_session_ids: targets.iter().map(|v| (*v).into()).collect(),
        kind,
        priority: PeerPriority::Normal,
        summary: "请核对结果".into(),
        body: "Keep each recipient's input independent".into(),
        data: json!({"score":0.75}),
    }
}

#[test]
fn native_peer_admission_scope() {
    let (root, mut db, agents, cap) = setup();
    let group = db
        .create_internal_conversation(&cap, &request_group("group", &agents[1]), 1003)
        .unwrap()
        .conversation;
    let a = participant(&group, &agents[0]);
    let b = participant(&group, &agents[1]);
    let request = send(&group, "atomic", &[a, b], PeerKind::Request);
    let inspect = sql(&root);
    // Fail after the first projection: the message receipt must roll back too.
    inspect.execute_batch("CREATE TRIGGER fail_peer BEFORE INSERT ON peer_session_inputs WHEN (SELECT COUNT(*) FROM peer_session_inputs)>0 BEGIN SELECT RAISE(ABORT,'injected projection failure'); END;").unwrap();
    assert!(db.send_peer(&cap, &request, 1004).is_err());
    assert_eq!(count(&inspect, "peer_messages"), 0);
    assert_eq!(count(&inspect, "peer_session_inputs"), 0);
    inspect.execute_batch("DROP TRIGGER fail_peer").unwrap();
    let receipt = db.send_peer(&cap, &request, 1005).unwrap();
    assert_eq!(receipt.recipients, 2);
    let mut reverse = request.clone();
    reverse.recipient_session_ids.reverse();
    assert!(db.send_peer(&cap, &reverse, 1006).unwrap().replayed);
    reverse.data = json!({"score":0.5});
    assert!(matches!(
        db.send_peer(&cap, &reverse, 1006),
        Err(Error::Conflict)
    ));
    for target in ["b", "c"] {
        // A participant's ordinary Matrix room is not this conversation.
        assert!(
            db.send_peer(
                &cap,
                &send(&group, target, &[target], PeerKind::Request),
                1007
            )
            .is_err()
        );
    }
    let item = &db.peer_inbox(b, 0, 1).unwrap()[0];
    assert_eq!(item.message.source_session_id, "a");
    assert_eq!(item.message.source_engagement_id, agents[0]);
    assert_eq!(item.message.data["score"], 0.75);
    let mut forged = value(&request);
    forged["source_session_id"] = json!("a");
    assert!(serde_json::from_value::<PeerSend>(forged).is_err());
    let mut duplicate = request.clone();
    duplicate.recipient_session_ids = vec![b.into(), b.into()];
    assert!(db.send_peer(&cap, &duplicate, 1008).is_err());
    db.enqueue_dispatch(&dispatch("foreign", "b", None))
        .unwrap();
    let foreign = claim(&mut db, 1009);
    db.start_dispatch(&foreign, 1010).unwrap();
    assert!(db.send_peer(&foreign, &request, 1011).is_err());
    assert!(db.send_peer(&cap, &request, 200_000).is_err());
    db.park_dispatch(&cap, true, 1012).unwrap();
    assert!(db.send_peer(&cap, &request, 1013).is_err());
    db.park_dispatch(&cap, false, 1014).unwrap();
    db.revoke("revoke_b", &agents[1]).unwrap();
    assert!(db.send_peer(&cap, &request, 1015).is_err());
    assert_eq!(count(&inspect, "peer_messages"), 1);
}

#[test]
fn native_peer_dispatch_ownership() {
    let (root, mut db, agents, cap) = setup();
    let group = db
        .create_internal_conversation(&cap, &request_group("group", &agents[1]), 1003)
        .unwrap()
        .conversation;
    let a = participant(&group, &agents[0]);
    let b = participant(&group, &agents[1]);
    let note = db
        .send_peer(
            &cap,
            &send(&group, "note", &[a, b], PeerKind::Notification),
            1004,
        )
        .unwrap();
    assert!(!db.peer_inbox(b, 0, 1).unwrap()[0].wake);
    assert!(
        db.enqueue_peer_dispatch(&dispatch("note_only", b, None), &[note.sequence])
            .is_err()
    );
    let request = db
        .send_peer(
            &cap,
            &send(&group, "request", &[a, b], PeerKind::Request),
            1005,
        )
        .unwrap();
    assert_eq!(db.peer_inbox(b, 0, 100).unwrap().len(), 2);
    assert_eq!(db.peer_inbox(b, 0, 100).unwrap().len(), 2); // Reads never acknowledge.
    assert_eq!(
        db.peer_inbox(b, note.sequence, 1).unwrap()[0]
            .message
            .sequence,
        request.sequence
    );
    assert!(db.peer_inbox(b, 0, 101).is_err());
    assert!(db.peer_inbox(b, u64::MAX, 1).is_err());
    let input = dispatch("worker", b, None);
    assert!(
        db.enqueue_peer_dispatch(&dispatch("foreign_input", "b", None), &[request.sequence])
            .is_err()
    );
    let inspect = sql(&root);
    inspect.execute_batch("CREATE TRIGGER fail_peer_claim BEFORE INSERT ON peer_dispatch_inputs BEGIN SELECT RAISE(ABORT,'injected claim failure'); END;").unwrap();
    assert!(
        db.enqueue_peer_dispatch(&input, &[note.sequence, request.sequence])
            .is_err()
    );
    assert_eq!(count(&inspect, "runner_dispatches"), 1);
    assert_eq!(db.peer_inbox(b, 0, 100).unwrap().len(), 2);
    inspect
        .execute_batch("DROP TRIGGER fail_peer_claim")
        .unwrap();
    db.enqueue_peer_dispatch(&input, &[request.sequence, note.sequence])
        .unwrap();
    db.enqueue_peer_dispatch(&input, &[note.sequence, request.sequence])
        .unwrap();
    assert!(
        db.enqueue_peer_dispatch(&dispatch("steal", b, None), &[request.sequence])
            .is_err()
    );
    let worker = claim(&mut db, 1006);
    let payload = db.start_dispatch(&worker, 1007).unwrap();
    assert_eq!(payload["peerInbox"].as_array().unwrap().len(), 2);
    let response = db
        .send_peer(
            &cap,
            &send(&group, "response", &[b], PeerKind::Response),
            1008,
        )
        .unwrap();
    assert_eq!(
        db.runner_peer_inbox(&worker, 0, 100, 1009).unwrap().len(),
        2
    );
    assert_eq!(
        db.peer_inbox(b, 0, 100).unwrap()[0].message.sequence,
        response.sequence
    );
    inspect.execute_batch("CREATE TRIGGER fail_peer_ack BEFORE UPDATE OF processed_at ON peer_session_inputs BEGIN SELECT RAISE(ABORT,'injected ack failure'); END;").unwrap();
    assert!(
        db.complete_dispatch(&worker, &json!({"result":"ok"}), 1010)
            .is_err()
    );
    assert_eq!(
        db.runner_peer_inbox(&worker, 0, 100, 1011).unwrap().len(),
        2
    );
    inspect.execute_batch("DROP TRIGGER fail_peer_ack").unwrap();
    db.complete_dispatch(&worker, &json!({"result":"ok"}), 1012)
        .unwrap();
    assert_eq!(db.peer_inbox(a, 0, 100).unwrap().len(), 2); // Other recipient's copy survives.
    assert_eq!(db.peer_inbox(b, 0, 100).unwrap().len(), 1);
    db.enqueue_peer_dispatch(&dispatch("response", b, None), &[response.sequence])
        .unwrap();
    let response_cap = claim(&mut db, 1013);
    db.start_dispatch(&response_cap, 1014).unwrap();
    assert_eq!(
        db.runner_peer_inbox(&response_cap, 0, 100, 1015).unwrap()[0]
            .message
            .kind,
        PeerKind::Response
    );
    db.complete_dispatch(&response_cap, &json!({}), 1016)
        .unwrap();
    // Closing a conversation must also prevent queued deliveries to its Matrix creator.
    let reply = db
        .send_peer(
            &cap,
            &send(&group, "creator_reply", &["a"], PeerKind::Response),
            1017,
        )
        .unwrap();
    db.enqueue_peer_dispatch(&dispatch("creator_reply", "a", None), &[reply.sequence])
        .unwrap();
    db.complete_dispatch(&cap, &json!({}), 1018).unwrap();
    inspect
        .execute(
            "UPDATE internal_conversations SET state='closed' WHERE id=?1",
            [&group.id],
        )
        .unwrap();
    assert!(
        db.claim_dispatch("runner", 1019, 1000, 2000, 8)
            .unwrap()
            .is_none()
    );
}

#[test]
fn native_peer_input_recovery() {
    let (root, mut db, agents, cap) = setup();
    let group = db
        .create_internal_conversation(&cap, &request_group("group", &agents[1]), 1003)
        .unwrap()
        .conversation;
    let b = participant(&group, &agents[1]);
    let first = db
        .send_peer(&cap, &send(&group, "first", &[b], PeerKind::Request), 1004)
        .unwrap();
    db.enqueue_peer_dispatch(&dispatch("first", b, None), &[first.sequence])
        .unwrap();
    let worker = claim(&mut db, 1005);
    db.start_dispatch(&worker, 1006).unwrap();
    let second = db
        .send_peer(&cap, &send(&group, "second", &[b], PeerKind::Request), 1007)
        .unwrap();
    db.enqueue_peer_dispatch(&dispatch("queued", b, None), &[second.sequence])
        .unwrap();
    // Keep the creator alive in a fresh attempt after the repository restarts.
    db.complete_dispatch(&cap, &json!({}), 1008).unwrap();
    drop(db);
    let mut db = DomainRepository::open(&root.path().join("state")).unwrap();
    assert!(db.runner_peer_inbox(&worker, 0, 100, 1009).is_err());
    db.enqueue_dispatch(&dispatch("new_creator", "a", None))
        .unwrap();
    let creator = claim(&mut db, 1010);
    db.start_dispatch(&creator, 1011).unwrap();
    let third = db
        .send_peer(
            &creator,
            &send(&group, "third", &[b], PeerKind::Response),
            1012,
        )
        .unwrap();
    assert_eq!(
        db.peer_inbox(b, 0, 100).unwrap()[0].message.sequence,
        third.sequence
    );
    let mut replacement = dispatch("recovery", b, None);
    replacement.payload = json!({"instruction":"Inspect uncertain work before continuation"});
    db.recover_dispatch(
        "first",
        &replacement,
        "Fixture inspection; no native process launched",
        1013,
    )
    .unwrap();
    let pending = db.peer_inbox(b, 0, 100).unwrap();
    assert_eq!(
        pending
            .iter()
            .map(|i| i.message.sequence)
            .collect::<Vec<_>>(),
        vec![second.sequence, third.sequence]
    );
    let recovered = claim(&mut db, 1014);
    let payload = db.start_dispatch(&recovered, 1015).unwrap();
    assert_eq!(
        payload["recoveryPeerInbox"][0]["message"]["sequence"],
        first.sequence
    );
    assert_eq!(
        db.runner_peer_inbox(&recovered, 0, 100, 1016)
            .unwrap()
            .len(),
        1
    );
    db.complete_dispatch(&recovered, &json!({}), 1017).unwrap();
    assert_eq!(db.peer_inbox(b, 0, 100).unwrap().len(), 2);
    assert!(db.complete_dispatch(&worker, &json!({}), 1018).is_err());
}
