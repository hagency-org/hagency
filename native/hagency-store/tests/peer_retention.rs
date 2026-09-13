//! The peer-corpus retention slice (ADR-125): the pin predicate, the
//! identity store, the graph move, the floor, the receipt and the
//! migration, each pinned by its own scenario. Fixtures reuse the
//! peers.rs harness shapes (three agents, a group conversation, sends
//! through the real `send_peer` path); the corpus is always sized above
//! the floor because the sweep clamps every ceiling up to it (the
//! Slice 1 lesson, learned the hard way).
mod common;
use common::*;
use hagency_core::{conversations::*, graphs::*, peers::*, tasks::*, workflows::*};
use hagency_store::{DomainRepository, EffectOutcome, Error};
use rusqlite::{Connection, params};
use serde_json::{Value, json};

struct Fixture {
    root: tempfile::TempDir,
    db: DomainRepository,
    agents: Vec<String>,
    cap: RunnerCapability,
    group: Conversation,
}

fn dispatch(id: &str, session: &str, task: Option<&str>) -> DispatchInput {
    DispatchInput {
        id: id.into(),
        session_id: session.into(),
        task_id: task.map(str::to_owned),
        resources: vec![],
        payload: json!({"instruction":"Verify the peer corpus bound"}),
    }
}

fn claim(db: &mut DomainRepository, now: u64) -> RunnerCapability {
    db.claim_dispatch("runner", now, 60_000, 120_000, 8)
        .unwrap()
        .unwrap()
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

fn send(group: &Conversation, key: &str, targets: &[&str], kind: PeerKind) -> PeerSend {
    PeerSend {
        call_id: key.into(),
        conversation_id: group.id.clone(),
        recipient_session_ids: targets.iter().map(|v| (*v).into()).collect(),
        kind,
        priority: PeerPriority::Normal,
        summary: "请核对结果".into(),
        body: format!("Peer corpus {key}"),
        data: json!({"score":0.5}),
    }
}

impl Fixture {
    /// The peers.rs three-agent shape: a, b in one project, c in another;
    /// the creator dispatch on session "a" is started and owns every send.
    fn new(tag: &str) -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut db = DomainRepository::open(&root.path().join("state")).unwrap();
        db.register(&registration()).unwrap();
        let pool = resource("pool", "seat", 1000);
        db.put_resource(&pool).unwrap();
        let mut agents = Vec::new();
        for (id, name) in [("a", "小白"), ("b", "Edison"), ("c", "Other")] {
            let req = request(id, name, &pool, 100);
            let p = proof(&req);
            let e = db.admit(&p, 1000).unwrap();
            db.approve(&format!("approve_{id}_{tag}"), &p, 1000)
                .unwrap();
            let effect = db.claim_effect().unwrap().unwrap();
            db.observe_effect(
                &effect.id,
                effect.fence,
                &EffectOutcome::Applied {
                    receipt: format!("fixture_{id}_{tag}"),
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
        let group = db
            .create_internal_conversation(&cap, &request_group("group", &agents[1]), 1003)
            .unwrap()
            .conversation;
        Self {
            root,
            db,
            agents,
            cap,
            group,
        }
    }
    fn b(&self) -> &str {
        participant(&self.group, &self.agents[1])
    }
    fn send_one(&mut self, key: &str, at: u64) -> u64 {
        let request = send(&self.group, key, &[self.b()], PeerKind::Request);
        self.db.send_peer(&self.cap, &request, at).unwrap().sequence
    }
    /// The window fill: unread Request sends to the second agent. The
    /// corpus must sit above the floor (100) for the ceiling to bite.
    fn fill(&mut self, n: usize, base: u64) {
        for i in 0..n {
            self.send_one(&format!("fill{i}"), base + i as u64);
        }
    }
    fn sql(&self) -> Connection {
        Connection::open(self.root.path().join("state/domain.sqlite3")).unwrap()
    }
    fn count(&self, table: &str) -> u64 {
        self.sql()
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    }
}

/// Mark every session-input row of one message processed (the P2/P3'
/// release witness), bound parameters only.
fn processed(f: &Fixture, sequence: u64, at: u64) {
    f.sql()
        .execute(
            "UPDATE peer_session_inputs SET processed_at=?2 WHERE message_sequence=?1",
            params![sequence, at],
        )
        .unwrap();
}

/// The children of one message: (session_inputs, dispatch_inputs).
fn children(f: &Fixture, sequence: u64) -> (u64, u64) {
    let sql = f.sql();
    let si: u64 = sql
        .query_row(
            "SELECT COUNT(*) FROM peer_session_inputs WHERE message_sequence=?1",
            [sequence],
            |r| r.get(0),
        )
        .unwrap();
    let pi: u64 = sql
        .query_row(
            "SELECT COUNT(*) FROM peer_dispatch_inputs WHERE message_sequence=?1",
            [sequence],
            |r| r.get(0),
        )
        .unwrap();
    (si, pi)
}

#[test]
fn native_retained_peer_corpus_prunes_below_ceiling_only_when_no_live_reference() {
    let mut f = Fixture::new("prune");
    let old1 = f.send_one("plain1", 2000);
    let old2 = f.send_one("plain2", 2001);
    let old3 = f.send_one("plain3", 2002);
    f.fill(100, 2003);
    processed(&f, old1, 3000);
    processed(&f, old2, 3000);
    processed(&f, old3, 3000);
    let outcome = f.db.sweep_peer_corpus(4000, 100, 512).unwrap();
    assert_eq!(outcome.pruned, 3);
    assert_eq!(outcome.remaining, 0, "103 rows, 3 pruned, 100 kept");
    assert_eq!(f.count("peer_messages"), 100);
    // The negative: a parent-only delete must fail — both child
    // projections are gone for every pruned sequence.
    for sequence in [old1, old2, old3] {
        assert_eq!(children(&f, sequence), (0, 0));
    }
    assert_eq!(f.count("retained_peer_index"), 3);
    // The pinned survivors keep their unread children.
    let unread: u64 = f
        .sql()
        .query_row(
            "SELECT COUNT(*) FROM peer_session_inputs WHERE processed_at IS NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(unread, 100);
}

#[test]
fn native_retained_peer_corpus_processed_dispatch_does_not_pin() {
    let mut f = Fixture::new("dispatch");
    let done = f.send_one("claimed", 2000);
    f.fill(100, 2001);
    let b = f.b().to_owned();
    f.db.enqueue_peer_dispatch(&dispatch("consume_done", &b, None), &[done])
        .unwrap();
    let cap = claim(&mut f.db, 3000);
    f.db.start_dispatch(&cap, 3001).unwrap();
    f.db.complete_dispatch(&cap, &json!({"ok":true}), 3002)
        .unwrap();
    let outcome = f.db.sweep_peer_corpus(4000, 100, 512).unwrap();
    assert_eq!(outcome.pruned, 1);
    assert_eq!(
        children(&f, done),
        (0, 0),
        "the consumed row is gone with its children"
    );
    // The mirror: the same shape with processed_at still NULL is retained
    // (P3' — claimed but not yet processed).
    let mut f2 = Fixture::new("dispatch_mirror");
    let held = f2.send_one("claimed2", 2000);
    f2.fill(100, 2001);
    let b2 = f2.b().to_owned();
    f2.db
        .enqueue_peer_dispatch(&dispatch("consume_held", &b2, None), &[held])
        .unwrap();
    let cap2 = claim(&mut f2.db, 3000);
    f2.db.start_dispatch(&cap2, 3001).unwrap();
    let outcome2 = f2.db.sweep_peer_corpus(4000, 100, 512).unwrap();
    assert_eq!(outcome2.pruned, 0, "claimed and unprocessed pins (P3')");
    assert_eq!(children(&f2, held), (1, 1));
}

#[test]
fn native_retained_peer_corpus_unknown_fate_is_retained() {
    let mut f = Fixture::new("unknown");
    let unknown = f.send_one("unknown_fate", 2000);
    f.fill(100, 2001);
    let b = f.b().to_owned();
    f.db.enqueue_peer_dispatch(&dispatch("consume_unknown", &b, None), &[unknown])
        .unwrap();
    let cap = claim(&mut f.db, 3000);
    f.db.start_dispatch(&cap, 3001).unwrap();
    processed(&f, unknown, 3002);
    // The losing path: the dispatch outcome is unknown, a settled stop and
    // a recovery row are both present — D-1's class, and the raw state is
    // what pins (the unresolved view must not release it).
    {
        let sql = f.sql();
        sql.execute(
            "UPDATE runner_dispatches SET state='outcome_unknown' WHERE id='consume_unknown'",
            [],
        )
        .unwrap();
        sql.execute(
            "INSERT INTO runner_dispatches(id,session_id,input,digest,state,fence,not_before) VALUES('replacement_unknown',?1,'{}','digest','queued',0,0)",
            params![b],
        )
        .unwrap();
        sql.execute(
            "INSERT INTO dispatch_recoveries(original_id,replacement_id,evidence,created_at) VALUES('consume_unknown','replacement_unknown','{}',3003)",
            [],
        )
        .unwrap();
        sql.execute(
            "INSERT INTO dispatch_stops(dispatch_id,fence,reason,created_at,evidence,settled_at) VALUES('consume_unknown',1,'reconciled',3003,'{}',3004)",
            [],
        )
        .unwrap();
    }
    let outcome = f.db.sweep_peer_corpus(4000, 100, 512).unwrap();
    assert_eq!(
        outcome.pruned, 0,
        "unknown fate is retained indefinitely (D-1)"
    );
    assert_eq!(outcome.remaining, 1);
    assert_eq!(children(&f, unknown), (1, 1));
    assert_eq!(f.count("retained_peer_index"), 0);
}

#[test]
fn native_retained_peer_corpus_closed_conversation_releases_its_unread() {
    let mut f = Fixture::new("closed");
    let unread = f.send_one("unread_closed", 2000);
    f.fill(100, 2001);
    f.sql()
        .execute(
            "UPDATE internal_conversations SET state='closed' WHERE id=?1",
            params![f.group.id],
        )
        .unwrap();
    let outcome = f.db.sweep_peer_corpus(4000, 100, 512).unwrap();
    assert_eq!(
        outcome.pruned, 1,
        "a closed conversation releases its unread (P2')"
    );
    assert_eq!(children(&f, unread), (0, 0));
    // The mirror: with the conversation still active the same unread pair
    // is pinned.
    let mut f2 = Fixture::new("closed_mirror");
    let unread2 = f2.send_one("unread_active", 2000);
    f2.fill(100, 2001);
    let outcome2 = f2.db.sweep_peer_corpus(4000, 100, 512).unwrap();
    assert_eq!(outcome2.pruned, 0, "an active conversation pins its unread");
    assert_eq!(children(&f2, unread2), (1, 0));
}

#[test]
fn native_retained_peer_corpus_inactive_engagement_releases_its_unread() {
    let mut f = Fixture::new("revoked");
    let unread = f.send_one("unread_revoked", 2000);
    f.fill(100, 2001);
    f.db.revoke("revoke_peer_b", &f.agents[1]).unwrap();
    let outcome = f.db.sweep_peer_corpus(4000, 100, 512).unwrap();
    assert_eq!(
        outcome.pruned, 1,
        "an inactive engagement releases its unread (P2')"
    );
    assert_eq!(children(&f, unread), (0, 0));
    // The mirror: with the engagement active the same pair is pinned.
    let mut f2 = Fixture::new("revoked_mirror");
    let unread2 = f2.send_one("unread_engaged", 2000);
    f2.fill(100, 2001);
    let outcome2 = f2.db.sweep_peer_corpus(4000, 100, 512).unwrap();
    assert_eq!(outcome2.pruned, 0, "an active engagement pins its unread");
    assert_eq!(children(&f2, unread2), (1, 0));
}

#[test]
fn native_retained_peer_corpus_pending_pin_exceeds_ceiling() {
    let mut f = Fixture::new("pending");
    f.fill(103, 2000);
    // Every row is pinned by an unread, visible session input (P2').
    let outcome = f.db.sweep_peer_corpus(4000, 100, 512).unwrap();
    assert_eq!(outcome.pruned, 0);
    assert_eq!(outcome.remaining, 3, "the over-ceiling figure is reported");
    assert_eq!(f.count("peer_messages"), 103);
    assert_eq!(f.count("retained_peer_index"), 0);
    // The bound never refuses work: one more send still lands.
    let extra = f.send_one("extra", 5000);
    assert!(extra > 0);
}

fn node_definition(id: &str, session: &str, dependencies: &[&str]) -> NodeDefinition {
    NodeDefinition {
        id: id.into(),
        assignee: session.into(),
        description: format!("Perform {id}"),
        depends_on: dependencies.iter().map(|s| (*s).into()).collect(),
        condition: None,
    }
}

fn workflow_request(group: &Conversation, agents: &[String]) -> WorkflowRequest {
    WorkflowRequest {
        call_id: "workflow".into(),
        conversation_id: group.id.clone(),
        definition: GraphDefinition {
            label: "Implement and independently verify".into(),
            nodes: vec![
                node_definition("implement", participant(group, &agents[1]), &[]),
                node_definition("verify", participant(group, &agents[0]), &["implement"]),
            ],
        },
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

/// Start the given node's consuming dispatch and return its capability.
fn start_node(
    db: &mut DomainRepository,
    workflow: &WorkflowView,
    node: &str,
    id: &str,
    now: u64,
) -> RunnerCapability {
    let n = binding(workflow, node);
    let session = n.session_id.clone();
    let task = n.task_id.clone();
    let sequence = n.message_sequence.unwrap();
    db.enqueue_peer_dispatch(&dispatch(id, &session, Some(&task)), &[sequence])
        .unwrap();
    let cap = claim(db, now);
    assert_eq!(cap.dispatch_id, id);
    db.start_dispatch(&cap, now + 1).unwrap();
    cap
}

fn node_result(node: &str, value: Value) -> WorkflowResultRequest {
    WorkflowResultRequest {
        call_id: format!("result_{node}"),
        node_id: node.into(),
        outcome: WorkflowOutcome::Complete { result: value },
    }
}

/// Flip a graph node's task to its terminal `Done` state — the precondition
/// `report_workflow_result`'s `Complete` arm enforces before it will observe
/// the completion (graphs.rs refuses while `task.status != Done`).
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
            waiting_reason: None,
            waiting_until: None,
        },
        now,
    )
    .unwrap();
}

#[test]
fn native_retained_peer_corpus_graph_binding_moves_with_the_message() {
    let mut f = Fixture::new("move");
    let request = workflow_request(&f.group, &f.agents);
    let workflow =
        f.db.create_workflow(&f.cap, &request, 2000)
            .unwrap()
            .workflow;
    let bound = binding(&workflow, "implement").message_sequence.unwrap();
    f.fill(100, 2001);
    // Terminal node: the report completes it and consumes the input.
    let worker = start_node(&mut f.db, &workflow, "implement", "worker", 3000);
    transition(
        &mut f.db,
        &worker,
        binding(&workflow, "implement").task_id.as_str(),
        TaskState::Done,
        3001,
    );
    f.db.report_workflow_result(
        &worker,
        &workflow.id,
        &node_result("implement", json!({"done":true})),
        3002,
    )
    .unwrap();
    // complete_dispatch last: report authorizes a "started" dispatch and
    // complete_dispatch's guard requires the node to already be complete.
    f.db.complete_dispatch(&worker, &json!({"done":true}), 3003)
        .unwrap();
    let outcome = f.db.sweep_peer_corpus(4000, 100, 512).unwrap();
    assert_eq!(
        outcome.pruned, 1,
        "the terminal node's message is a candidate"
    );
    assert_eq!(outcome.moved, 1, "the binding moved with the message");
    // The paired move: the column AND the persisted config are both clear,
    // so a workflow read after the prune returns Ok — a column-only move
    // would poison it with Error::State.
    let after = f.db.runner_workflow(&f.cap, &workflow.id, 4001).unwrap();
    assert!(binding(&after, "implement").message_sequence.is_none());
    assert_eq!(f.count("retained_peer_index"), 1);
    let identity: u64 = f
        .sql()
        .query_row("SELECT sequence FROM retained_peer_index", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        identity, bound,
        "the identity row records the pruned sequence"
    );
}

#[test]
fn native_retained_peer_corpus_live_graph_binding_is_retained() {
    let mut f = Fixture::new("livegraph");
    let request = workflow_request(&f.group, &f.agents);
    let workflow =
        f.db.create_workflow(&f.cap, &request, 2000)
            .unwrap()
            .workflow;
    let bound = binding(&workflow, "implement").message_sequence.unwrap();
    f.fill(100, 2001);
    // A live (dispatched/active) node: the input stays unread, the dispatch
    // stays started — P4/P5 pin it, P6 does not release it.
    let worker = start_node(&mut f.db, &workflow, "implement", "worker", 3000);
    let outcome = f.db.sweep_peer_corpus(4000, 100, 512).unwrap();
    assert_eq!(outcome.pruned, 0, "a live node's binding pins (P6)");
    assert_eq!(outcome.moved, 0);
    assert_eq!(children(&f, bound), (1, 1));
    // The mirror: with the node flipped terminal the same setup is pruned —
    // the task reaches Done, the consuming dispatch completes (releasing the
    // P4/P5 dispatch pin and marking the input processed), then the report
    // observes the completion.
    transition(
        &mut f.db,
        &worker,
        binding(&workflow, "implement").task_id.as_str(),
        TaskState::Done,
        3003,
    );
    f.db.report_workflow_result(
        &worker,
        &workflow.id,
        &node_result("implement", json!({"done":true})),
        3004,
    )
    .unwrap_or_else(|e| panic!("the creator may report its own node: {e}"));
    // complete_dispatch last: report authorizes a "started" dispatch and
    // complete_dispatch's guard requires the node to already be complete.
    f.db.complete_dispatch(&worker, &json!({"done":true}), 3005)
        .unwrap();
    let outcome2 = f.db.sweep_peer_corpus(4000, 100, 512).unwrap();
    assert_eq!(outcome2.pruned, 1);
    assert_eq!(outcome2.moved, 1);
}

#[test]
fn native_retained_peer_corpus_shared_views_no_consumer_observes_the_prune() {
    let mut f = Fixture::new("views");
    let pruned = f.send_one("gone", 2000);
    f.fill(100, 2001);
    processed(&f, pruned, 3000);
    let b = f.b().to_owned();
    // The non-emptiness precondition: b's unread inbox is live consumer
    // input before the sweep.
    let before = f.db.peer_inbox(&b, 0, 100).unwrap();
    assert!(
        !before.is_empty(),
        "the consumer set is non-empty pre-sweep"
    );
    let capacity_before: u64 = f
        .sql()
        .query_row(
            "SELECT COUNT(*) FROM peer_session_inputs i WHERE i.session_id=?1 AND i.processed_at IS NULL AND EXISTS(SELECT 1 FROM live_peer_inputs live WHERE live.session_id=i.session_id AND live.message_sequence=i.message_sequence)",
            params![b],
            |r| r.get(0),
        )
        .unwrap();
    assert!(capacity_before > 0);
    let outcome = f.db.sweep_peer_corpus(4000, 100, 512).unwrap();
    assert_eq!(outcome.pruned, 1);
    // The consumer queries answer without error and identically: the page
    // read and the capacity-reserve probe never see the pruned pair.
    // `PeerInboxItem` is not `PartialEq`, so the page is compared by its
    // observable content: length, sequence order and wake flags.
    let after = f.db.peer_inbox(&b, 0, 100).unwrap();
    assert_eq!(after.len(), before.len());
    let shapes = |items: &[hagency_core::peers::PeerInboxItem]| {
        items
            .iter()
            .map(|i| (i.message.sequence, i.wake))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        shapes(&after),
        shapes(&before),
        "the inbox page is unchanged by the prune"
    );
    let capacity_after: u64 = f
        .sql()
        .query_row(
            "SELECT COUNT(*) FROM peer_session_inputs i WHERE i.session_id=?1 AND i.processed_at IS NULL AND EXISTS(SELECT 1 FROM live_peer_inputs live WHERE live.session_id=i.session_id AND live.message_sequence=i.message_sequence)",
            params![b],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(capacity_before, capacity_after);
}

#[test]
fn native_retained_peer_corpus_queued_dispatch_input_is_not_dropped() {
    let mut f = Fixture::new("queued");
    let queued = f.send_one("queued_input", 2000);
    f.fill(100, 2001);
    let b = f.b().to_owned();
    f.db.enqueue_peer_dispatch(&dispatch("consume_queued", &b, None), &[queued])
        .unwrap();
    processed(&f, queued, 3000);
    let outcome = f.db.sweep_peer_corpus(4000, 100, 512).unwrap();
    assert_eq!(outcome.pruned, 0, "a queued dispatch pins (P4)");
    assert_eq!(children(&f, queued), (1, 1));
    // The claim-flip negative's mirror: with the dispatch flipped
    // completed the row is pruned — and the claim was never allowed while
    // the input rode a queued dispatch it could not see.
    f.sql()
        .execute(
            "UPDATE runner_dispatches SET state='completed' WHERE id='consume_queued'",
            [],
        )
        .unwrap();
    let outcome2 = f.db.sweep_peer_corpus(4001, 100, 512).unwrap();
    assert_eq!(outcome2.pruned, 1);
    assert_eq!(children(&f, queued), (0, 0));
}

#[test]
fn native_retained_peer_corpus_replay_answers_from_identity_store() {
    let mut f = Fixture::new("replay");
    let request = send(&f.group, "replay_me", &[f.b()], PeerKind::Request);
    let sequence = f.db.send_peer(&f.cap, &request, 2000).unwrap().sequence;
    f.fill(100, 2001);
    processed(&f, sequence, 3000);
    let outcome = f.db.sweep_peer_corpus(4000, 100, 512).unwrap();
    assert_eq!(outcome.pruned, 1);
    let b = f.b().to_owned();
    let inbox_before = f.db.peer_inbox(&b, 0, 100).unwrap().len();
    // An exact re-presentation answers replayed with the recorded
    // sequence — identity, not content.
    let replay = f.db.send_peer(&f.cap, &request, 5000).unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.sequence, sequence);
    // A divergent re-presentation under the same key is Conflict.
    let mut divergent = request.clone();
    divergent.data = json!({"score":0.9});
    assert!(matches!(
        f.db.send_peer(&f.cap, &divergent, 5001),
        Err(Error::Conflict)
    ));
    // The recipient's inbox is unchanged across the replay attempts.
    assert_eq!(f.db.peer_inbox(&b, 0, 100).unwrap().len(), inbox_before);
    assert_eq!(f.count("peer_messages"), 100, "no row was resurrected");
}

#[test]
fn native_retained_peer_corpus_identity_store_is_bounded() {
    let mut f = Fixture::new("bounded");
    // One processed message is the sole past-window candidate; the other
    // 100 stay unread (pinned) so the sweep prunes exactly one row and
    // writes its identity before applying the identity store's own bound.
    let pruned = f.send_one("gone", 2000);
    f.fill(100, 2001);
    processed(&f, pruned, 3000);
    // The identity store's own bound is PEER_RECEIPT_CEILING (10_000), not
    // the corpus ceiling; seed past it directly — the prune statement is
    // the DDL's own, oldest-first by (pruned_at_ms, sequence).
    {
        let sql = f.sql();
        for i in 0..10_005u64 {
            sql.execute(
                "INSERT INTO retained_peer_index(source_key,digest,sequence,pruned_at_ms) VALUES(?1,?2,?3,?4)",
                params![format!("seed_{i}"), "digest", 9_000_000 + i, i],
            )
            .unwrap();
        }
    }
    // The bound runs inside the sweep's own transaction — but it only
    // evicts when the sweep did work (pruned > 0), and the over-seed is
    // evicted by the OLDEST-FIRST statement, so the newest seed survives.
    let outcome = f.db.sweep_peer_corpus(4000, 100, 512).unwrap();
    assert_eq!(outcome.pruned, 1);
    let rows: u64 = f.count("retained_peer_index");
    assert_eq!(
        rows, 10_000,
        "bounded at PEER_RECEIPT_CEILING, oldest-first"
    );
    let oldest: u64 = f
        .sql()
        .query_row(
            "SELECT COUNT(*) FROM retained_peer_index WHERE source_key LIKE 'seed_%' AND sequence < 5",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(oldest, 0, "the five oldest seeds are gone");
    let newest: u64 = f
        .sql()
        .query_row(
            "SELECT COUNT(*) FROM retained_peer_index WHERE source_key='seed_10004'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(newest, 1, "the newest seed stays");
}

#[test]
fn native_retained_peer_corpus_receipt_row_is_written_inside_the_transaction() {
    let mut f = Fixture::new("receipt");
    let old = f.send_one("receipt_row", 2000);
    f.fill(100, 2001);
    processed(&f, old, 3000);
    // Seed the receipt table past its trim bound so the same writer's trim
    // is exercised, then run one pruning tick.
    {
        let sql = f.sql();
        for i in 0..105u64 {
            sql.execute(
                "INSERT INTO retention_prune_receipts(phase,pruned,oldest_ref,newest_ref,remaining,elapsed_ms,at_ms) VALUES('peer',1,?1,?1,0,1,?2)",
                params![i.to_string(), 1000 + i],
            )
            .unwrap();
        }
    }
    // R5: the receipt is INVISIBLE before the phase's commit — proved by
    // making the receipt INSERT abort inside the phase's own transaction.
    // A BEFORE-INSERT trigger on this tick's receipt row raises ABORT, so
    // the sweep's `Immediate` transaction rolls back wholesale. If the
    // receipt were written outside the transaction (post-commit), the
    // abort would leave the prune committed with no receipt; because it is
    // inside, the prune rolls back with it — nothing is observable.
    {
        let sql = f.sql();
        sql.execute(
            "CREATE TRIGGER observe_receipt_precommit BEFORE INSERT ON retention_prune_receipts \
             WHEN NEW.phase='peer' AND NEW.at_ms>=4000 \
             BEGIN SELECT RAISE(ABORT,'pre-commit observation'); END",
            [],
        )
        .unwrap();
    }
    let messages_before: u64 = f.count("peer_messages");
    let aborted = f.db.sweep_peer_corpus(4000, 100, 512);
    assert!(
        aborted.is_err(),
        "the receipt INSERT aborts the sweep transaction"
    );
    // Both the prune AND its receipt rolled back — neither is observable
    // before the phase commits.
    assert_eq!(
        f.count("peer_messages"),
        messages_before,
        "the prune rolled back with the aborted receipt"
    );
    let precommit_rows: u64 = f
        .sql()
        .query_row(
            "SELECT COUNT(*) FROM retention_prune_receipts WHERE at_ms>=4000",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        precommit_rows, 0,
        "no receipt row is observable before the phase's commit"
    );
    f.sql()
        .execute("DROP TRIGGER observe_receipt_precommit", [])
        .unwrap();
    let outcome = f.db.sweep_peer_corpus(4000, 100, 512).unwrap();
    assert_eq!(outcome.pruned, 1);
    // The corrected scenario 8: the freshly-written row carries the shared
    // table's SEVEN real columns, and there is no `archived` column for
    // this lane (the receipt is written inside the phase transaction).
    let row: (String, u64, String, String, u64, u64, u64) = f
        .sql()
        .query_row(
            "SELECT phase,pruned,oldest_ref,newest_ref,remaining,elapsed_ms,at_ms \
             FROM retention_prune_receipts WHERE at_ms>=4000",
            [],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(row.0, "peer", "the receipt row names its phase");
    assert_eq!(row.1, 1, "pruned");
    assert_eq!(row.4, 0, "remaining");
    let archived_col: u64 = f
        .sql()
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('retention_prune_receipts') WHERE name='archived'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(archived_col, 0, "there is no `archived` column");
    let rows: u64 = f
        .sql()
        .query_row("SELECT COUNT(*) FROM retention_prune_receipts", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert!(rows <= 100, "trimmed to the shared RETENTION_RECEIPT_LIMIT");
    assert_eq!(rows, 100);
}

#[test]
fn native_retained_peer_corpus_floor_is_hundred() {
    let mut f = Fixture::new("floor");
    f.fill(120, 2000);
    for sequence in 1..=120u64 {
        processed(&f, sequence, 3000);
    }
    // A ceiling below the floor is clamped up to 100.
    let outcome = f.db.sweep_peer_corpus(4000, 5, 512).unwrap();
    assert_eq!(outcome.pruned, 20, "the sweep clamps the ceiling too");
    assert_eq!(f.count("peer_messages"), 100);
    // A ceiling above the floor is NOT clamped: 1000 keeps 150 rows.
    let mut f2 = Fixture::new("floor_high");
    f2.fill(150, 2000);
    for sequence in 1..=150u64 {
        processed(&f2, sequence, 3000);
    }
    let outcome2 = f2.db.sweep_peer_corpus(4000, 1000, 512).unwrap();
    assert_eq!(
        outcome2.pruned, 0,
        "a corpus under the ceiling keeps everything"
    );
    assert_eq!(f2.count("peer_messages"), 150);
}

#[test]
fn native_retained_peer_corpus_migration_replays_after_rewind() {
    let mut f = Fixture::new("upgrade");
    let prunable = f.send_one("upgrade1", 2000);
    let pinned = f.send_one("upgrade2", 2001);
    let _ = pinned; // P2 holds this row: its session input stays unread.
    f.fill(100, 2002);
    processed(&f, prunable, 3000);
    let state = f.root.path().join("state");
    drop(f.db);
    {
        let sql = Connection::open(state.join("domain.sqlite3")).unwrap();
        sql.pragma_update(None, "user_version", 26).unwrap();
    }
    // The double open: the second run is at head 29 and replays nothing.
    for _ in 0..2 {
        let db = DomainRepository::open(&state).unwrap();
        drop(db);
        let sql = Connection::open(state.join("domain.sqlite3")).unwrap();
        assert_eq!(
            sql.pragma_query_value(None, "user_version", |r| r.get::<_, u64>(0))
                .unwrap(),
            30
        );
        let index: u64 = sql
            .query_row("SELECT COUNT(*) FROM retained_peer_index", [], |r| r.get(0))
            .unwrap();
        assert_eq!(index, 0, "the migration creates and drains nothing");
        let indexes: u64 = sql
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name IN (?1,?2,?3)",
                params![
                    "peer_session_inputs_message",
                    "peer_dispatch_inputs_message",
                    "peer_index_pruned_at"
                ],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(indexes, 3, "the peer pin-probe indexes exist");
    }
    // Drain through the sweep entry point, one row per tick (the batch).
    // 102 rows live (1 prunable + 1 pinned + 100 fill); after the batch
    // prunes the one past-window row, 101 remain — one over the ceiling.
    let mut db = DomainRepository::open(&state).unwrap();
    let first = db.sweep_peer_corpus(5000, 100, 1).unwrap();
    assert_eq!(first.pruned, 1, "the batch bound stops at one row");
    assert_eq!(first.remaining, 1, "102 live rows, 1 pruned, 101 kept");
    // The identity row is keyed by the pruned row's SEQUENCE — send_peer's
    // source_key is a digest ("peer_<hex>"), never the call_id text, so a
    // LIKE over the key can never match. Assert by the sequence the
    // identity store is defined to record.
    let identity: Option<u64> = Connection::open(state.join("domain.sqlite3"))
        .unwrap()
        .query_row(
            "SELECT sequence FROM retained_peer_index WHERE sequence=?1",
            [prunable],
            |r| r.get(0),
        )
        .ok();
    assert_eq!(
        identity,
        Some(prunable),
        "the drained row's identity is kept"
    );
}

#[test]
fn native_retained_peer_corpus_migration_head_is_current() {
    let mut f = Fixture::new("head");
    let _ = f.send_one("head_row", 2000);
    let state = f.root.path().join("state");
    drop(f.db);
    // A fresh database opens at head 29 — and a deeper rewind (to 24, the
    // ceiling-alerts fixture's shape) replays 025, 026 AND 027 over a live
    // database, proving the whole retention block replays in order.
    {
        let sql = Connection::open(state.join("domain.sqlite3")).unwrap();
        assert_eq!(
            sql.pragma_query_value(None, "user_version", |r| r.get::<_, u64>(0))
                .unwrap(),
            30
        );
        // 025 is NOT idempotent (ALTER TABLE ... ADD COLUMN status): a
        // deeper rewind to 24 replays it over a table that already carries
        // the columns and fails on the duplicate. Rebuild the 024 shape
        // first (the ceiling_alerts.rs:749 pattern) so 025 adds the columns
        // to a bare 024 table and 026/027 replay as no-ops.
        sql.execute_batch("DROP TABLE ceiling_alerts;").unwrap();
        sql.execute_batch(include_str!("../src/migrations/024-ceiling-alerts.sql"))
            .unwrap();
        sql.pragma_update(None, "user_version", 24).unwrap();
    }
    for _ in 0..2 {
        let db = DomainRepository::open(&state).unwrap();
        drop(db);
        let sql = Connection::open(state.join("domain.sqlite3")).unwrap();
        assert_eq!(
            sql.pragma_query_value(None, "user_version", |r| r.get::<_, u64>(0))
                .unwrap(),
            30,
            "every reopen lands at the current head"
        );
    }
}
