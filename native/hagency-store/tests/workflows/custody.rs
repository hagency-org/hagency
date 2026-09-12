use super::*;

fn leases(inspect: &rusqlite::Connection) -> Vec<String> {
    inspect
        .prepare("SELECT dispatch_id FROM resource_leases ORDER BY dispatch_id")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn reader_input(w: &WorkflowView, node: &str, id: &str) -> DispatchInput {
    let n = binding(w, node);
    let mut input = dispatch(id, &n.session_id, Some(&n.task_id));
    input.resources = vec![ResourceLease {
        id: "shared".into(),
        exclusive: false,
    }];
    input
}

fn start_reader(
    db: &mut DomainRepository,
    w: &WorkflowView,
    node: &str,
    id: &str,
    now: u64,
) -> (DispatchInput, RunnerCapability) {
    let input = reader_input(w, node, id);
    db.enqueue_peer_dispatch(&input, &[binding(w, node).message_sequence.unwrap()])
        .unwrap();
    let cap = db
        .claim_dispatch("reader", now, 10, 120_000, 8)
        .unwrap()
        .unwrap();
    assert_eq!(cap.dispatch_id, id);
    db.start_dispatch(&cap, now + 1).unwrap();
    (input, cap)
}

fn queue_writer(db: &mut DomainRepository) {
    let mut input = dispatch("writer", "b", None);
    input.resources = vec![ResourceLease {
        id: "shared".into(),
        exclusive: true,
    }];
    db.enqueue_dispatch(&input).unwrap();
}

fn queue_controller(db: &mut DomainRepository) {
    // A fresh attempt retains the exact original creator SID, without borrowing
    // the unknown worker's capability or impersonating another Agent's session.
    let mut input = dispatch("controller", "a", Some("parent"));
    input.payload = json!({"instruction":"Inspect and cancel the stored graph"});
    db.enqueue_dispatch(&input).unwrap();
}

#[test]
fn native_graph_cancellation_unknown_readers() {
    for restart in [false, true] {
        let (root, mut db, agents, creator) = setup_with_parent(true);
        let group = make_group(&mut db, &agents, &creator);
        let w = db
            .create_workflow(&creator, &request(&group, &agents), 1004)
            .unwrap()
            .workflow;
        db.complete_dispatch(&creator, &json!({"delegated":true}), 1005)
            .unwrap();
        db.register_workspace("shared").unwrap();
        let (_, worker) = start_reader(&mut db, &w, "implement", "reader", 1010);
        let before = value(
            db.canonical_task(&binding(&w, "implement").task_id)
                .unwrap(),
        );
        if restart {
            drop(db);
            db = DomainRepository::open(&root.path().join("state")).unwrap();
        } else {
            db.reconcile_dispatches(1021).unwrap();
        }
        let inspect = sql(&root);
        assert_eq!(state(&inspect, "reader"), "outcome_unknown");
        assert_eq!(leases(&inspect), ["reader"]);
        assert!(db.pending_conversation_stops("", 100).unwrap().is_empty());
        assert!(db.check_runner(&worker, 1022).is_err());
        queue_controller(&mut db);
        // Ownership loss does not prove a process exited, even without a stop
        // intent yet. That unknown process still consumes the only live slot.
        assert!(
            db.claim_dispatch("host", 1022, 1000, 1000, 1)
                .unwrap()
                .is_none()
        );
        let controller = db
            .claim_dispatch("host", 1023, 1000, 1000, 2)
            .unwrap()
            .unwrap();
        assert_eq!(controller.dispatch_id, "controller");
        db.start_dispatch(&controller, 1024).unwrap();
        assert_eq!(
            db.runner_workflow(&controller, &w.id, 1024).unwrap().id,
            w.id
        );
        queue_writer(&mut db);
        assert!(
            db.claim_dispatch("writer", 1025, 1000, 1000, 8)
                .unwrap()
                .is_none()
        );
        db.cancel_workflow(
            &controller,
            &w.id,
            &WorkflowCancel {
                call_id: "cancel".into(),
            },
            1026,
        )
        .unwrap();
        assert_eq!(leases(&inspect), ["reader"]);
        assert_eq!(
            db.pending_conversation_stops("", 100).unwrap(),
            [("reader".into(), worker.fence)]
        );
        assert!(
            db.claim_dispatch("writer", 1027, 1000, 1000, 8)
                .unwrap()
                .is_none()
        );
        db.settle_conversation_stop(
            "reader",
            worker.fence,
            "fixture inspected the unknown reader after cancellation",
            1028,
        )
        .unwrap();
        assert!(leases(&inspect).is_empty());
        assert_eq!(claim(&mut db, 1029).dispatch_id, "writer");
        assert_eq!(
            value(
                db.canonical_task(&binding(&w, "implement").task_id)
                    .unwrap()
            ),
            before
        );
        assert_eq!(
            db.canonical_task("parent").unwrap().status,
            TaskState::InProgress
        );
    }
}

#[test]
fn native_graph_cancellation_inspection_isolation() {
    let (root, mut db, agents, creator) = setup_with_parent(true);
    let group = make_group(&mut db, &agents, &creator);
    let mut req = request(&group, &agents);
    req.definition.nodes[1].depends_on.clear();
    let w = db.create_workflow(&creator, &req, 1004).unwrap().workflow;
    db.complete_dispatch(&creator, &json!({"delegated":true}), 1005)
        .unwrap();
    db.register_workspace("shared").unwrap();
    let (first_input, first) = start_reader(&mut db, &w, "implement", "first", 1010);
    let (_, second) = start_reader(&mut db, &w, "verify", "second", 1012);
    db.reconcile_dispatches(1023).unwrap();
    let inspect = sql(&root);
    assert_eq!(leases(&inspect), ["first", "second"]);
    queue_writer(&mut db);
    queue_controller(&mut db);
    assert!(
        db.claim_dispatch("host", 1024, 1000, 1000, 2)
            .unwrap()
            .is_none()
    );
    let controller = db
        .claim_dispatch("host", 1025, 1000, 1000, 3)
        .unwrap()
        .unwrap();
    assert_eq!(controller.dispatch_id, "controller");
    db.start_dispatch(&controller, 1026).unwrap();
    let mut replacement = first_input.clone();
    replacement.id = "first_recovery".into();
    replacement.payload =
        json!({"instruction":"Continue only after inspecting the original reader"});
    let task_before = value(
        db.canonical_task(first_input.task_id.as_ref().unwrap())
            .unwrap(),
    );
    // Fail after custody release, replacement insertion and frozen-input transfer.
    // The whole inspection transaction must restore its original state.
    inspect.execute_batch("CREATE TRIGGER reject_inspection BEFORE INSERT ON dispatch_recoveries BEGIN SELECT RAISE(ABORT,'fixture inspection failure'); END;").unwrap();
    assert!(
        db.recover_dispatch(
            "first",
            &replacement,
            "fixture first reader inspected",
            1027
        )
        .is_err()
    );
    assert_eq!(leases(&inspect), ["first", "second"]);
    assert_eq!(count(&inspect, "dispatch_recoveries"), 0);
    assert_eq!(
        inspect
            .query_row(
                "SELECT COUNT(*) FROM runner_dispatches WHERE id='first_recovery'",
                [],
                |r| r.get::<_, u64>(0),
            )
            .unwrap(),
        0
    );
    let assigned: String = inspect
        .query_row(
            "SELECT dispatch_id FROM peer_session_inputs WHERE message_sequence=?1",
            [binding(&w, "implement").message_sequence.unwrap()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(assigned, "first");
    assert_eq!(
        value(
            db.canonical_task(first_input.task_id.as_ref().unwrap())
                .unwrap()
        ),
        task_before
    );
    inspect
        .execute_batch("DROP TRIGGER reject_inspection")
        .unwrap();
    db.recover_dispatch(
        "first",
        &replacement,
        "fixture first reader inspected",
        1028,
    )
    .unwrap();
    assert_eq!(leases(&inspect), ["second"]);
    assert_eq!(count(&inspect, "dispatch_recoveries"), 1);
    assert_eq!(state(&inspect, "first_recovery"), "queued");
    assert!(db.check_runner(&first, 1029).is_err());
    // Cancellation supersedes the unstarted replacement. The inspected original
    // must not regain a stop intent or leases; the other unknown reader still does.
    db.cancel_workflow(
        &controller,
        &w.id,
        &WorkflowCancel {
            call_id: "cancel".into(),
        },
        1030,
    )
    .unwrap();
    assert_eq!(state(&inspect, "first_recovery"), "superseded");
    assert_eq!(leases(&inspect), ["second"]);
    assert_eq!(
        db.pending_conversation_stops("", 100).unwrap(),
        [("second".into(), second.fence)]
    );
    assert!(
        db.claim_dispatch("writer", 1031, 1000, 1000, 8)
            .unwrap()
            .is_none()
    );
    db.settle_conversation_stop(
        "second",
        second.fence,
        "fixture second reader inspected separately",
        1032,
    )
    .unwrap();
    assert!(leases(&inspect).is_empty());
    assert_eq!(claim(&mut db, 1033).dispatch_id, "writer");
    assert_eq!(
        value(
            db.canonical_task(first_input.task_id.as_ref().unwrap())
                .unwrap()
        ),
        task_before
    );
}

#[test]
fn native_graph_cancellation_schema9_custody() {
    let (root, mut db, agents, creator) = setup_with_parent(true);
    let group = make_group(&mut db, &agents, &creator);
    db.complete_dispatch(&creator, &json!({"delegated":true}), 1004)
        .unwrap();
    db.register_workspace("shared").unwrap();
    let sid = participant(&group, &agents[1]);
    db.create_canonical_task("legacy_task", sid, "Inspect old shared work", 1005)
        .unwrap();
    let mut input = dispatch("legacy_reader", sid, Some("legacy_task"));
    input.resources = vec![ResourceLease {
        id: "shared".into(),
        exclusive: false,
    }];
    db.enqueue_dispatch(&input).unwrap();
    let worker = db
        .claim_dispatch("legacy", 1010, 10, 120_000, 8)
        .unwrap()
        .unwrap();
    db.start_dispatch(&worker, 1011).unwrap();
    let mut completed = dispatch("completed_reader", "a", Some("parent"));
    completed.resources = input.resources.clone();
    db.enqueue_dispatch(&completed).unwrap();
    let done = claim(&mut db, 1012);
    assert_eq!(done.dispatch_id, "completed_reader");
    db.start_dispatch(&done, 1013).unwrap();
    db.complete_dispatch(&done, &json!({"observed":true}), 1014)
        .unwrap();
    db.reconcile_dispatches(1021).unwrap();
    let before = value(db.canonical_task("legacy_task").unwrap());
    drop(db);
    let inspect = sql(&root);
    // Schema 9 persisted unknown attempts after deleting their resource leases.
    // Reproduce that old state rather than pretending it has schema 10 custody.
    common::remove_graph_schema(&inspect);
    inspect
        .execute_batch("DELETE FROM resource_leases; PRAGMA user_version=9;")
        .unwrap();
    db = DomainRepository::open(&root.path().join("state")).unwrap();
    assert_eq!(
        inspect
            .pragma_query_value(None, "user_version", |r| r.get::<_, u64>(0))
            .unwrap(),
        23
    );
    assert_eq!(leases(&inspect), ["legacy_reader"]);
    assert_eq!(state(&inspect, "completed_reader"), "completed");
    assert_eq!(value(db.canonical_task("legacy_task").unwrap()), before);
    assert!(db.check_runner(&worker, 1022).is_err());
    queue_writer(&mut db);
    assert!(
        db.claim_dispatch("writer", 1022, 1000, 1000, 8)
            .unwrap()
            .is_none()
    );
    let mut replacement = input;
    replacement.id = "legacy_recovery".into();
    replacement.payload = json!({"instruction":"Continue after inspecting the legacy reader"});
    db.recover_dispatch(
        "legacy_reader",
        &replacement,
        "fixture legacy shared scope inspected",
        1023,
    )
    .unwrap();
    assert!(leases(&inspect).is_empty());
    drop(db);
    db = DomainRepository::open(&root.path().join("state")).unwrap();
    assert!(leases(&inspect).is_empty());
    assert_eq!(claim(&mut db, 1024).dispatch_id, "writer");
    assert_eq!(value(db.canonical_task("legacy_task").unwrap()), before);
}
