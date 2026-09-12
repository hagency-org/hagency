use super::*;
use hagency_core::peers::*;

fn change(key: &str, revision: u64, members: Option<&[String]>) -> ConversationChange {
    ConversationChange {
        call_id: key.into(),
        expected_revision: revision,
        action: match members {
            Some(ids) => ConversationAction::Members {
                participant_engagements: ids.to_vec(),
            },
            None => ConversationAction::Close {},
        },
    }
}
fn state(db: &rusqlite::Connection, id: &str) -> String {
    db.query_row(
        "SELECT state FROM runner_dispatches WHERE id=?1",
        [id],
        |r| r.get(0),
    )
    .unwrap()
}
fn peer(group: &Conversation, key: &str, target: &str) -> PeerSend {
    PeerSend {
        call_id: key.into(),
        conversation_id: group.id.clone(),
        recipient_session_ids: vec![target.into()],
        kind: PeerKind::Request,
        priority: PeerPriority::Normal,
        summary: "coordination".into(),
        body: String::new(),
        data: json!({}),
    }
}

#[test]
fn native_conversation_membership_lifecycle() {
    let (root, mut db, agents, cap) = setup();
    let group = db
        .create_internal_conversation(&cap, &request_group("group", &agents[1]), 1003)
        .unwrap()
        .conversation;
    let old = participant(&group, &agents[1]).to_owned();
    db.enqueue_dispatch(&dispatch("worker", &old, None))
        .unwrap();
    let worker = claim(&mut db, 1004);
    db.start_dispatch(&worker, 1005).unwrap();
    let remove = change("remove", 0, Some(&agents[..1]));
    assert!(
        db.change_internal_conversation(&worker, &group.id, &remove, 1006)
            .is_err()
    );
    db.register_session(&SessionBinding {
        id: "creator_other_thread".into(),
        engagement_id: agents[0].clone(),
        room_id: "!project:example.test".into(),
        thread_root: Some("$other_thread".into()),
    })
    .unwrap();
    db.enqueue_dispatch(&dispatch(
        "same_agent_wrong_session",
        "creator_other_thread",
        None,
    ))
    .unwrap();
    let other = claim(&mut db, 1006);
    db.start_dispatch(&other, 1007).unwrap();
    assert!(
        db.change_internal_conversation(&other, &group.id, &remove, 1008)
            .is_err()
    );
    assert!(
        db.change_internal_conversation(
            &cap,
            &group.id,
            &change("foreign", 0, Some(&agents[2..])),
            1008
        )
        .is_err()
    );
    assert!(
        db.change_internal_conversation(&cap, &group.id, &remove, 200_000)
            .is_err()
    );
    db.park_dispatch(&cap, true, 1008).unwrap();
    assert!(
        db.change_internal_conversation(&cap, &group.id, &remove, 1009)
            .is_err()
    );
    db.park_dispatch(&cap, false, 1009).unwrap();
    let inspect = sql(&root);
    inspect.execute_batch("CREATE TRIGGER fail_membership_receipt BEFORE INSERT ON conversation_operations BEGIN SELECT RAISE(ABORT,'fixture failure'); END;").unwrap();
    assert!(
        db.change_internal_conversation(&cap, &group.id, &remove, 1010)
            .is_err()
    );
    assert_eq!(
        db.runner_conversation(&cap, &group.id, 1010)
            .unwrap()
            .revision,
        0
    );
    assert_eq!(state(&inspect, "worker"), "started");
    assert_eq!(count(&inspect, "dispatch_stops"), 0);
    db.check_runner(&worker, 1010).unwrap();
    inspect
        .execute_batch("DROP TRIGGER fail_membership_receipt")
        .unwrap();
    let removed = db
        .change_internal_conversation(&cap, &group.id, &remove, 1011)
        .unwrap();
    assert_eq!(removed.conversation.revision, 1);
    assert_eq!(removed.conversation.participants.len(), 1);
    assert!(db.check_runner(&worker, 1012).is_err());
    assert!(
        db.change_internal_conversation(&cap, &group.id, &change("stale", 0, None), 1012)
            .is_err()
    );
    assert!(
        db.change_internal_conversation(&cap, &group.id, &change("remove", 0, None), 1012)
            .is_err()
    );
    let add = change("add", 1, Some(&agents[..2]));
    inspect.execute_batch("CREATE TRIGGER fail_rejoin_receipt BEFORE INSERT ON conversation_operations BEGIN SELECT RAISE(ABORT,'fixture failure'); END;").unwrap();
    assert!(
        db.change_internal_conversation(&cap, &group.id, &add, 1013)
            .is_err()
    );
    assert_eq!(count(&inspect, "runner_sessions"), 6);
    assert_eq!(
        db.runner_conversation(&cap, &group.id, 1013)
            .unwrap()
            .revision,
        1
    );
    inspect
        .execute_batch("DROP TRIGGER fail_rejoin_receipt")
        .unwrap();
    let joined = db
        .change_internal_conversation(&cap, &group.id, &add, 1013)
        .unwrap();
    let new = participant(&joined.conversation, &agents[1]).to_owned();
    assert_ne!(old, new);
    assert!(
        db.enqueue_dispatch(&dispatch("revive_old", &old, None))
            .is_err()
    );
    assert!(
        db.send_peer(&cap, &peer(&group, "stale_target", &old), 1014)
            .is_err()
    );
    let retry = db
        .change_internal_conversation(&cap, &group.id, &remove, 1014)
        .unwrap();
    assert!(retry.replayed);
    assert_eq!(retry.conversation.revision, 1);
    assert_eq!(
        db.runner_conversation(&cap, &group.id, 1014)
            .unwrap()
            .revision,
        2
    );
    let mut reordered = add;
    if let ConversationAction::Members {
        participant_engagements,
    } = &mut reordered.action
    {
        participant_engagements.reverse();
    }
    assert!(
        db.change_internal_conversation(&cap, &group.id, &reordered, 1014)
            .unwrap()
            .replayed
    );
    db.enqueue_dispatch(&dispatch("new_worker", &new, None))
        .unwrap();
    let new_worker = claim(&mut db, 1015);
    db.start_dispatch(&new_worker, 1016).unwrap();
    db.runner_conversation(&new_worker, &group.id, 1017)
        .unwrap();
    assert!(db.peer_inbox(&new, 0, 100).unwrap().is_empty());
    assert_eq!(count(&inspect, "runner_sessions"), 7); // Includes the retained old incarnation.
    assert!(
        db.settle_conversation_stop("worker", worker.fence + 1, "fixture inspection", 1018)
            .is_err()
    );
    db.settle_conversation_stop("worker", worker.fence, "fixture inspection", 1018)
        .unwrap();
    assert!(db.check_runner(&worker, 1019).is_err());
    assert!(
        db.enqueue_dispatch(&dispatch("still_retired", &old, None))
            .is_err()
    );
}

#[test]
fn native_conversation_close_custody() {
    for phase in ["queued", "leased", "started", "parked"] {
        let (root, mut db, agents, cap) = setup();
        let group = db
            .create_internal_conversation(&cap, &request_group("group", &agents[1]), 1003)
            .unwrap()
            .conversation;
        let target = participant(&group, &agents[1]).to_owned();
        db.register_workspace("workspace").unwrap();
        db.create_canonical_task("work", &target, "Work remains unfinished", 1004)
            .unwrap();
        let mut input = dispatch("worker", &target, Some("work"));
        input.resources = vec![ResourceLease {
            id: "workspace".into(),
            exclusive: true,
        }];
        db.enqueue_dispatch(&input).unwrap();
        let worker = if phase != "queued" {
            Some(claim(&mut db, 1005))
        } else {
            None
        };
        if ["started", "parked"].contains(&phase) {
            db.start_dispatch(worker.as_ref().unwrap(), 1006).unwrap();
        }
        let mut child = None;
        if phase == "parked" {
            child = Some(
                db.create_internal_conversation(
                    worker.as_ref().unwrap(),
                    &request_group("child", &agents[0]),
                    1007,
                )
                .unwrap()
                .conversation,
            );
            let child_target = participant(child.as_ref().unwrap(), &agents[0]);
            db.enqueue_dispatch(&dispatch("child_queue", child_target, None))
                .unwrap();
            db.park_dispatch(worker.as_ref().unwrap(), true, 1008)
                .unwrap();
        }
        db.enqueue_dispatch(&dispatch("old_queue", &target, None))
            .unwrap();
        let mut competing = dispatch("competitor", "b", None);
        competing.resources = input.resources.clone();
        db.enqueue_dispatch(&competing).unwrap();
        let before = db.canonical_task("work").unwrap();
        let closing = change("close", 0, None);
        let result = db
            .change_internal_conversation(&cap, &group.id, &closing, 1009)
            .unwrap();
        assert_eq!(result.conversation.state, "closed");
        assert!(
            db.change_internal_conversation(&cap, &group.id, &closing, 1010)
                .unwrap()
                .replayed
        );
        assert!(
            db.change_internal_conversation(
                &cap,
                &group.id,
                &change("reopen", 1, Some(&agents[..2])),
                1010
            )
            .is_err()
        );
        let inspect = sql(&root);
        assert_eq!(state(&inspect, "old_queue"), "superseded");
        if let Some(child) = child {
            let child_state: String = inspect
                .query_row(
                    "SELECT state FROM internal_conversations WHERE id=?1",
                    [child.id],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(child_state, "closed");
            assert_eq!(state(&inspect, "child_queue"), "superseded");
        }
        let uncertain = ["started", "parked"].contains(&phase);
        assert_eq!(
            state(&inspect, "worker"),
            if uncertain {
                "outcome_unknown"
            } else {
                "superseded"
            }
        );
        assert_eq!(count(&inspect, "resource_leases"), u64::from(uncertain));
        assert_eq!(value(db.canonical_task("work").unwrap()), value(&before));
        if let Some(worker) = &worker {
            assert!(db.start_dispatch(worker, 1011).is_err());
            assert!(db.check_runner(worker, 1011).is_err());
            assert!(
                db.complete_dispatch(worker, &json!({"done":true}), 1011)
                    .is_err()
            );
        }
        if uncertain {
            assert!(
                db.claim_dispatch("competitor", 1012, 1000, 1000, 8)
                    .unwrap()
                    .is_none()
            );
            let stop = db.pending_conversation_stops("", 100).unwrap();
            assert_eq!(stop.len(), 1);
            drop(db);
            db = DomainRepository::open(&root.path().join("state")).unwrap();
            assert_eq!(db.pending_conversation_stops("", 100).unwrap(), stop);
            assert_eq!(count(&inspect, "resource_leases"), 1);
            assert!(
                db.claim_dispatch("competitor", 1013, 1000, 1000, 8)
                    .unwrap()
                    .is_none()
            );
            let worker = worker.as_ref().unwrap();
            let mut recovery = input.clone();
            recovery.id = "bad_recovery".into();
            recovery.payload = json!({"instruction":"resume anyway"});
            assert!(
                db.recover_dispatch("worker", &recovery, "fixture inspection", 1014)
                    .is_err()
            );
            inspect.execute_batch("CREATE TRIGGER fail_stop_settlement BEFORE DELETE ON resource_leases BEGIN SELECT RAISE(ABORT,'fixture failure'); END;").unwrap();
            assert!(
                db.settle_conversation_stop("worker", worker.fence, "fixture inspection", 1014)
                    .is_err()
            );
            assert_eq!(db.pending_conversation_stops("", 100).unwrap(), stop);
            inspect
                .execute_batch("DROP TRIGGER fail_stop_settlement")
                .unwrap();
            db.settle_conversation_stop("worker", worker.fence, "fixture inspection", 1015)
                .unwrap();
            db.settle_conversation_stop("worker", worker.fence, "fixture inspection", 1016)
                .unwrap();
            assert!(
                db.settle_conversation_stop("worker", worker.fence, "different inspection", 1016)
                    .is_err()
            );
            assert!(db.pending_conversation_stops("", 100).unwrap().is_empty());
            assert_eq!(count(&inspect, "resource_leases"), 0);
            assert_eq!(value(db.canonical_task("work").unwrap()), value(&before));
        }
        assert_eq!(
            db.claim_dispatch("competitor", 1017, 1000, 1000, 8)
                .unwrap()
                .unwrap()
                .dispatch_id,
            "competitor"
        );
    }
    shared_custody();
}

fn shared_custody() {
    let (root, mut db, agents, cap) = setup();
    let group = db
        .create_internal_conversation(&cap, &request_group("group", &agents[1]), 1003)
        .unwrap()
        .conversation;
    db.register_workspace("shared").unwrap();
    let mut workers = Vec::new();
    for (index, agent) in agents[..2].iter().enumerate() {
        let mut input = dispatch(&format!("reader_{index}"), participant(&group, agent), None);
        input.resources = vec![ResourceLease {
            id: "shared".into(),
            exclusive: false,
        }];
        db.enqueue_dispatch(&input).unwrap();
        let worker = claim(&mut db, 1004);
        db.start_dispatch(&worker, 1005).unwrap();
        workers.push(worker);
    }
    let mut write = dispatch("writer", "b", None);
    write.resources = vec![ResourceLease {
        id: "shared".into(),
        exclusive: true,
    }];
    db.enqueue_dispatch(&write).unwrap();
    db.change_internal_conversation(&cap, &group.id, &change("close", 0, None), 1006)
        .unwrap();
    let inspect = sql(&root);
    assert_eq!(count(&inspect, "resource_leases"), 2);
    assert!(
        db.claim_dispatch("host", 1007, 1000, 1000, 8)
            .unwrap()
            .is_none()
    );
    db.settle_conversation_stop(
        &workers[0].dispatch_id,
        workers[0].fence,
        "fixture first inspected",
        1008,
    )
    .unwrap();
    assert_eq!(count(&inspect, "resource_leases"), 1);
    assert!(
        db.claim_dispatch("host", 1009, 1000, 1000, 8)
            .unwrap()
            .is_none()
    );
    // Pending stop still occupies a runtime slot even when no workspace is needed.
    db.enqueue_dispatch(&dispatch("no_workspace", "c", None))
        .unwrap();
    assert!(
        db.claim_dispatch("host", 1010, 1000, 1000, 2)
            .unwrap()
            .is_none()
    );
    db.settle_conversation_stop(
        &workers[1].dispatch_id,
        workers[1].fence,
        "fixture second inspected",
        1011,
    )
    .unwrap();
    assert_eq!(
        db.claim_dispatch("host", 1012, 1000, 1000, 2)
            .unwrap()
            .unwrap()
            .dispatch_id,
        "writer"
    );
}

#[test]
fn native_conversation_close_inbox() {
    let (root, mut db, agents, cap) = setup();
    let mut groups = Vec::new();
    let mut sequences = Vec::new();
    for key in ["closing", "remaining"] {
        let group = db
            .create_internal_conversation(&cap, &request_group(key, &agents[1]), 1003)
            .unwrap()
            .conversation;
        db.enqueue_dispatch(&dispatch(key, participant(&group, &agents[1]), None))
            .unwrap();
        let worker = claim(&mut db, 1004);
        db.start_dispatch(&worker, 1005).unwrap();
        sequences.push(
            db.send_peer(&worker, &peer(&group, key, "a"), 1006)
                .unwrap()
                .sequence,
        );
        db.complete_dispatch(&worker, &json!({}), 1007).unwrap();
        groups.push(group);
    }
    db.complete_dispatch(&cap, &json!({}), 1008).unwrap();
    db.enqueue_peer_dispatch(&dispatch("batch", "a", None), &sequences)
        .unwrap();
    let batch = claim(&mut db, 1009);
    db.start_dispatch(&batch, 1010).unwrap();
    db.change_internal_conversation(&batch, &groups[0].id, &change("close", 0, None), 1011)
        .unwrap();
    assert!(db.check_runner(&batch, 1012).is_err());
    assert_eq!(
        db.pending_conversation_stops("", 100).unwrap(),
        vec![("batch".into(), batch.fence)]
    );
    let inspect = sql(&root);
    assert_eq!(count(&inspect, "peer_messages"), 2);
    assert!(db.peer_inbox("a", 0, 100).unwrap().is_empty()); // Valid input remains assigned until inspection.
    db.settle_conversation_stop("batch", batch.fence, "fixture inspection", 1013)
        .unwrap();
    let inbox = db.peer_inbox("a", 0, 100).unwrap();
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0].message.sequence, sequences[1]);
    let processed: u64 = inspect
        .query_row(
            "SELECT COUNT(*) FROM peer_session_inputs WHERE processed_at IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(processed, 0);
    db.enqueue_peer_dispatch(&dispatch("remaining_batch", "a", None), &[sequences[1]])
        .unwrap();
    let remaining = claim(&mut db, 1014);
    let payload = db.start_dispatch(&remaining, 1015).unwrap();
    assert_eq!(payload["peerInbox"].as_array().unwrap().len(), 1);
    assert_eq!(
        payload["peerInbox"][0]["message"]["conversation_id"],
        groups[1].id
    );
    assert!(
        db.runner_conversation(&remaining, &groups[0].id, 1016)
            .is_err()
    );
    db.runner_conversation(&remaining, &groups[1].id, 1016)
        .unwrap();
}
