//! ADR-182 decision 3, the store's part: an open agent fence keeps the host
//! claim and both selectors from minting work for the engagement, and only
//! the operator's resolution of the fenced dispatch clears it. Fixture helpers
//! are copied from `tests/delegated_intents.rs` and `tests/owned_claim.rs` on
//! purpose: those files belong to other work and must not be edited.
mod common;
use common::*;
use hagency_core::{
    JSON_SAFE_MAX,
    agent_inbox::{AgentInboxPlan, AgentInboxSelection},
    ingress::{MatrixEventObservation, MatrixIngressReceipt, VerifiedNoticeClaim},
    messages::InboundMessage,
    replies::*,
    task_intents::{Delegation, IntentResult, TaskDefinition},
    tasks::*,
};
use hagency_store::{
    AgentFence, DomainRepository, EffectOutcome, Error, FenceReason, OwnedClaimProfile,
    OwnedClaimRoom,
};
use serde_json::json;
use std::collections::BTreeSet;

const ROOM: &str = "!project:example.test";

struct Agent {
    engagement: String,
    transport: MatrixTransportObservation,
}

/// Two agents of the same fleet and project, both with an available transport
/// and the shared Group room observed: the delegator owns a room-level verified
/// session, the assignee owns nothing until the test gives it sessions.
struct Fixture {
    root: tempfile::TempDir,
    db: DomainRepository,
    delegator: Agent,
    assignee: Agent,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut db = DomainRepository::open(&root.path().join("state")).unwrap();
        db.register(&registration()).unwrap();
        let pool = resource("pool", "seat", 1000);
        db.put_resource(&pool).unwrap();
        let mut agents = Vec::new();
        for (key, name, mxid, device) in [
            ("one", "Worker", "@worker:example.test", "DEVICE_WORKER"),
            ("two", "Helper", "@helper:example.test", "DEVICE_HELPER"),
        ] {
            let p = proof(&request(key, name, &pool, 100));
            let e = db.admit(&p, 1000).unwrap();
            db.approve(&format!("approve_{key}"), &p, 1000).unwrap();
            let effect = db.claim_effect().unwrap().unwrap();
            db.observe_effect(
                &effect.id,
                effect.fence,
                &EffectOutcome::Applied {
                    receipt: "fixture account".into(),
                },
            )
            .unwrap();
            let transport = MatrixTransportObservation {
                engagement_id: e.id.clone(),
                registration_generation: 1,
                generation: 1,
                sender_mxid: mxid.into(),
                device_id: device.into(),
            };
            db.observe_matrix_transport(&transport, 1001).unwrap();
            agents.push(Agent {
                engagement: e.id,
                transport,
            });
        }
        let joined = BTreeSet::from([
            "@owner:example.test".to_owned(),
            "@worker:example.test".to_owned(),
            "@helper:example.test".to_owned(),
        ]);
        for agent in &agents {
            db.observe_matrix_room(
                &MatrixRoomObservation {
                    engagement_id: agent.engagement.clone(),
                    registration_generation: 1,
                    transport_generation: 1,
                    room_id: ROOM.into(),
                    generation: 1,
                    privacy: RoomPrivacy::Group {},
                    joined: joined.clone(),
                    invite_only: true,
                    encrypted: true,
                },
                1002,
            )
            .unwrap();
        }
        db.register_workspace("work_delegator").unwrap();
        db.register_workspace("work_assignee").unwrap();
        let assignee = agents.pop().unwrap();
        let delegator = agents.pop().unwrap();
        db.resolve_verified_matrix_session(
            &SessionBinding {
                id: "delegator".into(),
                engagement_id: delegator.engagement.clone(),
                room_id: ROOM.into(),
                thread_root: None,
            },
            1003,
        )
        .unwrap();
        Self {
            root,
            db,
            delegator,
            assignee,
        }
    }
    fn sql(&self) -> rusqlite::Connection {
        rusqlite::Connection::open(self.root.path().join("state/domain.sqlite3")).unwrap()
    }
    fn count(&self, query: &str) -> u64 {
        self.sql()
            .query_row(query, [], |r| r.get::<_, u64>(0))
            .unwrap()
    }
    fn admit(
        &mut self,
        session: &str,
        thread_root: Option<&str>,
        event_id: &str,
        mention: &str,
        origin_ts: u64,
    ) -> MatrixIngressReceipt {
        let observation = MatrixEventObservation {
            scope: self.db.matrix_ingress_scope(session).unwrap(),
            event: InboundMessage {
                server_name: "example.test".into(),
                room_id: ROOM.into(),
                event_id: event_id.into(),
                sender_mxid: "@owner:example.test".into(),
                thread_root: thread_root.map(str::to_owned),
                body: format!("{mention} hand the quarterly report to a colleague"),
                kind: "m.text".into(),
                origin_ts,
            },
            mentions: BTreeSet::from([mention.to_owned()]),
            encrypted: true,
        };
        self.db
            .admit_matrix_event(&observation, origin_ts + 1)
            .unwrap()
    }
    /// The delegator's own agent-inbox dispatch, started, exactly as the live
    /// Codex agent held when it called `delegate_task`.
    fn working(&mut self) -> RunnerCapability {
        let context = self.admit("delegator", None, "$for_other", "@other:example.test", 3000);
        let wake = self.admit("delegator", None, "$wake", "@worker:example.test", 3002);
        assert!(!context.wake && wake.wake);
        let plan = AgentInboxPlan {
            session_id: "delegator".into(),
            workspace_id: "work_delegator".into(),
        };
        let AgentInboxSelection::Selected { dispatch_id, .. } =
            self.db.select_agent_inbox(&plan, 3004).unwrap()
        else {
            panic!("the delegator's own verified wake did not create a dispatch")
        };
        let cap = self
            .db
            .claim_dispatch("runner_delegator", 3005, 60_000, 120_000, 8)
            .unwrap()
            .unwrap();
        assert_eq!(cap.dispatch_id, dispatch_id);
        self.db.start_dispatch(&cap, 3006).unwrap();
        cap
    }
    fn delegate(&mut self, cap: &RunnerCapability, call: &str, at: u64) -> IntentResult {
        let input = Delegation {
            call_id: call.into(),
            assignee_engagement: self.assignee.engagement.clone(),
            root_sequence: None,
            input_sequences: vec![],
            definition: TaskDefinition {
                title: "delegated quarterly report".into(),
                description: "Draft the quarterly report and reply with it.".into(),
                ..TaskDefinition::default()
            },
        };
        self.db.delegate_task(cap, &input, at).unwrap()
    }
    /// Drive the assignee's own "Task created: …" notice to delivered, which is
    /// what activates the intent and re-projects the delegator's messages.
    fn deliver_notice(&mut self, at: u64) -> VerifiedNoticeClaim {
        let claim = self
            .db
            .claim_verified_task_notice_for(&self.assignee.engagement, at, 1000)
            .unwrap()
            .expect("the assignee claims its own task notice");
        self.db
            .begin_verified_task_notice_send(&claim.claim.notice.id, &claim.claim.token, at)
            .unwrap();
        self.db
            .deliver_verified_task_notice(
                &claim.claim.notice.id,
                &claim.claim.token,
                &notice_delivery(&claim),
                at + 1,
            )
            .unwrap();
        claim
    }
    fn profile(agent: &Agent, workspace: &str) -> OwnedClaimProfile {
        OwnedClaimProfile::new(
            agent.transport.clone(),
            vec![OwnedClaimRoom::new(ROOM.into(), 1, RoomPrivacy::Group {}).unwrap()],
            vec![workspace.into()],
        )
        .unwrap()
    }
    /// A verified thread session of the agent, with its own workspace.
    fn session(&mut self, id: &str, engagement: &str, workspace: &str, now: u64) {
        self.db
            .resolve_verified_matrix_session(
                &SessionBinding {
                    id: id.into(),
                    engagement_id: engagement.into(),
                    room_id: ROOM.into(),
                    thread_root: Some(format!("$thread_{id}")),
                },
                now,
            )
            .unwrap();
        self.db.register_workspace(workspace).unwrap();
    }
    /// Host work queued in a session: a task and an exclusive dispatch on the
    /// session's workspace, as `tests/owned_claim.rs` queues it.
    fn queue(&mut self, session: &str, workspace: &str, now: u64) {
        self.db
            .create_canonical_task(session, session, "Host claim task", now)
            .unwrap();
        self.db
            .enqueue_dispatch(&DispatchInput {
                id: session.into(),
                session_id: session.into(),
                task_id: Some(session.into()),
                resources: vec![ResourceLease {
                    id: workspace.into(),
                    exclusive: true,
                }],
                payload: json!({"instruction":"fixture"}),
            })
            .unwrap();
    }
    /// An orphan the product's own way: the attempt's lease expires after it
    /// started, so `lose` settles it `outcome_unknown`, quarantines the session
    /// and dirties the workspace — and writes no `dispatch_stops` row, the
    /// shape `recover_dispatch` resolves (the console's seeded orphan).
    fn orphan(&mut self, session: &str, engagement: &str, workspace: &str, now: u64) {
        self.session(session, engagement, workspace, now);
        self.queue(session, workspace, now + 1);
        let cap = self
            .db
            .claim_dispatch(&format!("runner_{session}"), now + 2, 1_000, 1_000, 8)
            .unwrap()
            .unwrap();
        assert_eq!(cap.dispatch_id, session);
        self.db.start_dispatch(&cap, now + 3).unwrap();
        self.db.reconcile_dispatches(now + 2_000).unwrap();
        assert_eq!(
            self.count(&format!(
                "SELECT COUNT(*) FROM runner_dispatches WHERE id='{session}' AND state='outcome_unknown'"
            )),
            1
        );
        assert_eq!(
            self.count(&format!(
                "SELECT COUNT(*) FROM dispatch_stops WHERE dispatch_id='{session}'"
            )),
            0
        );
    }
    /// The operator's recovery of an orphan: same session, same resources, a
    /// different instruction.
    fn recover(&mut self, original: &str, workspace: &str, now: u64) {
        self.db
            .recover_dispatch(
                original,
                &DispatchInput {
                    id: format!("{original}_recovered"),
                    session_id: original.into(),
                    task_id: Some(original.into()),
                    resources: vec![ResourceLease {
                        id: workspace.into(),
                        exclusive: true,
                    }],
                    payload: json!({"instruction":"Inspect the workspace and finish only the remaining work"}),
                },
                "operator inspected the workspace; no process was spawned",
                now,
            )
            .unwrap();
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

/// Spec: the claim path and the selectors honour an open fence. Queued host
/// work in a healthy session, a verified wake in another and an active
/// delegated intent in a third all belong to the fenced engagement: the host
/// claim returns nothing and both selectors return no wake, minting nothing
/// and saying nothing. An unfenced engagement is unaffected. The operator's
/// recovery of the fenced dispatch clears the fence and the same work is
/// claimed and selected.
#[test]
fn native_fenced_engagement_claims_nothing() {
    let mut f = Fixture::new();
    let worker = f.delegator.engagement.clone();
    let helper = f.assignee.engagement.clone();
    // The assignee's active delegated intent, the intent selector's input.
    let cap = f.working();
    let created = f.delegate(&cap, "call-1", 3007);
    f.deliver_notice(3008);
    let intent = AgentInboxPlan {
        session_id: created.session_id.clone(),
        workspace_id: "work_assignee".into(),
    };
    // The assignee's own verified wake, the agent inbox selector's input.
    f.session("helper_wake", &helper, "work_helper", 3100);
    let wake = f.admit(
        "helper_wake",
        Some("$thread_helper_wake"),
        "$helper_wake",
        "@helper:example.test",
        3200,
    );
    assert!(wake.wake);
    let inbox = AgentInboxPlan {
        session_id: "helper_wake".into(),
        workspace_id: "work_helper".into(),
    };
    // The fenced dispatch: the assignee's orphan in a session of its own.
    f.orphan("helper_orphan", &helper, "work_orphan", 5000);
    // Queued host work in healthy sessions of both engagements.
    f.session("helper_queue", &helper, "work_queue", 7100);
    f.queue("helper_queue", "work_queue", 7101);
    f.session("worker_queue", &worker, "work_worker", 7100);
    f.queue("worker_queue", "work_worker", 7101);
    let helper_profile = Fixture::profile(&f.assignee, "work_queue");
    let worker_profile = Fixture::profile(&f.delegator, "work_worker");

    let fence =
        f.db.write_agent_fence(
            &helper,
            "helper_orphan",
            1,
            FenceReason::CleanupUnproven,
            8000,
        )
        .unwrap();
    assert_eq!(f.db.open_agent_fence(&helper).unwrap(), Some(fence));
    assert_eq!(f.db.open_agent_fence(&worker).unwrap(), None);
    // `max_live` is wide so occupancy (the orphan counts as unresolved) is
    // not what refuses the claim: the fence is.
    let dispatches = f.count("SELECT COUNT(*) FROM runner_dispatches");
    let notices = f.count("SELECT COUNT(*) FROM task_notices");
    assert!(
        f.db.claim_owned_dispatch_for_host(&helper_profile, "helper_host", 8100, 60_000, 60_000, 8)
            .unwrap()
            .is_none(),
        "a fenced engagement claims nothing"
    );
    assert_eq!(
        f.db.select_agent_inbox(&inbox, 8200).unwrap(),
        AgentInboxSelection::NoWake
    );
    assert_eq!(
        f.db.select_intent_inbox(&intent, 8200).unwrap(),
        AgentInboxSelection::NoWake
    );
    assert_eq!(
        f.count("SELECT COUNT(*) FROM runner_dispatches"),
        dispatches,
        "nothing is minted under a fence"
    );
    assert_eq!(
        f.count("SELECT COUNT(*) FROM task_notices"),
        notices,
        "a fence is the operator's matter; the thread hears nothing"
    );
    // The unfenced engagement is unaffected.
    let claimed =
        f.db.claim_owned_dispatch_for_host(&worker_profile, "worker_host", 8100, 60_000, 60_000, 8)
            .unwrap()
            .unwrap();
    assert_eq!(claimed.dispatch_id, "worker_queue");

    // The operator's recovery of the fenced dispatch clears the fence.
    f.recover("helper_orphan", "work_orphan", 8300);
    assert_eq!(f.db.open_agent_fence(&helper).unwrap(), None);
    let cleared = f.db.agent_fences(&helper).unwrap();
    assert_eq!(cleared.len(), 1);
    assert_eq!(cleared[0].cleared_at, Some(8300));
    assert_eq!(cleared[0].cleared_by.as_deref(), Some("recover_dispatch"));
    // The same work is claimed and selected.
    let claimed =
        f.db.claim_owned_dispatch_for_host(&helper_profile, "helper_host", 8400, 60_000, 60_000, 8)
            .unwrap()
            .unwrap();
    assert_eq!(claimed.dispatch_id, "helper_queue");
    assert!(matches!(
        f.db.select_agent_inbox(&inbox, 8500).unwrap(),
        AgentInboxSelection::Selected { count: 1, .. }
    ));
    assert!(matches!(
        f.db.select_intent_inbox(&intent, 8500).unwrap(),
        AgentInboxSelection::Selected { task_id, .. } if task_id == created.task_id
    ));
}

/// Spec: the operator's resolution clears the fence. Two fenced orphans of one
/// engagement: recovering one clears its fence only, with the route's word,
/// and leaves the other open and the count of unresolved dispatches right;
/// a fence is idempotent for its open triple and `open_agent_fence` is the
/// oldest.
#[test]
fn native_resolution_clears_the_fence() {
    let mut f = Fixture::new();
    let worker = f.delegator.engagement.clone();
    let helper = f.assignee.engagement.clone();
    f.orphan("orphan_a", &helper, "work_a", 5000);
    f.orphan("orphan_b", &helper, "work_b", 7100);
    assert_eq!(
        f.db.unresolved_dispatches_for_engagement(&helper).unwrap(),
        2
    );
    assert_eq!(
        f.db.unresolved_dispatches_for_engagement(&worker).unwrap(),
        0
    );
    assert_eq!(f.db.open_agent_fence(&helper).unwrap(), None);

    let a =
        f.db.write_agent_fence(&helper, "orphan_a", 1, FenceReason::CleanupUnproven, 9000)
            .unwrap();
    assert_eq!(
        a,
        AgentFence {
            id: a.id,
            engagement_id: helper.clone(),
            dispatch_id: "orphan_a".into(),
            fence: 1,
            reason: FenceReason::CleanupUnproven,
            created_at: 9000,
            cleared_at: None,
            cleared_by: None,
        }
    );
    // Idempotent for the same open triple: the driver's repeat and a
    // re-attach see the fence that stands, not a second one.
    assert_eq!(
        f.db.write_agent_fence(&helper, "orphan_a", 1, FenceReason::CleanupUnknown, 9001)
            .unwrap(),
        a
    );
    let b =
        f.db.write_agent_fence(&helper, "orphan_b", 1, FenceReason::CleanupUnknown, 9002)
            .unwrap();
    assert!(b.id > a.id);
    assert_eq!(b.reason, FenceReason::CleanupUnknown);
    // A fence names the agent's own attempt, never another agent's, and
    // never an attempt that does not exist.
    assert!(matches!(
        f.db.write_agent_fence(&worker, "orphan_a", 1, FenceReason::CleanupUnproven, 9003),
        Err(Error::RunnerAuthority)
    ));
    assert!(matches!(
        f.db.write_agent_fence(&helper, "missing", 1, FenceReason::CleanupUnproven, 9003),
        Err(Error::NotFound)
    ));
    assert!(matches!(
        f.db.write_agent_fence(
            &helper,
            "orphan_a",
            JSON_SAFE_MAX + 1,
            FenceReason::CleanupUnproven,
            9003
        ),
        Err(Error::Invalid(_))
    ));
    assert_eq!(f.db.agent_fences(&helper).unwrap().len(), 2);
    assert_eq!(f.db.open_agent_fence(&helper).unwrap(), Some(a.clone()));
    assert_eq!(
        f.db.agent_fences(&helper).unwrap(),
        vec![b.clone(), a.clone()]
    );
    assert_eq!(f.db.agent_fences(&worker).unwrap(), vec![]);

    // Recovering the other dispatch clears its fence only.
    f.recover("orphan_b", "work_b", 9100);
    let fences = f.db.agent_fences(&helper).unwrap();
    assert_eq!(
        fences[0],
        AgentFence {
            cleared_at: Some(9100),
            cleared_by: Some("recover_dispatch".into()),
            ..b.clone()
        }
    );
    assert_eq!(fences[1], a);
    assert_eq!(f.db.open_agent_fence(&helper).unwrap(), Some(a.clone()));
    assert_eq!(
        f.db.unresolved_dispatches_for_engagement(&helper).unwrap(),
        1
    );

    f.recover("orphan_a", "work_a", 9300);
    assert_eq!(f.db.open_agent_fence(&helper).unwrap(), None);
    assert_eq!(
        f.db.unresolved_dispatches_for_engagement(&helper).unwrap(),
        0
    );
    let fences = f.db.agent_fences(&helper).unwrap();
    assert_eq!(fences.len(), 2);
    assert!(fences.iter().all(|fence| fence.cleared_at.is_some()));
    assert_eq!(fences[1].cleared_at, Some(9300));
    assert_eq!(fences[1].cleared_by.as_deref(), Some("recover_dispatch"));
    // Idempotency looks at open fences only: a cleared row is history, and
    // a new fence on the same attempt is a new row.
    let again =
        f.db.write_agent_fence(&helper, "orphan_b", 1, FenceReason::CleanupUnknown, 9400)
            .unwrap();
    assert!(again.id > b.id);
    assert_eq!(f.db.open_agent_fence(&helper).unwrap(), Some(again));
}
