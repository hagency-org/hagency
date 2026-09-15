use super::*;
use hagency_core::peers::{PeerKind, PeerPriority, PeerSend};

fn batch(group: &Conversation, recipient: &str, call: &str, size: usize) -> WorkflowRequest {
    WorkflowRequest {
        call_id: call.into(),
        conversation_id: group.id.clone(),
        definition: GraphDefinition {
            label: "Quota regression".into(),
            nodes: (0..size)
                .map(|i| node(&format!("node_{i}"), recipient, &[]))
                .collect(),
        },
    }
}

fn ordinary_peer(group: &Conversation, recipient: &str, call: &str) -> PeerSend {
    PeerSend {
        call_id: call.into(),
        conversation_id: group.id.clone(),
        recipient_session_ids: vec![recipient.into()],
        kind: PeerKind::Request,
        priority: PeerPriority::Normal,
        summary: "Independent follow-up".into(),
        body: "History retirement must not prevent future work".into(),
        data: Value::Null,
    }
}

fn pending(inspect: &rusqlite::Connection, recipient: &str, live_only: bool) -> u64 {
    let query = if live_only {
        "SELECT COUNT(*) FROM peer_session_inputs i JOIN live_peer_inputs l ON l.session_id=i.session_id AND l.message_sequence=i.message_sequence WHERE i.session_id=?1 AND i.processed_at IS NULL"
    } else {
        "SELECT COUNT(*) FROM peer_session_inputs WHERE session_id=?1 AND processed_at IS NULL"
    };
    inspect.query_row(query, [recipient], |r| r.get(0)).unwrap()
}

#[test]
fn native_graph_cancellation_releases_pending_quota() {
    let (root, mut db, agents, creator) = setup_with_parent(true);
    let group = make_group(&mut db, &agents, &creator);
    let recipient = participant(&group, &agents[1]);
    let inspect = sql(&root);
    let mut now = 1004;

    // Sixteen bounded transactions leave more than the 2,000-input quota in
    // immutable cancelled history, all for the same still-current member SID.
    for i in 0..16 {
        let request = batch(&group, recipient, &format!("cancelled_{i}"), 128);
        let graph = db
            .create_workflow(&creator, &request, now)
            .unwrap()
            .workflow;
        assert!(
            graph
                .nodes
                .iter()
                .all(|n| n.binding.session_id == recipient)
        );
        now += 1;
        db.cancel_workflow(
            &creator,
            &graph.id,
            &WorkflowCancel {
                call_id: format!("cancel_{i}"),
            },
            now,
        )
        .unwrap();
        now += 1;
        assert_eq!(pending(&inspect, recipient, true), 0);
    }
    assert_eq!(pending(&inspect, recipient, false), 2048);
    assert_eq!(count(&inspect, "peer_messages"), 2048);
    assert_eq!(count(&inspect, "runner_sessions"), 5);
    db.send_peer(
        &creator,
        &ordinary_peer(&group, recipient, "after_history"),
        now,
    )
    .unwrap();
    now += 1;

    // Keep exactly 2,000 live inputs. Retired history is excluded from pending
    // capacity, while genuinely live work still consumes every available slot.
    let mut first_live = None;
    for i in 0..16 {
        let size = if i == 15 { 79 } else { 128 };
        let graph = db
            .create_workflow(
                &creator,
                &batch(&group, recipient, &format!("live_{i}"), size),
                now,
            )
            .unwrap()
            .workflow;
        first_live.get_or_insert(graph.id);
        now += 1;
    }
    assert_eq!(pending(&inspect, recipient, true), 2000);
    assert_eq!(pending(&inspect, recipient, false), 4048);
    let full_graph = batch(&group, recipient, "at_capacity", 1);
    let full_peer = ordinary_peer(&group, recipient, "at_capacity");
    let graph_count = count(&inspect, "task_graphs");
    let task_count = count(&inspect, "canonical_tasks");
    let message_count = count(&inspect, "peer_messages");
    assert!(matches!(
        db.create_workflow(&creator, &full_graph, now),
        Err(Error::Capacity)
    ));
    assert!(matches!(
        db.send_peer(&creator, &full_peer, now),
        Err(Error::Capacity)
    ));
    assert_eq!(count(&inspect, "task_graphs"), graph_count);
    assert_eq!(count(&inspect, "canonical_tasks"), task_count);
    assert_eq!(count(&inspect, "peer_messages"), message_count);

    db.cancel_workflow(
        &creator,
        &first_live.unwrap(),
        &WorkflowCancel {
            call_id: "free_live_capacity".into(),
        },
        now,
    )
    .unwrap();
    now += 1;
    assert_eq!(pending(&inspect, recipient, true), 1872);
    assert_eq!(pending(&inspect, recipient, false), 4048);
    assert!(
        !db.create_workflow(&creator, &full_graph, now)
            .unwrap()
            .replayed
    );
    assert!(!db.send_peer(&creator, &full_peer, now).unwrap().replayed);
    assert_eq!(pending(&inspect, recipient, true), 1874);
    assert_eq!(pending(&inspect, recipient, false), 4050);
    assert_eq!(
        inspect
            .query_row(
                "SELECT COUNT(*) FROM peer_session_inputs WHERE processed_at IS NOT NULL",
                [],
                |r| r.get::<_, u64>(0)
            )
            .unwrap(),
        0
    );
    assert_eq!(
        db.canonical_task("parent").unwrap().status,
        TaskState::InProgress
    );
}
