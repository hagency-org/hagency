#[path = "approvals/card.rs"]
mod card;
#[path = "approvals/clock.rs"]
mod clock;
mod common;
#[path = "approvals/owned.rs"]
mod owned;
#[path = "approvals/responses.rs"]
mod responses;
use common::*;
use hagency_core::{approvals::*, replies::*, tasks::*};
use hagency_store::{DomainRepository, EffectOutcome, Error};
use serde_json::json;
use std::collections::BTreeSet;

struct Fixture {
    root: tempfile::TempDir,
    db: DomainRepository,
    agents: Vec<String>,
    caps: Vec<RunnerCapability>,
    rooms: Vec<ApprovalRoomObservation>,
    contexts: Vec<HostApprovalContext>,
    fingerprints: Vec<String>,
    response_grants: [Vec<hagency_store::ApprovalResponseGrant>; 2],
}
impl Fixture {
    fn new(write: bool) -> Self {
        Self::new_at(write, 1000, 60_000)
    }
    fn new_at(write: bool, now: u64, lease_ms: u64) -> Self {
        Self::configured(write, now, lease_ms, true)
    }
    fn configured(write: bool, now: u64, lease_ms: u64, bind: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut db = DomainRepository::open(&root.path().join("state")).unwrap();
        db.register(&registration()).unwrap();
        let pool = resource("pool", "seat", 1000);
        db.put_resource(&pool).unwrap();
        let mut agents = vec![];
        let mut caps = vec![];
        let mut rooms = vec![];
        let mut contexts = vec![];
        let mut fingerprints = vec![];
        for name in ["a", "b"] {
            let p = proof(&request(name, name, &pool, 20));
            // Shared provisioning evidence has its own fixed observation time;
            // current runner/session authority is established below at `now`.
            let e = db.admit(&p, 1000).unwrap();
            db.approve(&format!("approve_{name}"), &p, 1000).unwrap();
            let effect = db.claim_effect().unwrap().unwrap();
            db.observe_effect(
                &effect.id,
                effect.fence,
                &EffectOutcome::Applied {
                    receipt: format!("provision {name}"),
                },
            )
            .unwrap();
            db.observe_matrix_transport(
                &MatrixTransportObservation {
                    engagement_id: e.id.clone(),
                    registration_generation: 1,
                    generation: 1,
                    sender_mxid: format!("@{name}:example.test"),
                    device_id: format!("DEV_{name}"),
                },
                now + 1,
            )
            .unwrap();
            db.observe_matrix_room(
                &MatrixRoomObservation {
                    engagement_id: e.id.clone(),
                    registration_generation: 1,
                    transport_generation: 1,
                    generation: 1,
                    room_id: "!project:example.test".into(),
                    privacy: RoomPrivacy::Group {},
                    joined: BTreeSet::from([
                        "@a:example.test".into(),
                        "@b:example.test".into(),
                        "@owner:example.test".into(),
                    ]),
                    invite_only: true,
                    encrypted: false,
                },
                now + 2,
            )
            .unwrap();
            db.resolve_verified_matrix_session(
                &SessionBinding {
                    id: name.into(),
                    engagement_id: e.id.clone(),
                    room_id: "!project:example.test".into(),
                    thread_root: None,
                },
                now + 3,
            )
            .unwrap();
            db.create_canonical_task(&format!("task_{name}"), name, "Approval task", now + 4)
                .unwrap();
            db.register_workspace(&format!("workspace_{name}")).unwrap();
            db.enqueue_dispatch(&DispatchInput {
                id: format!("dispatch_{name}"),
                session_id: name.into(),
                task_id: Some(format!("task_{name}")),
                resources: vec![ResourceLease {
                    id: format!("workspace_{name}"),
                    exclusive: write,
                }],
                payload: json!({"instruction":"test"}),
            })
            .unwrap();
            let cap = db
                .claim_dispatch(&format!("runner_{name}"), now + 5, lease_ms, 120_000, 8)
                .unwrap()
                .unwrap();
            if bind {
                db.start_dispatch(&cap, now + 6).unwrap();
            } else {
                let scope = db.owned_dispatch_scope(&cap, now + 6).unwrap();
                fingerprints.push(scope.fingerprint().to_owned());
                db.start_owned_dispatch(&cap, scope.fingerprint(), now + 6)
                    .unwrap();
            }
            let room = ApprovalRoomObservation {
                engagement_id: e.id.clone(),
                registration_generation: 1,
                generation: 1,
                room_id: "!private:example.test".into(),
                device_id: "BOT_DEVICE".into(),
                joined: BTreeSet::from([
                    "@owner:example.test".into(),
                    "@approval:example.test".into(),
                ]),
                invite_only: true,
                encrypted: true,
                available: true,
            };
            db.observe_approval_room(&room, now + 7).unwrap();
            let context = HostApprovalContext {
                id: format!("context_{name}"),
                connection_id: format!("connection_{name}"),
                thread_id: format!("thread_{name}"),
                turn_id: format!("turn_{name}"),
                workspace_resource: format!("workspace_{name}"),
                workspace: format!("/work/{name}"),
                windows_paths: false,
                environment_id: None,
                may_write: write,
                yolo: false,
            };
            if bind {
                db.bind_approval_context(&cap, &context, now + 8).unwrap();
            }
            agents.push(e.id);
            caps.push(cap);
            rooms.push(room);
            contexts.push(context);
        }
        Self {
            root,
            db,
            agents,
            caps,
            rooms,
            contexts,
            fingerprints,
            response_grants: [Vec::new(), Vec::new()],
        }
    }
    fn sql(&self) -> rusqlite::Connection {
        rusqlite::Connection::open(self.root.path().join("state/domain.sqlite3")).unwrap()
    }
    fn input(&self, agent: usize, id: u64) -> HostApprovalRequest {
        let c = &self.contexts[agent];
        HostApprovalRequest {
            context_id: c.id.clone(),
            upstream_id: ApprovalRpcId::Number(id),
            item_id: format!("item_{id}"),
            method: "item/commandExecution/requestApproval".into(),
            params: json!({"threadId":c.thread_id,"turnId":c.turn_id,"itemId":format!("item_{id}"),"command":"echo approved","cwd":c.workspace}),
            expires_at: 12000,
        }
    }
    fn admit(&mut self, agent: usize, id: u64) -> ApprovalSummary {
        let input = self.input(agent, id);
        self.db
            .request_owner_approval(&self.caps[agent], &input, 1010)
            .unwrap()
    }
    fn verdict(&self, id: &str, choice: ApprovalChoice, event: &str) -> OwnerVerdictObservation {
        let card = self.db.private_approval(id, 1011).unwrap();
        OwnerVerdictObservation {
            request_id: id.into(),
            request_digest: card.digest,
            binding_generation: card.binding_generation,
            server_name: "example.test".into(),
            room_id: card.room_id,
            sender_mxid: card.owner_mxid,
            event_id: format!("${event}"),
            encrypted: true,
            choice,
        }
    }
    fn choose(&mut self, id: &str, choice: ApprovalChoice) {
        let v = self.verdict(id, choice, id);
        self.db.observe_owner_verdict(&v, 1012).unwrap();
    }
    fn apply(&mut self, agent: usize, id: &str) -> ApprovalApplication {
        let grant = self
            .db
            .authorize_approval_response(&self.caps[agent], id, 1013)
            .unwrap();
        let app = grant.application().clone();
        self.response_grants[agent].push(grant);
        let waiting: bool = self.sql().query_row("SELECT EXISTS(SELECT 1 FROM owner_approvals a JOIN approval_contexts c ON c.id=a.context_id WHERE c.dispatch_id=?1 AND a.state IN ('pending','decided'))", [&self.caps[agent].dispatch_id], |r|r.get(0)).unwrap();
        if !waiting {
            // Host-observation fixture only: no runtime write/application proof.
            let grants = &mut self.response_grants[agent];
            self.db
                .begin_approval_responses(
                    &self.caps[agent],
                    grants,
                    std::time::Instant::now() + std::time::Duration::from_secs(1),
                    1013,
                )
                .unwrap();
            for grant in grants {
                self.db
                    .observe_approval_response(
                        grant,
                        hagency_store::ApprovalResponseObservation::WriteAccepted,
                    )
                    .unwrap();
            }
            self.response_grants[agent].clear();
        }
        self.db
            .observe_approval_application(&observed(&app, ApplicationOutcome::Applied), 1014)
            .unwrap();
        app
    }
    fn state(&self, agent: usize) -> String {
        self.sql()
            .query_row(
                "SELECT state FROM runner_dispatches WHERE id=?1",
                [&self.caps[agent].dispatch_id],
                |r| r.get(0),
            )
            .unwrap()
    }
    fn grants(&self, agent: usize) -> Vec<GrantSummary> {
        self.db
            .approval_grants(&self.agents[agent], "", 100)
            .unwrap()
    }
}
fn observed(
    app: &ApprovalApplication,
    outcome: ApplicationOutcome,
) -> ApprovalApplicationObservation {
    ApprovalApplicationObservation {
        application: app.clone(),
        outcome,
        evidence: "authenticated host observed exact native response write".into(),
    }
}
fn count(sql: &rusqlite::Connection, table: &str) -> u64 {
    sql.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}
#[test]
fn native_owner_approval_binding() {
    trait Ambiguous<A> {
        fn check() {}
    }
    impl<T: ?Sized> Ambiguous<()> for T {}
    impl<T: serde::de::DeserializeOwned> Ambiguous<u8> for T {}
    let _ = <ApprovalRoomObservation as Ambiguous<_>>::check;
    let _ = <HostApprovalContext as Ambiguous<_>>::check;
    let _ = <HostApprovalRequest as Ambiguous<_>>::check;
    let _ = <OwnerVerdictObservation as Ambiguous<_>>::check;
    let _ = <ApprovalApplicationObservation as Ambiguous<_>>::check;
    for change in ["third", "missing", "plaintext", "public", "device"] {
        let mut f = Fixture::new(true);
        assert_eq!(count(&f.sql(), "approval_rooms"), 1);
        assert_eq!(count(&f.sql(), "approval_bindings"), 2);
        let a = f.admit(0, 1);
        let b = f.admit(1, 1);
        let good = f.verdict(&a.id, ApprovalChoice::Always, "owner");
        for wrong in [
            "sender",
            "other_server",
            "room",
            "public",
            "plaintext",
            "generation",
            "digest",
            "agent",
        ] {
            let mut v = good.clone();
            match wrong {
                "sender" => v.sender_mxid = "@other:example.test".into(),
                "other_server" => v.sender_mxid = "@owner:evil.test".into(),
                "room" => v.room_id = "!another:example.test".into(),
                "public" => v.room_id = "!project:example.test".into(),
                "plaintext" => v.encrypted = false,
                "generation" => v.binding_generation += 1,
                "digest" => v.request_digest = "f".repeat(64),
                _ => v.request_id = b.id.clone(),
            };
            assert!(f.db.observe_owner_verdict(&v, 1012).is_err(), "{wrong}");
        }
        f.db.observe_owner_verdict(&good, 1012).unwrap();
        f.choose(&b.id, ApprovalChoice::Always);
        assert!(!f.grants(0)[0].revoked);
        let mut room = f.rooms[1].clone();
        room.generation = 2;
        match change {
            "third" => {
                room.joined.insert("@third:example.test".into());
            }
            "missing" => room.available = false,
            "plaintext" => room.encrypted = false,
            "public" => room.invite_only = false,
            _ => room.device_id = "ROTATED".into(),
        }
        f.db.observe_approval_room(&room, 1013).unwrap();
        assert!(f.grants(0)[0].revoked && f.grants(1)[0].revoked);
        assert!(
            f.db.consume_owner_approval(&f.caps[0], &a.id, 1014)
                .is_err()
        );
        assert!(
            f.db.consume_owner_approval(&f.caps[1], &b.id, 1014)
                .is_err()
        );
        // A fresh safe generation does not resurrect old decisions or grants.
        let mut safe = f.rooms[0].clone();
        safe.generation = 3;
        f.db.observe_approval_room(&safe, 1015).unwrap();
        assert!(
            f.db.consume_owner_approval(&f.caps[0], &a.id, 1016)
                .is_err()
        );
    }
}
#[test]
fn native_owner_approval_context() {
    let mut f = Fixture::new(true);
    for field in ["resource", "yolo", "workspace", "id", "generation"] {
        let mut c = f.contexts[0].clone();
        match field {
            "resource" => c.workspace_resource = "workspace_b".into(),
            "yolo" => c.yolo = true,
            "workspace" => c.workspace = "relative".into(),
            "id" => c.connection_id = "changed".into(),
            _ => c.turn_id = "different".into(),
        };
        assert!(
            f.db.bind_approval_context(&f.caps[0], &c, 1009).is_err(),
            "{field}"
        );
    }
    for field in ["thread", "turn", "item", "environment", "context"] {
        let mut input = f.input(0, 1);
        match field {
            "thread" => input.params["threadId"] = json!("bad"),
            "turn" => input.params["turnId"] = json!("bad"),
            "item" => input.item_id = "bad".into(),
            "environment" => input.params["environmentId"] = json!("bad"),
            _ => input.context_id = f.contexts[1].id.clone(),
        };
        assert!(
            f.db.request_owner_approval(&f.caps[0], &input, 1010)
                .is_err(),
            "{field}"
        );
    }
    let input = f.input(0, 1);
    let mut bad = f.caps[0].clone();
    bad.fence += 1;
    assert!(f.db.request_owner_approval(&bad, &input, 1010).is_err());
    let mut readonly = Fixture::new(false);
    let mut c = readonly.contexts[0].clone();
    c.id = "write".into();
    c.may_write = true;
    assert!(
        readonly
            .db
            .bind_approval_context(&readonly.caps[0], &c, 1009)
            .is_err()
    );
    c.may_write = false;
    c.yolo = true;
    assert!(
        readonly
            .db
            .bind_approval_context(&readonly.caps[0], &c, 1009)
            .is_err()
    );
    let mut input = readonly.input(0, 1);
    assert!(
        readonly
            .db
            .request_owner_approval(&readonly.caps[0], &input, 1010)
            .is_err()
    );
    input.params.as_object_mut().unwrap().remove("command");
    input.params["networkApprovalContext"] = json!({"host":"example.com","protocol":"https"});
    let a = readonly
        .db
        .request_owner_approval(&readonly.caps[0], &input, 1010)
        .unwrap();
    readonly.choose(&a.id, ApprovalChoice::Once);
    assert!(readonly.apply(0, &a.id).allow);
    let mut unknown = f.input(0, 2);
    unknown.method = "unknown/requestApproval".into();
    let a =
        f.db.request_owner_approval(&f.caps[0], &unknown, 1010)
            .unwrap();
    assert!(!a.reusable_scope);
    let v = f.verdict(&a.id, ApprovalChoice::Always, "unsupported");
    assert!(f.db.observe_owner_verdict(&v, 1012).is_err());
    f.choose(&a.id, ApprovalChoice::Once);
    assert!(f.apply(0, &a.id).allow);
}
#[test]
fn native_owner_approval_application() {
    let mut f = Fixture::new(true);
    let a = f.admit(0, 1);
    let b = f.admit(0, 2);
    assert_eq!(f.state(0), "parked");
    assert!(f.db.park_dispatch(&f.caps[0], false, 1011).is_err());
    assert!(
        f.db.consume_owner_approval(&f.caps[0], &a.id, 1011)
            .is_err()
    );
    f.choose(&a.id, ApprovalChoice::Once);
    let app = f.apply(0, &a.id);
    assert!(app.allow);
    assert_eq!(f.state(0), "parked");
    assert!(f.db.park_dispatch(&f.caps[0], false, 1015).is_err());
    assert!(
        f.db.consume_owner_approval(&f.caps[0], &a.id, 1015)
            .is_err()
    );
    f.choose(&b.id, ApprovalChoice::Deny);
    let deny = f.apply(0, &b.id);
    assert!(!deny.allow);
    assert_eq!(f.state(0), "started");
    assert_eq!(
        f.db.observe_approval_application(&observed(&app, ApplicationOutcome::Applied), 1015)
            .unwrap()
            .state,
        "applied"
    );
    let mut wrong = observed(&app, ApplicationOutcome::Applied);
    wrong.application.upstream_id = ApprovalRpcId::String("1".into());
    assert!(f.db.observe_approval_application(&wrong, 1015).is_err());
    let c = f.admit(0, 3);
    f.choose(&c.id, ApprovalChoice::Once);
    let pending =
        f.db.consume_owner_approval(&f.caps[0], &c.id, 1013)
            .unwrap();
    f.db.observe_approval_application(&observed(&pending, ApplicationOutcome::Unknown), 1014)
        .unwrap();
    assert_eq!(f.db.approval_summary(&c.id).unwrap().state, "uncertain");
    assert!(
        f.db.consume_owner_approval(&f.caps[0], &c.id, 1015)
            .is_err()
    );
    assert!(f.db.park_dispatch(&f.caps[0], false, 1015).is_err());
    f.db.observe_approval_application(&observed(&pending, ApplicationOutcome::NotApplied), 1016)
        .unwrap();
    assert!(f.db.park_dispatch(&f.caps[0], false, 1017).is_err());
    assert!(
        f.db.consume_owner_approval(&f.caps[0], &c.id, 1017)
            .is_err()
    );
}
#[test]
fn native_owner_approval_grants() {
    let mut f = Fixture::new(true);
    let a = f.admit(0, 1);
    f.choose(&a.id, ApprovalChoice::Always);
    f.apply(0, &a.id);
    let grant = f.grants(0)[0].id.clone();
    let b = f.admit(1, 1);
    assert_eq!(b.state, "pending");
    let second = f.admit(0, 2);
    assert_eq!(second.state, "decided");
    f.db.revoke_approval_grant(&grant).unwrap();
    assert!(
        f.db.authorize_approval_response(&f.caps[0], &second.id, 1013)
            .is_err()
    );
    let denied =
        f.db.consume_owner_approval(&f.caps[0], &second.id, 1013)
            .unwrap();
    assert!(!denied.allow);
    f.db.observe_approval_application(&observed(&denied, ApplicationOutcome::Applied), 1014)
        .unwrap();
    assert_eq!(
        f.state(0),
        "parked",
        "legacy application cannot resume execution"
    );
    // A separate live attempt exercises Task grants; refused legacy authority
    // above is never upgraded into a new router-response grant.
    let mut f = Fixture::new(true);
    let b = f.admit(1, 1);
    // Task grants stop at canonical Done; granting does not complete the task.
    let third = f.admit(0, 3);
    f.choose(&third.id, ApprovalChoice::Task);
    f.apply(0, &third.id);
    let fourth = f.admit(0, 4);
    assert_eq!(fourth.state, "decided");
    f.apply(0, &fourth.id);
    f.db.mutate_task(
        &f.caps[0],
        "task_a",
        "done",
        &TaskMutation::Transition {
            status: TaskState::Done,
            waiting_reason: None,
            waiting_until: None,
        },
        1020,
    )
    .unwrap();
    assert!(f.grants(0).iter().all(|g| g.revoked));
    // Completion after owner consent but before consumption cannot be revived by a grant.
    let id = b.id;
    f.choose(&id, ApprovalChoice::Task);
    f.sql().execute("UPDATE canonical_tasks SET config=json_set(config,'$.status','done','$.execution_epoch',1) WHERE id='task_b'",[]).unwrap();
    assert!(f.db.consume_owner_approval(&f.caps[1], &id, 1021).is_err());
    assert!(f.grants(1)[0].revoked);
    for changed in ["workspace", "environment", "may_write"] {
        let mut f = Fixture::new(true);
        let mut input = f.input(0, 1);
        input.params.as_object_mut().unwrap().remove("command");
        input.params["networkApprovalContext"] = json!({"host":"example.com","protocol":"https"});
        let a =
            f.db.request_owner_approval(&f.caps[0], &input, 1010)
                .unwrap();
        f.choose(&a.id, ApprovalChoice::Always);
        f.apply(0, &a.id);
        let mut c = f.contexts[0].clone();
        c.id = "new_context".into();
        c.connection_id = "new_connection".into();
        match changed {
            "workspace" => c.workspace = "/work/another".into(),
            "environment" => c.environment_id = Some("new_environment".into()),
            _ => c.may_write = false,
        };
        f.db.bind_approval_context(&f.caps[0], &c, 1020).unwrap();
        input.context_id = c.id;
        input.params["cwd"] = json!(c.workspace);
        if let Some(env) = c.environment_id {
            input.params["environmentId"] = json!(env);
        }
        assert_eq!(
            f.db.request_owner_approval(&f.caps[0], &input, 1021)
                .unwrap()
                .state,
            "pending",
            "{changed}"
        );
    }
    for changed in ["owner", "binding", "engagement", "registration"] {
        let mut f = Fixture::new(true);
        let a = f.admit(0, 1);
        f.choose(&a.id, ApprovalChoice::Always);
        match changed {
            "owner" => {
                f.sql()
                    .execute("UPDATE projects SET owner_mxid='@another:example.test'", [])
                    .unwrap();
                f.sql()
                    .execute("UPDATE projects SET owner_mxid='@owner:example.test'", [])
                    .unwrap();
            }
            "binding" => {
                let mut room = f.rooms[0].clone();
                room.generation = 2;
                room.device_id = "new_device".into();
                f.db.observe_approval_room(&room, 1013).unwrap();
            }
            "engagement" => {
                f.db.revoke("revoke", &f.agents[0]).unwrap();
            }
            _ => {
                let mut reg = registration();
                reg.generation = 2;
                f.db.register(&reg).unwrap();
            }
        }
        assert!(f.grants(0)[0].revoked, "{changed}");
        assert!(
            f.db.consume_owner_approval(&f.caps[0], &a.id, 1014)
                .is_err(),
            "{changed}"
        );
    }
}
#[test]
fn native_owner_approval_recovery() {
    let mut f = Fixture::new(true);
    let sql = f.sql();
    sql.execute_batch("CREATE TRIGGER fail_request AFTER INSERT ON owner_approvals BEGIN SELECT RAISE(ABORT,'injected request'); END;").unwrap();
    let input = f.input(0, 1);
    assert!(
        f.db.request_owner_approval(&f.caps[0], &input, 1010)
            .is_err()
    );
    assert_eq!(f.state(0), "started");
    assert_eq!(count(&sql, "owner_approvals"), 0);
    sql.execute_batch("DROP TRIGGER fail_request;").unwrap();
    let a = f.admit(0, 1);
    let verdict = f.verdict(&a.id, ApprovalChoice::Always, "owner");
    sql.execute_batch("CREATE TRIGGER fail_verdict AFTER INSERT ON approval_verdict_receipts BEGIN SELECT RAISE(ABORT,'injected verdict'); END;").unwrap();
    assert!(f.db.observe_owner_verdict(&verdict, 1012).is_err());
    assert!(f.grants(0).is_empty());
    assert_eq!(f.db.approval_summary(&a.id).unwrap().state, "pending");
    assert_eq!(count(&sql, "approval_verdict_receipts"), 0);
    sql.execute_batch("DROP TRIGGER fail_verdict;").unwrap();
    f.db.observe_owner_verdict(&verdict, 1012).unwrap();
    assert_eq!(
        f.db.observe_owner_verdict(&verdict, 1013).unwrap().state,
        "decided"
    );
    assert_eq!(f.grants(0).len(), 1);
    let mut conflict = verdict.clone();
    conflict.choice = ApprovalChoice::Deny;
    assert!(f.db.observe_owner_verdict(&conflict, 1013).is_err());
    sql.execute_batch("CREATE TRIGGER fail_consume AFTER UPDATE ON owner_approvals WHEN NEW.state='applying' BEGIN SELECT RAISE(ABORT,'injected consume'); END;").unwrap();
    assert!(
        f.db.consume_owner_approval(&f.caps[0], &a.id, 1013)
            .is_err()
    );
    assert_eq!(f.db.approval_summary(&a.id).unwrap().state, "decided");
    sql.execute_batch("DROP TRIGGER fail_consume;").unwrap();
    let app =
        f.db.consume_owner_approval(&f.caps[0], &a.id, 1013)
            .unwrap();
    assert!(app.allow);
    let pending = f.admit(0, 2);
    drop(f.db);
    f.db = DomainRepository::open(&f.root.path().join("state")).unwrap();
    assert_eq!(f.db.approval_summary(&a.id).unwrap().state, "uncertain");
    assert_eq!(f.db.approval_summary(&pending.id).unwrap().state, "decided");
    assert_eq!(f.state(0), "outcome_unknown");
    assert!(
        f.db.consume_owner_approval(&f.caps[0], &a.id, 1014)
            .is_err()
    );
    assert!(
        f.db.consume_owner_approval(&f.caps[0], &pending.id, 1014)
            .is_err()
    );
    f.db.observe_approval_application(&observed(&app, ApplicationOutcome::Applied), 1015)
        .unwrap();
    assert_eq!(f.db.approval_summary(&a.id).unwrap().state, "applied");
    assert_eq!(f.state(0), "outcome_unknown");
    assert!(
        f.db.consume_owner_approval(&f.caps[0], &a.id, 1016)
            .is_err()
    );
    for expired in ["approval", "lease", "capability", "task"] {
        let mut f = Fixture::new(true);
        let mut input = f.input(0, 1);
        if expired == "approval" {
            input.expires_at = 1011;
        }
        let a =
            f.db.request_owner_approval(&f.caps[0], &input, 1010)
                .unwrap();
        let v = f.verdict(&a.id, ApprovalChoice::Once, "verdict");
        match expired {
            "lease" => {
                f.sql()
                    .execute(
                        "UPDATE runner_dispatches SET lease_until=1011 WHERE id='dispatch_a'",
                        [],
                    )
                    .unwrap();
            }
            "capability" => {
                f.sql()
                    .execute(
                        "UPDATE runner_dispatches SET capability_until=1011 WHERE id='dispatch_a'",
                        [],
                    )
                    .unwrap();
            }
            "task" => {
                f.sql().execute("UPDATE canonical_tasks SET config=json_set(config,'$.status','done','$.execution_epoch',1) WHERE id='task_a'",[]).unwrap();
            }
            _ => {}
        }
        assert!(f.db.observe_owner_verdict(&v, 1012).is_err(), "{expired}");
        let applied = f.db.consume_owner_approval(&f.caps[0], &a.id, 1012);
        if expired == "approval" {
            assert!(!applied.unwrap().allow);
        } else {
            assert!(applied.is_err());
        }
    }
}
#[test]
fn native_owner_approval_bounds() {
    let mut f = Fixture::new(true);
    let first = f.admit(0, 1);
    let input = f.input(0, 1);
    assert_eq!(
        f.db.request_owner_approval(&f.caps[0], &input, 1011)
            .unwrap(),
        first
    );
    let mut changed = input.clone();
    changed.params["command"] = json!("changed");
    assert!(matches!(
        f.db.request_owner_approval(&f.caps[0], &changed, 1011),
        Err(Error::Conflict)
    ));
    changed = input.clone();
    changed.upstream_id = ApprovalRpcId::String("1".into());
    let distinct =
        f.db.request_owner_approval(&f.caps[0], &changed, 1011)
            .unwrap();
    assert_ne!(distinct.id, first.id);
    for id in 2..=15 {
        f.admit(0, id);
    }
    let over = f.input(0, 16);
    assert!(matches!(
        f.db.request_owner_approval(&f.caps[0], &over, 1011),
        Err(Error::Capacity)
    ));
    assert_eq!(count(&f.sql(), "owner_approvals"), 16);
    assert_eq!(
        f.db.request_owner_approval(&f.caps[0], &input, 1011)
            .unwrap()
            .id,
        first.id
    );
    let mut huge = f.input(1, 1);
    huge.params["reason"] = json!("x".repeat(50 * 1024));
    assert!(
        f.db.request_owner_approval(&f.caps[1], &huge, 1011)
            .is_err()
    );
    let mut nested = json!("deep");
    for _ in 0..70 {
        nested = json!([nested]);
    }
    huge.params = nested;
    assert!(
        f.db.request_owner_approval(&f.caps[1], &huge, 1011)
            .is_err()
    );
    f.choose(&first.id, ApprovalChoice::Always);
    let public = value(f.db.approval_summary(&first.id).unwrap());
    let grants = value(f.grants(0));
    let encoded = format!("{public}{grants}");
    for secret in [
        "!private",
        "@owner",
        "/work/",
        "thread_a",
        "turn_a",
        "connection_a",
        "item_1",
        "echo approved",
        "source_key",
        "workspace",
    ] {
        assert!(!encoded.contains(secret), "{secret}");
    }
}
#[test]
fn native_owner_approval_recovery_persistent_grant() {
    let mut f = Fixture::new(true);
    let first = f.admit(0, 1);
    f.choose(&first.id, ApprovalChoice::Always);
    f.apply(0, &first.id);
    f.db.mutate_task(
        &f.caps[0],
        "task_a",
        "done",
        &TaskMutation::Transition {
            status: TaskState::Done,
            waiting_reason: None,
            waiting_until: None,
        },
        1020,
    )
    .unwrap();
    f.db.complete_dispatch(&f.caps[0], &json!({"finished":true}), 1021)
        .unwrap();
    drop(f.db);
    f.db = DomainRepository::open(&f.root.path().join("state")).unwrap();
    assert!(!f.grants(0)[0].revoked);
    f.db.create_canonical_task("later_task", "a", "Later work", 2000)
        .unwrap();
    f.db.enqueue_dispatch(&DispatchInput {
        id: "later_dispatch".into(),
        session_id: "a".into(),
        task_id: Some("later_task".into()),
        resources: vec![ResourceLease {
            id: "workspace_a".into(),
            exclusive: true,
        }],
        payload: json!({"instruction":"later"}),
    })
    .unwrap();
    let cap =
        f.db.claim_dispatch("later_runner", 2001, 60_000, 120_000, 8)
            .unwrap()
            .unwrap();
    f.db.start_dispatch(&cap, 2002).unwrap();
    let mut c = f.contexts[0].clone();
    c.id = "later_context".into();
    c.connection_id = "later_connection".into();
    c.turn_id = "later_turn".into();
    f.db.bind_approval_context(&cap, &c, 2003).unwrap();
    let mut input = f.input(0, 2);
    input.context_id = c.id;
    input.params["turnId"] = json!(c.turn_id);
    let next = f.db.request_owner_approval(&cap, &input, 2004).unwrap();
    assert_eq!(next.state, "decided");
    assert!(
        f.db.consume_owner_approval(&cap, &next.id, 2005)
            .unwrap()
            .allow
    );
    assert_eq!(f.grants(0).len(), 1);
}
#[test]
fn native_owner_approval_recovery_schema12() {
    let temp = tempfile::tempdir().unwrap();
    let directory = temp.path().join("state");
    let db = DomainRepository::open(&directory).unwrap();
    drop(db);
    let sql = rusqlite::Connection::open(directory.join("domain.sqlite3")).unwrap();
    remove_approval_schema(&sql);
    sql.pragma_update(None, "user_version", 12).unwrap();
    for _ in 0..2 {
        let db = DomainRepository::open(&directory).unwrap();
        assert_eq!(
            sql.pragma_query_value(None, "user_version", |r| r.get::<_, u64>(0))
                .unwrap(),
            23
        );
        assert_eq!(count(&sql, "approval_bindings"), 0);
        assert_eq!(count(&sql, "approval_grants"), 0);
        drop(db);
    }
}
#[test]
fn native_owner_approval_binding_same_generation_negative() {
    for change in ["third", "encryption", "missing", "invite"] {
        let mut f = Fixture::new(true);
        let a = f.admit(0, 1);
        f.choose(&a.id, ApprovalChoice::Always);
        let old = f.rooms[0].clone();
        let mut negative = f.rooms[1].clone();
        match change {
            "third" => {
                negative.joined.insert("@third:example.test".into());
            }
            "encryption" => negative.encrypted = false,
            "missing" => negative.available = false,
            _ => negative.invite_only = false,
        }
        f.db.observe_approval_room(&negative, 1013).unwrap();
        assert!(f.grants(0)[0].revoked);
        assert!(
            f.db.consume_owner_approval(&f.caps[0], &a.id, 1014)
                .is_err()
        );
        f.db.observe_approval_room(&negative, 1014).unwrap();
        assert!(f.db.observe_approval_room(&old, 1015).is_err());
        assert!(f.db.private_approval(&a.id, 1015).is_err());
        let mut restored = old;
        restored.generation = 2;
        f.db.observe_approval_room(&restored, 1016).unwrap();
        assert!(
            f.db.consume_owner_approval(&f.caps[0], &a.id, 1017)
                .is_err()
        );
        assert!(f.grants(0)[0].revoked);
    }
}
#[test]
fn native_owner_approval_binding_rejects_another_project() {
    let mut f = Fixture::new(true);
    let pool = resource("pool", "seat", 1000);
    let mut incoming = request("other_project", "other_worker", &pool, 20);
    incoming.target_project_id = "project_two".into();
    incoming.target_room_id = "!other_project:example.test".into();
    let p = proof(&incoming);
    let e = f.db.admit(&p, 1010).unwrap();
    f.db.approve("other_approve", &p, 1010).unwrap();
    let effect = f.db.claim_effect().unwrap().unwrap();
    f.db.observe_effect(
        &effect.id,
        effect.fence,
        &EffectOutcome::Applied {
            receipt: "other provision".into(),
        },
    )
    .unwrap();
    let mut room = f.rooms[0].clone();
    room.engagement_id = e.id;
    assert!(f.db.observe_approval_room(&room, 1011).is_err());
    assert_eq!(count(&f.sql(), "approval_rooms"), 1);
    assert_eq!(count(&f.sql(), "approval_bindings"), 2);
    // Corrupting a project binding cannot make a public room an approval surface.
    f.sql()
        .execute(
            "UPDATE projects SET owner_room_id=room_id WHERE id='project_one'",
            [],
        )
        .unwrap();
    let mut room = f.rooms[0].clone();
    room.room_id = "!project:example.test".into();
    assert!(f.db.observe_approval_room(&room, 1012).is_err());
}

#[test]
fn native_matrix_transport_negative_retires_only_own_approval_authority() {
    let mut f = Fixture::new(true);
    for agent in 0..2 {
        let a = f.admit(agent, 1);
        f.choose(&a.id, ApprovalChoice::Always);
        f.apply(agent, &a.id);
    }
    let applying = f.admit(0, 2);
    f.db.consume_owner_approval(&f.caps[0], &applying.id, 1015)
        .unwrap();
    let decided = f.admit(0, 3);
    let expected =
        f.db.matrix_transport_state(&f.agents[0])
            .unwrap()
            .unwrap()
            .observation;
    f.db.invalidate_matrix_transport(
        &MatrixTransportInvalidation {
            expected,
            reason: "whoami mismatch".into(),
        },
        1016,
    )
    .unwrap();
    assert!(f.grants(0)[0].revoked);
    assert!(!f.grants(1)[0].revoked);
    for (id, state) in [(&applying.id, "uncertain"), (&decided.id, "invalidated")] {
        assert_eq!(
            f.sql()
                .query_row("SELECT state FROM owner_approvals WHERE id=?1", [id], |r| r
                    .get::<_, String>(0))
                .unwrap(),
            state
        );
    }
    assert!(
        f.db.consume_owner_approval(&f.caps[0], &decided.id, 1017)
            .is_err()
    );
}

#[path = "approvals/intake.rs"]
mod intake;
