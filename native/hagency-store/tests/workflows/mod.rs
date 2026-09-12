use super::*;
use hagency_core::{graphs::*, workflows::*};
use serde_json::Value;

mod custody;
mod quota;

fn request(group: &Conversation, agents: &[String]) -> WorkflowRequest {
    WorkflowRequest {
        call_id: "workflow".into(),
        conversation_id: group.id.clone(),
        definition: GraphDefinition {
            label: "Implement and independently verify".into(),
            nodes: vec![
                node("implement", participant(group, &agents[1]), &[]),
                node("verify", participant(group, &agents[0]), &["implement"]),
            ],
        },
    }
}
fn node(id: &str, session: &str, dependencies: &[&str]) -> NodeDefinition {
    NodeDefinition {
        id: id.into(),
        assignee: session.into(),
        description: format!("Perform {id}"),
        depends_on: dependencies.iter().map(|s| (*s).into()).collect(),
        condition: None,
    }
}
fn binding<'a>(workflow: &'a WorkflowView, id: &str) -> &'a WorkflowNode {
    &workflow
        .nodes
        .iter()
        .find(|n| n.node_id == id)
        .unwrap()
        .binding
}
fn phase(workflow: &WorkflowView, id: &str) -> NodeStatus {
    workflow
        .nodes
        .iter()
        .find(|n| n.node_id == id)
        .unwrap()
        .state
}
fn enqueue(db: &mut DomainRepository, w: &WorkflowView, node: &str, id: &str) {
    let n = binding(w, node);
    db.enqueue_peer_dispatch(
        &dispatch(id, &n.session_id, Some(&n.task_id)),
        &[n.message_sequence.unwrap()],
    )
    .unwrap();
}
fn start(
    db: &mut DomainRepository,
    w: &WorkflowView,
    node: &str,
    id: &str,
    now: u64,
) -> RunnerCapability {
    enqueue(db, w, node, id);
    let cap = claim(db, now);
    assert_eq!(cap.dispatch_id, id);
    db.start_dispatch(&cap, now + 1).unwrap();
    cap
}
fn transition(
    db: &mut DomainRepository,
    cap: &RunnerCapability,
    task: &str,
    status: TaskState,
    now: u64,
) {
    db.mutate_task(
        cap,
        task,
        "transition",
        &TaskMutation::Transition {
            status,
            waiting_reason: (status == TaskState::Blocked).then(|| "Verification failed".into()),
            waiting_until: (status == TaskState::Blocked).then(|| "2026-10-01T00:00:00Z".into()),
        },
        now,
    )
    .unwrap();
}
fn result(node: &str, value: Value) -> WorkflowResultRequest {
    WorkflowResultRequest {
        call_id: format!("result_{node}"),
        node_id: node.into(),
        outcome: WorkflowOutcome::Complete { result: value },
    }
}
fn make_group(
    db: &mut DomainRepository,
    agents: &[String],
    cap: &RunnerCapability,
) -> Conversation {
    db.create_internal_conversation(cap, &request_group("group", &agents[1]), 1003)
        .unwrap()
        .conversation
}

#[test]
fn native_graph_transactions() {
    let (root, mut db, agents, creator) = setup_with_parent(true);
    let group = make_group(&mut db, &agents, &creator);
    let req = request(&group, &agents);
    let inspect = sql(&root);
    for table in ["canonical_tasks", "peer_messages", "graph_commands"] {
        inspect.execute_batch(&format!("CREATE TRIGGER fail_graph BEFORE INSERT ON {table} BEGIN SELECT RAISE(ABORT,'injected graph failure'); END;")).unwrap();
        assert!(db.create_workflow(&creator, &req, 1004).is_err());
        for empty in [
            "task_graphs",
            "graph_nodes",
            "graph_commands",
            "peer_messages",
            "peer_session_inputs",
        ] {
            assert_eq!(count(&inspect, empty), 0, "{table}: {empty}");
        }
        assert_eq!(count(&inspect, "canonical_tasks"), 1);
        inspect.execute_batch("DROP TRIGGER fail_graph").unwrap();
    }
    let w = db.create_workflow(&creator, &req, 1005).unwrap().workflow;
    assert_eq!(w.parent_task_id.as_deref(), Some("parent"));
    assert_eq!(phase(&w, "implement"), NodeStatus::Dispatched);
    assert_eq!(phase(&w, "verify"), NodeStatus::Pending);
    assert!(binding(&w, "verify").message_sequence.is_none());
    for n in &w.nodes {
        let task = db.canonical_task(&n.binding.task_id).unwrap();
        assert_eq!(task.status, TaskState::Created);
        assert_eq!(task.parent_id.as_deref(), Some("parent"));
    }
    let replay = db.create_workflow(&creator, &req, 1006).unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.workflow.id, w.id);
    let mut changed = req.clone();
    changed.definition.label = "Different work".into();
    assert!(matches!(
        db.create_workflow(&creator, &changed, 1006),
        Err(Error::Conflict)
    ));
    assert_eq!(count(&inspect, "canonical_tasks"), 3);
    assert_eq!(count(&inspect, "peer_messages"), 1);
    let worker = start(&mut db, &w, "implement", "worker", 1010);
    let report = result("implement", json!({"score":0.25}));
    assert!(
        db.report_workflow_result(&worker, &w.id, &report, 1012)
            .is_err()
    );
    transition(
        &mut db,
        &worker,
        &binding(&w, "implement").task_id,
        TaskState::Done,
        1013,
    );
    inspect.execute_batch("CREATE TRIGGER fail_assignment BEFORE INSERT ON peer_messages BEGIN SELECT RAISE(ABORT,'injected assignment failure'); END;").unwrap();
    assert!(
        db.report_workflow_result(&worker, &w.id, &report, 1014)
            .is_err()
    );
    assert_eq!(
        phase(
            &db.runner_workflow(&creator, &w.id, 1015).unwrap(),
            "implement"
        ),
        NodeStatus::Active
    );
    assert_eq!(count(&inspect, "graph_dependencies"), 0);
    assert_eq!(count(&inspect, "graph_commands"), 1);
    assert_eq!(count(&inspect, "peer_messages"), 1);
    assert_eq!(
        db.canonical_task(&binding(&w, "implement").task_id)
            .unwrap()
            .status,
        TaskState::Done
    );
    inspect
        .execute_batch("DROP TRIGGER fail_assignment")
        .unwrap();
    let receipt = db
        .report_workflow_result(&worker, &w.id, &report, 1016)
        .unwrap();
    assert!(!receipt.replayed);
    assert_eq!(receipt.execution_epoch, 1);
    assert!(
        db.report_workflow_result(&worker, &w.id, &report, 1017)
            .unwrap()
            .replayed
    );
    assert!(matches!(
        db.report_workflow_result(
            &worker,
            &w.id,
            &result("implement", json!("different")),
            1017
        ),
        Err(Error::Conflict)
    ));
    assert_eq!(count(&inspect, "peer_messages"), 2);
    assert_eq!(count(&inspect, "graph_dependencies"), 1);
    db.complete_dispatch(&worker, &json!({"reported":true}), 1018)
        .unwrap();
    let w = db.runner_workflow(&creator, &w.id, 1019).unwrap();
    let verifier = start(&mut db, &w, "verify", "verifier", 1020);
    let refs = db
        .workflow_dependencies(&verifier, &w.id, 0, 32, 1022)
        .unwrap();
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].task_id, binding(&w, "implement").task_id);
    assert_eq!(
        db.workflow_dependency(&verifier, &w.id, "implement", 1022)
            .unwrap()
            .result,
        json!({"score":0.25})
    );
    transition(
        &mut db,
        &verifier,
        &binding(&w, "verify").task_id,
        TaskState::Done,
        1023,
    );
    assert_eq!(
        db.report_workflow_result(&verifier, &w.id, &result("verify", json!(null)), 1024)
            .unwrap()
            .graph_state,
        GraphStatus::Complete
    );
    db.complete_dispatch(&verifier, &json!({"reported":true}), 1025)
        .unwrap();
    assert_eq!(
        db.runner_workflow(&creator, &w.id, 1026).unwrap().state,
        GraphStatus::Complete
    );
    assert_eq!(
        db.workflow_dependency(&creator, &w.id, "verify", 1026)
            .unwrap()
            .result,
        Value::Null
    );
    assert_eq!(
        db.canonical_task("parent").unwrap().status,
        TaskState::InProgress
    );
    assert_eq!(
        inspect
            .query_row(
                "SELECT COUNT(*) FROM peer_session_inputs WHERE processed_at IS NOT NULL",
                [],
                |r| r.get::<_, u64>(0)
            )
            .unwrap(),
        2
    );
    // An existing schema 9 database retains canonical work when upgraded twice.
    let (root, db, _, cap) = setup_with_parent(true);
    drop(db);
    let inspect = sql(&root);
    remove_graph_schema(&inspect);
    inspect.pragma_update(None, "user_version", 9).unwrap();
    for _ in 0..2 {
        let db = DomainRepository::open(&root.path().join("state")).unwrap();
        assert_eq!(
            db.canonical_task("parent").unwrap().status,
            TaskState::InProgress
        );
        assert!(db.check_runner(&cap, 1010).is_err());
        assert_eq!(count(&inspect, "task_graphs"), 0);
        assert_eq!(
            inspect
                .pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
                .unwrap(),
            23
        );
    }
}

#[test]
fn native_graph_authority() {
    let (_root, mut db, agents, creator) = setup_with_parent(true);
    let group = make_group(&mut db, &agents, &creator);
    let req = request(&group, &agents);
    for assignee in ["a", "c", "Edison", &agents[1], "missing"] {
        let mut forged = req.clone();
        forged.definition.nodes[0].assignee = assignee.into();
        assert!(
            db.create_workflow(&creator, &forged, 1004).is_err(),
            "{assignee}"
        );
    }
    let w = db.create_workflow(&creator, &req, 1005).unwrap().workflow;
    let worker = start(&mut db, &w, "implement", "worker", 1010);
    assert!(db.runner_workflow(&worker, &w.id, 1012).is_err());
    assert!(
        db.runner_workflows(&worker, "", 100, 1012)
            .unwrap()
            .is_empty()
    );
    assert!(
        db.cancel_workflow(
            &worker,
            &w.id,
            &WorkflowCancel {
                call_id: "cancel".into()
            },
            1012
        )
        .is_err()
    );
    assert!(db.create_workflow(&worker, &req, 1012).is_err());
    assert!(
        db.report_workflow_result(&worker, &w.id, &result("verify", json!(true)), 1012)
            .is_err()
    );
    assert!(
        db.workflow_dependency(&worker, &w.id, "implement", 1012)
            .is_err()
    );
    db.register_session(&SessionBinding {
        id: "other_thread".into(),
        engagement_id: agents[0].clone(),
        room_id: "!project:example.test".into(),
        thread_root: Some("$other".into()),
    })
    .unwrap();
    db.enqueue_dispatch(&dispatch("other_thread", "other_thread", None))
        .unwrap();
    let other = claim(&mut db, 1013);
    db.start_dispatch(&other, 1014).unwrap();
    assert!(db.runner_workflow(&other, &w.id, 1015).is_err());
    assert!(
        db.cancel_workflow(
            &other,
            &w.id,
            &WorkflowCancel {
                call_id: "cancel".into()
            },
            1015
        )
        .is_err()
    );
    db.park_dispatch(&creator, true, 1016).unwrap();
    assert!(db.runner_workflow(&creator, &w.id, 1017).is_err());
    db.park_dispatch(&creator, false, 1018).unwrap();
    let mut forged = worker.clone();
    forged.fence += 1;
    assert!(
        db.workflow_dependencies(&forged, &w.id, 0, 32, 1019)
            .is_err()
    );
    assert!(db.create_workflow(&creator, &req, 200_000).is_err());
    assert_eq!(
        db.runner_workflows(&creator, "", 100, 1019).unwrap().len(),
        1
    );
    db.revoke("revoke_b", &agents[1]).unwrap();
    assert!(db.runner_workflow(&creator, &w.id, 1020).is_err());
    assert!(db.check_runner(&worker, 1020).is_err());
}

#[test]
fn native_graph_dispatch_scope() {
    let (root, mut db, agents, creator) = setup_with_parent(true);
    let group = make_group(&mut db, &agents, &creator);
    let w = db
        .create_workflow(&creator, &request(&group, &agents), 1004)
        .unwrap()
        .workflow;
    let first = binding(&w, "implement");
    let pending = binding(&w, "verify");
    assert!(
        db.enqueue_dispatch(&dispatch("bypass", &first.session_id, Some(&first.task_id)))
            .is_err()
    );
    for input in [
        dispatch("pending", &pending.session_id, Some(&pending.task_id)),
        dispatch("without_task", &first.session_id, None),
        dispatch("wrong_task", &first.session_id, Some("parent")),
    ] {
        assert!(
            db.enqueue_peer_dispatch(&input, &[first.message_sequence.unwrap()])
                .is_err()
        );
    }
    db.register_workspace("workspace").unwrap();
    let mut input = dispatch("worker", &first.session_id, Some(&first.task_id));
    input.resources = vec![ResourceLease {
        id: "workspace".into(),
        exclusive: true,
    }];
    db.enqueue_peer_dispatch(&input, &[first.message_sequence.unwrap()])
        .unwrap();
    let worker = claim(&mut db, 1010);
    db.start_dispatch(&worker, 1011).unwrap();
    let inbox = db.runner_peer_inbox(&worker, 0, 100, 1012).unwrap();
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0].message.data["task_id"], first.task_id);
    assert_eq!(inbox[0].message.source_session_id, "a");
    assert!(
        db.complete_dispatch(&worker, &json!({"result":"completed"}), 1012)
            .is_err()
    );
    transition(&mut db, &worker, &first.task_id, TaskState::Done, 1013);
    assert!(
        db.complete_dispatch(&worker, &json!({"result":"completed"}), 1014)
            .is_err()
    );
    let inspect = sql(&root);
    assert_eq!(count(&inspect, "resource_leases"), 1);
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
    db.report_workflow_result(
        &worker,
        &w.id,
        &result("implement", json!({"verified":true})),
        1015,
    )
    .unwrap();
    db.complete_dispatch(&worker, &json!({"reported":true}), 1016)
        .unwrap();
    assert_eq!(count(&inspect, "resource_leases"), 0);
    assert_eq!(
        db.canonical_task(&pending.task_id).unwrap().status,
        TaskState::Created
    );
}

#[test]
fn native_graph_result_recovery() {
    for reported in [false, true] {
        let (root, mut db, agents, creator) = setup_with_parent(true);
        let group = make_group(&mut db, &agents, &creator);
        let w = db
            .create_workflow(&creator, &request(&group, &agents), 1004)
            .unwrap()
            .workflow;
        let worker = start(&mut db, &w, "implement", "worker", 1010);
        let n = binding(&w, "implement");
        transition(&mut db, &worker, &n.task_id, TaskState::Done, 1012);
        let report = result("implement", json!({"score":0.25}));
        if reported {
            db.report_workflow_result(&worker, &w.id, &report, 1013)
                .unwrap();
        }
        // The finite definition survives its creator process ending.
        db.complete_dispatch(&creator, &json!({"delegated":true}), 1014)
            .unwrap();
        drop(db);
        db = DomainRepository::open(&root.path().join("state")).unwrap();
        assert!(db.check_runner(&worker, 1015).is_err());
        let mut input = dispatch("report_recovery", &n.session_id, None);
        input.payload =
            json!({"instruction":"Report the inspected canonical completion; do not rerun work"});
        db.recover_dispatch("worker", &input, "fixture inspected original work", 1016)
            .unwrap();
        let recovery = claim(&mut db, 1017);
        assert_eq!(recovery.dispatch_id, "report_recovery");
        db.start_dispatch(&recovery, 1018).unwrap();
        assert_eq!(
            db.runner_peer_inbox(&recovery, 0, 100, 1019).unwrap().len(),
            1
        );
        assert!(db.runner_workflow(&recovery, &w.id, 1019).is_err());
        assert!(
            db.create_workflow(&recovery, &request(&group, &agents), 1019)
                .is_err()
        );
        assert!(
            db.create_coordinator_task(&recovery, "forged", &n.session_id, "More work", 1019)
                .is_err()
        );
        let r = db
            .report_workflow_result(&recovery, &w.id, &report, 1020)
            .unwrap();
        assert_eq!(r.replayed, reported);
        assert_eq!(r.execution_epoch, 1);
        assert!(
            db.report_workflow_result(&worker, &w.id, &report, 1021)
                .is_err()
        );
        drop(db);
        db = DomainRepository::open(&root.path().join("state")).unwrap();
        let mut next = input.clone();
        next.id = "report_again".into();
        next.payload =
            json!({"instruction":"Confirm the stored result after inspecting the report process"});
        db.recover_dispatch("report_recovery", &next, "fixture inspected report", 1022)
            .unwrap();
        let again = claim(&mut db, 1023);
        assert_eq!(again.dispatch_id, "report_again");
        db.start_dispatch(&again, 1024).unwrap();
        assert!(
            db.report_workflow_result(&again, &w.id, &report, 1025)
                .unwrap()
                .replayed
        );
        assert!(matches!(
            db.report_workflow_result(&again, &w.id, &result("implement", json!(false)), 1025),
            Err(Error::Conflict)
        ));
        db.complete_dispatch(&again, &json!({"reported":true}), 1026)
            .unwrap();
        assert_eq!(count(&sql(&root), "peer_messages"), 2);
        assert_eq!(db.canonical_task(&n.task_id).unwrap().execution_epoch, 1);
        assert_eq!(
            db.canonical_task("parent").unwrap().status,
            TaskState::InProgress
        );
    }
    // A terminal graph still permits exact report recovery, never a new epoch.
    let (root, mut db, agents, creator) = setup_with_parent(true);
    let group = make_group(&mut db, &agents, &creator);
    let mut req = request(&group, &agents);
    req.definition.nodes.truncate(1);
    let w = db.create_workflow(&creator, &req, 1004).unwrap().workflow;
    let worker = start(&mut db, &w, "implement", "worker", 1010);
    let n = binding(&w, "implement");
    transition(&mut db, &worker, &n.task_id, TaskState::Done, 1012);
    db.report_workflow_result(&worker, &w.id, &result("implement", json!(true)), 1013)
        .unwrap();
    drop(db);
    db = DomainRepository::open(&root.path().join("state")).unwrap();
    let mut next = dispatch("final_report", &n.session_id, None);
    next.payload = json!({"instruction":"Report terminal graph after inspection"});
    db.recover_dispatch("worker", &next, "fixture terminal inspection", 1014)
        .unwrap();
    let report = claim(&mut db, 1015);
    db.start_dispatch(&report, 1016).unwrap();
    assert_eq!(
        db.runner_peer_inbox(&report, 0, 100, 1017).unwrap().len(),
        1
    );
    assert!(
        db.report_workflow_result(&report, &w.id, &result("implement", json!(true)), 1017)
            .unwrap()
            .replayed
    );
    let inspect = sql(&root);
    inspect
        .execute(
            "UPDATE canonical_tasks SET config=json_set(config,'$.execution_epoch',2) WHERE id=?1",
            [&n.task_id],
        )
        .unwrap();
    assert!(
        db.report_workflow_result(&report, &w.id, &result("implement", json!(true)), 1018)
            .is_err()
    );
    assert!(
        db.complete_dispatch(&report, &json!({"reported":true}), 1018)
            .is_err()
    );
}

fn state(inspect: &rusqlite::Connection, id: &str) -> String {
    inspect
        .query_row(
            "SELECT state FROM runner_dispatches WHERE id=?1",
            [id],
            |r| r.get(0),
        )
        .unwrap()
}
#[test]
fn native_graph_cancellation() {
    for phase in ["queued", "leased", "started", "parked"] {
        let (root, mut db, agents, creator) = setup_with_parent(true);
        let group = make_group(&mut db, &agents, &creator);
        let w = db
            .create_workflow(&creator, &request(&group, &agents), 1004)
            .unwrap()
            .workflow;
        let n = binding(&w, "implement");
        db.register_workspace("work").unwrap();
        let mut input = dispatch("worker", &n.session_id, Some(&n.task_id));
        input.resources = vec![ResourceLease {
            id: "work".into(),
            exclusive: true,
        }];
        db.enqueue_peer_dispatch(&input, &[n.message_sequence.unwrap()])
            .unwrap();
        let worker = if phase == "queued" {
            None
        } else {
            Some(claim(&mut db, 1010))
        };
        if ["started", "parked"].contains(&phase) {
            db.start_dispatch(worker.as_ref().unwrap(), 1011).unwrap();
        }
        if phase == "parked" {
            db.park_dispatch(worker.as_ref().unwrap(), true, 1012)
                .unwrap();
        }
        // A separate session of the same Agent has unrelated queued work.
        let mut other = dispatch("unrelated", "b", None);
        other.resources = input.resources.clone();
        db.enqueue_dispatch(&other).unwrap();
        let before = value(db.canonical_task(&n.task_id).unwrap());
        let cancel = WorkflowCancel {
            call_id: "cancel".into(),
        };
        let inspect = sql(&root);
        inspect.execute_batch("CREATE TRIGGER fail_cancel BEFORE INSERT ON graph_commands BEGIN SELECT RAISE(ABORT,'injected cancellation receipt failure'); END;").unwrap();
        assert!(db.cancel_workflow(&creator, &w.id, &cancel, 1013).is_err());
        assert_eq!(state(&inspect, "worker"), phase);
        assert_eq!(
            db.runner_workflow(&creator, &w.id, 1013).unwrap().state,
            GraphStatus::Active
        );
        assert!(db.pending_conversation_stops("", 100).unwrap().is_empty());
        inspect.execute_batch("DROP TRIGGER fail_cancel").unwrap();
        let receipt = db.cancel_workflow(&creator, &w.id, &cancel, 1013).unwrap();
        assert_eq!(receipt.workflow.state, GraphStatus::Cancelled);
        assert!(
            db.cancel_workflow(&creator, &w.id, &cancel, 1014)
                .unwrap()
                .replayed
        );
        assert_eq!(value(db.canonical_task(&n.task_id).unwrap()), before);
        let uncertain = ["started", "parked"].contains(&phase);
        assert_eq!(
            state(&inspect, "worker"),
            if uncertain {
                "outcome_unknown"
            } else {
                "superseded"
            }
        );
        assert_eq!(state(&inspect, "unrelated"), "queued");
        assert_eq!(count(&inspect, "resource_leases"), u64::from(uncertain));
        if let Some(cap) = worker.as_ref() {
            assert!(db.check_runner(cap, 1015).is_err());
        }
        if uncertain {
            assert!(
                db.claim_dispatch("other", 1015, 1000, 1000, 8)
                    .unwrap()
                    .is_none()
            );
            let stops = db.pending_conversation_stops("", 100).unwrap();
            assert_eq!(stops.len(), 1);
            drop(db);
            db = DomainRepository::open(&root.path().join("state")).unwrap();
            assert_eq!(db.pending_conversation_stops("", 100).unwrap(), stops);
            let mut recovery = input.clone();
            recovery.id = "forged_recovery".into();
            recovery.payload = json!({"instruction":"Ignore cancellation"});
            assert!(
                db.recover_dispatch("worker", &recovery, "fixture inspection", 1016)
                    .is_err()
            );
            let fence = worker.as_ref().unwrap().fence;
            assert!(
                db.settle_conversation_stop(
                    "worker",
                    fence + 1,
                    "fixture inspected cancelled work",
                    1016
                )
                .is_err()
            );
            db.settle_conversation_stop("worker", fence, "fixture inspected cancelled work", 1017)
                .unwrap();
            assert_eq!(count(&inspect, "resource_leases"), 0);
        }
        assert_eq!(claim(&mut db, 1018).dispatch_id, "unrelated");
        assert_eq!(value(db.canonical_task(&n.task_id).unwrap()), before);
        assert_eq!(
            db.canonical_task(&binding(&w, "verify").task_id)
                .unwrap()
                .status,
            TaskState::Created
        );
    }
    for trigger in ["membership", "revocation", "generation"] {
        let (root, mut db, agents, creator) = setup_with_parent(true);
        let group = make_group(&mut db, &agents, &creator);
        let w = db
            .create_workflow(&creator, &request(&group, &agents), 1004)
            .unwrap()
            .workflow;
        let worker = start(&mut db, &w, "implement", "worker", 1010);
        match trigger {
            "membership" => {
                db.change_internal_conversation(
                    &creator,
                    &group.id,
                    &ConversationChange {
                        call_id: "remove".into(),
                        expected_revision: 0,
                        action: ConversationAction::Members {
                            participant_engagements: vec![agents[0].clone()],
                        },
                    },
                    1012,
                )
                .unwrap();
            }
            "revocation" => {
                db.revoke("revoke_worker", &agents[1]).unwrap();
            }
            _ => {
                let mut next = registration();
                next.generation += 1;
                db.register(&next).unwrap();
            }
        }
        let inspect = sql(&root);
        assert_eq!(
            inspect
                .query_row("SELECT state FROM task_graphs WHERE id=?1", [&w.id], |r| {
                    r.get::<_, String>(0)
                })
                .unwrap(),
            "cancelled"
        );
        assert_eq!(state(&inspect, "worker"), "outcome_unknown");
        assert!(db.check_runner(&worker, 1013).is_err());
        assert!(db.runner_workflow(&creator, &w.id, 1013).is_err());
        assert_eq!(
            db.canonical_task(&binding(&w, "implement").task_id)
                .unwrap()
                .status,
            TaskState::InProgress
        );
        if trigger == "membership" {
            let next = db
                .change_internal_conversation(
                    &creator,
                    &group.id,
                    &ConversationChange {
                        call_id: "rejoin".into(),
                        expected_revision: 1,
                        action: ConversationAction::Members {
                            participant_engagements: agents[..2].to_vec(),
                        },
                    },
                    1014,
                )
                .unwrap();
            assert_ne!(
                participant(&next.conversation, &agents[1]),
                binding(&w, "implement").session_id
            );
            assert!(db.runner_workflow(&creator, &w.id, 1015).is_err());
        }
    }
}

#[test]
fn native_graph_dependency_outcomes() {
    let (root, mut db, agents, creator) = setup_with_parent(true);
    let group = make_group(&mut db, &agents, &creator);
    let mut req = request(&group, &agents);
    req.definition.nodes[1].condition = Some(Condition(
        json!({"path":"score","eq":0.25})
            .as_object()
            .unwrap()
            .clone(),
    ));
    let mut skip = node("skip", participant(&group, &agents[0]), &["implement"]);
    skip.condition = Some(Condition(
        json!({"path":"score","eq":0.5})
            .as_object()
            .unwrap()
            .clone(),
    ));
    req.definition.nodes.push(skip);
    let w = db.create_workflow(&creator, &req, 1004).unwrap().workflow;
    let worker = start(&mut db, &w, "implement", "worker", 1010);
    let large = json!({"score":0.25,"artifact":"x".repeat(40_000)});
    transition(
        &mut db,
        &worker,
        &binding(&w, "implement").task_id,
        TaskState::Done,
        1012,
    );
    db.report_workflow_result(&worker, &w.id, &result("implement", large.clone()), 1013)
        .unwrap();
    db.complete_dispatch(&worker, &json!({"reported":true}), 1014)
        .unwrap();
    let w = db.runner_workflow(&creator, &w.id, 1015).unwrap();
    assert_eq!(phase(&w, "verify"), NodeStatus::Dispatched);
    assert_eq!(phase(&w, "skip"), NodeStatus::Skipped);
    assert!(binding(&w, "skip").message_sequence.is_none());
    assert_eq!(
        db.canonical_task(&binding(&w, "skip").task_id)
            .unwrap()
            .status,
        TaskState::Created
    );
    assert!(serde_json::to_string(&w).unwrap().len() < 10_000);
    assert!(
        serde_json::to_string(&db.runner_workflows(&creator, "", 100, 1015).unwrap())
            .unwrap()
            .len()
            < 1000
    );
    let verifier = start(&mut db, &w, "verify", "verifier", 1016);
    let inbox = db.runner_peer_inbox(&verifier, 0, 100, 1018).unwrap();
    assert!(serde_json::to_string(&inbox).unwrap().len() < 4000);
    assert_eq!(
        db.workflow_dependency(&verifier, &w.id, "implement", 1018)
            .unwrap()
            .result,
        large
    );
    assert!(
        db.workflow_dependency(&verifier, &w.id, "skip", 1018)
            .is_err()
    );
    assert!(
        db.workflow_dependencies(&verifier, &w.id, 0, 33, 1018)
            .is_err()
    );
    assert!(
        db.workflow_dependencies(&verifier, &w.id, 1, 1, 1018)
            .unwrap()
            .is_empty()
    );
    let inspect = sql(&root);
    assert!(
        inspect
            .query_row("SELECT length(config) FROM task_graphs", [], |r| r
                .get::<_, u64>(0))
            .unwrap()
            < 10_000
    );

    let (_root, mut db, agents, creator) = setup_with_parent(true);
    let group = make_group(&mut db, &agents, &creator);
    let mut req = request(&group, &agents);
    req.definition
        .nodes
        .push(node("independent", participant(&group, &agents[0]), &[]));
    let w = db.create_workflow(&creator, &req, 1004).unwrap().workflow;
    let worker = start(&mut db, &w, "implement", "worker", 1010);
    let failed = WorkflowResultRequest {
        call_id: "failed".into(),
        node_id: "implement".into(),
        outcome: WorkflowOutcome::Failed {
            error: "Verification failed".into(),
        },
    };
    assert!(
        db.report_workflow_result(&worker, &w.id, &failed, 1012)
            .is_err()
    );
    transition(
        &mut db,
        &worker,
        &binding(&w, "implement").task_id,
        TaskState::Blocked,
        1013,
    );
    db.report_workflow_result(&worker, &w.id, &failed, 1014)
        .unwrap();
    // Terminal failure intentionally fences this capability; its creator reads
    // the durable verdict after a lost response instead of retrying execution.
    assert!(
        db.report_workflow_result(&worker, &w.id, &failed, 1015)
            .is_err()
    );
    let w = db.runner_workflow(&creator, &w.id, 1015).unwrap();
    assert_eq!(phase(&w, "implement"), NodeStatus::Failed);
    assert_eq!(phase(&w, "verify"), NodeStatus::Failed);
    assert!(binding(&w, "verify").message_sequence.is_none());
    assert_eq!(w.state, GraphStatus::Active);
    assert!(db.check_runner(&worker, 1015).is_err());
    let independent = start(&mut db, &w, "independent", "independent", 1016);
    transition(
        &mut db,
        &independent,
        &binding(&w, "independent").task_id,
        TaskState::Done,
        1018,
    );
    assert_eq!(
        db.report_workflow_result(
            &independent,
            &w.id,
            &result("independent", json!(true)),
            1019
        )
        .unwrap()
        .graph_state,
        GraphStatus::Failed
    );
    db.complete_dispatch(&independent, &json!({"reported":true}), 1020)
        .unwrap();
    assert_eq!(
        db.canonical_task(&binding(&w, "implement").task_id)
            .unwrap()
            .status,
        TaskState::Blocked
    );
    assert_eq!(
        db.canonical_task(&binding(&w, "verify").task_id)
            .unwrap()
            .status,
        TaskState::Created
    );
    assert_eq!(
        db.canonical_task("parent").unwrap().status,
        TaskState::InProgress
    );
}
