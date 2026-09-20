use super::*;
use hagency_store::{OutcomeAction, OutcomeResolution};
use serde_json::Value;

fn inspect(
    f: &mut Fixture,
    action: OutcomeAction,
    next: &DispatchInput,
) -> (Value, OutcomeResolution) {
    let inspection =
        f.db.begin_outcome_inspection(&f.engagement, "dispatch", 60_000, 1010)
            .unwrap();
    let command = OutcomeResolution {
        original: "dispatch".into(),
        request_id: "operator_resolution".into(),
        inspection_id: inspection["inspectionId"].as_str().unwrap().into(),
        inspection_token: inspection["inspectionToken"].as_str().unwrap().into(),
        action,
        operator_note: "Reviewed retained workspace and external effects".into(),
        replacement: (action == OutcomeAction::Continue).then(|| next.clone()),
    };
    (inspection, command)
}
fn custody(f: &Fixture) -> Value {
    let sql = f.sql();
    let query = |query: &str| -> Vec<String> {
        sql.prepare(query)
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    };
    json!([
        query("SELECT json_array(id,state,fence,input) FROM runner_dispatches ORDER BY id"),
        query("SELECT json_array(dispatch_id,fence,evidence,settled_at) FROM dispatch_stops"),
        query("SELECT json_array(resource_id,dispatch_id,exclusive) FROM resource_leases"),
        query("SELECT json_array(id,dirty) FROM workspace_resources"),
        query("SELECT json_array(id,quarantined,binding) FROM runner_sessions"),
        query("SELECT config FROM canonical_tasks"),
        query("SELECT json_array(id,consumed_at) FROM outcome_inspections ORDER BY id"),
        query("SELECT response FROM outcome_resolutions"),
        query("SELECT json_array(dispatch_id,processed_at) FROM session_inputs"),
    ])
}

#[test]
fn native_owned_stop_resolution_observation() {
    for action in [
        OutcomeAction::Continue,
        OutcomeAction::AcceptCompleted,
        OutcomeAction::KeepBlocked,
    ] {
        let (mut f, _, next) = stopped_fixture();
        assert!(!f.db.owned_stop_resolution_recorded(&f.cap).unwrap());
        let (_, command) = inspect(&mut f, action, &next);
        assert!(!f.db.owned_stop_resolution_recorded(&f.cap).unwrap());
        f.db.resolve_stopped_dispatch(&f.engagement, &command, 1011)
            .unwrap();
        assert!(f.db.owned_stop_resolution_recorded(&f.cap).unwrap());
        for field in ["id", "fence", "runner", "secret"] {
            let mut foreign = f.cap.clone();
            match field {
                "id" => foreign.dispatch_id = "foreign".into(),
                "fence" => foreign.fence += 1,
                "runner" => foreign.runner_id = "foreign".into(),
                _ => foreign.secret = "0".repeat(64),
            }
            assert!(
                matches!(
                    f.db.owned_stop_resolution_recorded(&foreign),
                    Err(Error::RunnerAuthority)
                ),
                "{field}"
            );
        }
        assert_eq!(f.count("SELECT COUNT(*) FROM runner_dispatches WHERE id='dispatch' AND state='outcome_unknown'"),1);
        f.sql()
            .execute("DELETE FROM outcome_resolutions", [])
            .unwrap();
        if action == OutcomeAction::Continue {
            f.sql()
                .execute("DELETE FROM dispatch_recoveries", [])
                .unwrap();
        }
        assert!(
            !f.db.owned_stop_resolution_recorded(&f.cap).unwrap(),
            "a settled stop alone cannot resume"
        );
    }
    let (mut f, digest, next) = stopped_fixture();
    f.db.continue_stopped_dispatch(
        &f.engagement,
        "dispatch",
        (f.cap.fence, &digest),
        &next,
        "reviewed effects",
        1010,
    )
    .unwrap();
    assert!(f.db.owned_stop_resolution_recorded(&f.cap).unwrap());
    f.sql()
        .execute("DELETE FROM owned_stop_inspections", [])
        .unwrap();
    assert!(!f.db.owned_stop_resolution_recorded(&f.cap).unwrap());
}

#[test]
fn native_outcome_resolution_actions() {
    for action in [
        OutcomeAction::Continue,
        OutcomeAction::AcceptCompleted,
        OutcomeAction::KeepBlocked,
    ] {
        let (mut f, _, next) = stopped_fixture();
        let (_, command) = inspect(&mut f, action, &next);
        // A queued older instruction must never outrun the operator's decision.
        f.sql().execute("INSERT INTO runner_dispatches(id,session_id,task_id,input,digest,state) SELECT 'older_queued',session_id,task_id,input,'old','queued' FROM runner_dispatches WHERE id='dispatch'",[]).unwrap();
        let response =
            f.db.resolve_stopped_dispatch(&f.engagement, &command, 1011)
                .unwrap();
        let task = f.db.canonical_task("task").unwrap();
        let status = match action {
            OutcomeAction::Continue => TaskState::InProgress,
            OutcomeAction::AcceptCompleted => TaskState::Done,
            OutcomeAction::KeepBlocked => TaskState::Blocked,
        };
        assert_eq!(task.status, status);
        assert_eq!(
            task.execution_epoch,
            u64::from(action == OutcomeAction::AcceptCompleted)
        );
        assert_eq!(
            task.completed_at,
            (action == OutcomeAction::AcceptCompleted).then_some(1011)
        );
        assert_eq!(
            task.waiting_reason.is_some(),
            action == OutcomeAction::KeepBlocked
        );
        assert_eq!(f.count("SELECT COUNT(*) FROM runner_dispatches WHERE id='dispatch' AND state='outcome_unknown'"),1);
        assert_eq!(f.count("SELECT COUNT(*) FROM runner_dispatches WHERE id='older_queued' AND state='superseded'"),1);
        assert_eq!(
            f.count("SELECT COUNT(*) FROM resource_leases WHERE dispatch_id='dispatch'"),
            0
        );
        assert_eq!(
            f.count("SELECT COUNT(*) FROM dispatch_stops WHERE settled_at=1011"),
            1
        );
        assert_eq!(
            f.count("SELECT COUNT(*) FROM session_inputs WHERE processed_at IS NOT NULL"),
            0
        );
        assert_eq!(f.count("SELECT COUNT(*) FROM owned_task_completions"), 0);
        assert_eq!(f.count("SELECT COUNT(*) FROM final_replies"), 0);
        assert_eq!(
            f.count("SELECT COUNT(*) FROM runner_outputs WHERE accepted=1"),
            0
        );
        assert_eq!(f.count("SELECT COUNT(*) FROM outcome_resolutions"), 1);
        let token_rows:String=f.sql().query_row("SELECT json_array(token_hash,snapshot_digest,receipt_digest) FROM outcome_inspections",[],|r|r.get(0)).unwrap();
        let evidence: String = f
            .sql()
            .query_row("SELECT evidence FROM dispatch_stops", [], |r| r.get(0))
            .unwrap();
        assert!(!token_rows.contains(&command.inspection_token));
        assert!(!evidence.contains(&command.inspection_token));
        if action == OutcomeAction::Continue {
            assert_eq!(f.count("SELECT COUNT(*) FROM session_inputs WHERE dispatch_id='recovery' AND processed_at IS NULL"),1);
            let claimed =
                f.db.claim_dispatch("next_runner", 1012, 60_000, 120_000, 1)
                    .unwrap()
                    .unwrap();
            assert_eq!(claimed.dispatch_id, next.id);
        } else {
            assert_eq!(f.count("SELECT COUNT(*) FROM session_inputs WHERE dispatch_id='dispatch' AND processed_at IS NULL"),1);
            assert!(
                f.db.claim_dispatch("next_runner", 1012, 60_000, 120_000, 1)
                    .unwrap()
                    .is_none()
            );
        }
        let path = f.root.path().join("state");
        drop(f.db);
        let mut db = DomainRepository::open(&path).unwrap();
        assert_eq!(
            db.resolve_stopped_dispatch(&f.engagement, &command, 100_000)
                .unwrap(),
            response
        );
        if action == OutcomeAction::Continue {
            let mut equivalent = command.clone();
            equivalent.replacement.as_mut().unwrap().payload["count"] = json!(1.0);
            assert_eq!(
                db.resolve_stopped_dispatch(&f.engagement, &equivalent, 100_000)
                    .unwrap(),
                response
            );
        }
        let mut altered = command.clone();
        altered.operator_note = "changed review".into();
        assert!(matches!(
            db.resolve_stopped_dispatch(&f.engagement, &altered, 100_001),
            Err(Error::Conflict)
        ));
        altered = command.clone();
        altered.request_id = "second_decision".into();
        assert!(matches!(
            db.resolve_stopped_dispatch(&f.engagement, &altered, 100_001),
            Err(Error::Conflict)
        ));
        assert!(matches!(
            db.resolve_stopped_dispatch("foreign_agent", &command, 100_001),
            Err(Error::NotFound)
        ));
        assert!(
            db.begin_outcome_inspection(&f.engagement, "dispatch", 60_000, 100_001)
                .is_err()
        );
    }
}

#[test]
fn native_outcome_resolution_rollback() {
    for action in [
        OutcomeAction::Continue,
        OutcomeAction::AcceptCompleted,
        OutcomeAction::KeepBlocked,
    ] {
        let (mut f, _, next) = stopped_fixture();
        let (_, command) = inspect(&mut f, action, &next);
        f.sql().execute_batch("CREATE TRIGGER fail_resolution BEFORE INSERT ON outcome_resolutions BEGIN SELECT RAISE(ABORT,'injected final receipt failure'); END;").unwrap();
        let before = custody(&f);
        let events = f.count("SELECT COUNT(*) FROM task_outbox");
        assert!(matches!(
            f.db.resolve_stopped_dispatch(&f.engagement, &command, 1011),
            Err(Error::Sqlite(_))
        ));
        assert_eq!(custody(&f), before);
        assert_eq!(f.count("SELECT COUNT(*) FROM task_outbox"), events);
        assert_eq!(f.count("SELECT COUNT(*) FROM dispatch_recoveries"), 0);
        f.sql()
            .execute_batch("DROP TRIGGER fail_resolution;")
            .unwrap();
        f.db.resolve_stopped_dispatch(&f.engagement, &command, 1012)
            .unwrap();
    }
}

#[test]
fn native_outcome_resolution_refusals() {
    for case in [
        "expired",
        "expiry_boundary",
        "token",
        "inspection",
        "agent",
        "original",
        "receipt",
        "fence",
        "scope",
        "missing",
        "reason",
        "done",
        "epoch",
        "task_changed",
        "stale",
        "lease",
        "dirty",
        "quarantine",
        "active",
        "sibling",
        "receive",
        "upload",
        "replacement",
        "instruction",
        "graph",
    ] {
        let (mut f, _, next) = stopped_fixture();
        let (_, mut command) = inspect(&mut f, OutcomeAction::AcceptCompleted, &next);
        let mut now = 1011;
        let mut agent = f.engagement.clone();
        match case {
            "expired" => now = 61_011,
            "expiry_boundary" => now = 61_010,
            "token" => command.inspection_token = "0".repeat(64),
            "inspection" => command.inspection_id = "absent".into(),
            "agent" => agent = "foreign".into(),
            "original" => command.original = "missing".into(),
            "receipt" => {
                f.sql()
                    .execute(
                        "UPDATE owned_stop_inspections SET digest=?1",
                        ["b".repeat(64)],
                    )
                    .unwrap();
            }
            "fence" => {
                f.sql()
                    .execute(
                        "UPDATE runner_dispatches SET fence=fence+1 WHERE id='dispatch'",
                        [],
                    )
                    .unwrap();
            }
            "scope" => {
                f.sql().execute("UPDATE owned_stop_inspections SET config=json_set(config,'$.scope','changed')",[]).unwrap();
            }
            "missing" => {
                f.sql()
                    .execute("DELETE FROM owned_stop_inspections", [])
                    .unwrap();
            }
            "reason" => {
                f.sql()
                    .execute("UPDATE dispatch_stops SET reason='membership_retired'", [])
                    .unwrap();
            }
            "done" => {
                f.sql().execute("UPDATE canonical_tasks SET config=json_set(config,'$.status','done') WHERE id='task'",[]).unwrap();
            }
            "epoch" => {
                f.sql().execute("UPDATE canonical_tasks SET config=json_set(config,'$.execution_epoch',1) WHERE id='task'",[]).unwrap();
            }
            "task_changed" => {
                f.sql().execute("UPDATE canonical_tasks SET config=json_set(config,'$.description','new task details') WHERE id='task'",[]).unwrap();
            }
            "stale" => {
                f.db.invalidate_matrix_transport(
                    &MatrixTransportInvalidation {
                        expected: f.transport.clone(),
                        reason: "retired".into(),
                    },
                    1011,
                )
                .unwrap();
            }
            "lease" => {
                f.sql().execute("DELETE FROM resource_leases", []).unwrap();
            }
            "dirty" => {
                f.sql()
                    .execute("UPDATE workspace_resources SET dirty=0", [])
                    .unwrap();
            }
            "quarantine" => {
                f.sql()
                    .execute("UPDATE runner_sessions SET quarantined=0", [])
                    .unwrap();
            }
            "active" => {
                f.sql().execute("INSERT INTO runner_dispatches(id,session_id,task_id,input,digest,state) SELECT 'active_sibling',session_id,task_id,input,'sibling','started' FROM runner_dispatches WHERE id='dispatch'",[]).unwrap();
            }
            "sibling" => {
                f.sql().execute("INSERT INTO runner_dispatches(id,session_id,task_id,input,digest,state) SELECT 'unknown_sibling',session_id,task_id,input,'sibling','outcome_unknown' FROM runner_dispatches WHERE id='dispatch'",[]).unwrap();
            }
            "receive" => {
                f.sql().execute("INSERT INTO received_files(id,capability_digest,event_id,workspace_id,binding,binding_digest,byte_limit,facts,state) VALUES('pending_receive','fixture','$file','workspace','{}','fixture',1,'{}','write_possible')",[]).unwrap();
            }
            "upload" => {
                f.sql().execute("INSERT INTO file_uploads(id,dispatch_id,call_id,request_digest,capability_digest,scope_fingerprint,route,preparation_hash,stage,stage_state,upload_state,created_at,updated_at) VALUES('pending_upload','dispatch','file','fixture','fixture','fixture','{}','fixture','{}','staged','write_possible',1007,1007)",[]).unwrap();
            }
            "replacement" => command.replacement = Some(next.clone()),
            "instruction" => {
                command.action = OutcomeAction::Continue;
                let mut next = next.clone();
                next.payload = json!({"instruction":"Verify result"});
                command.replacement = Some(next);
            }
            "graph" => {
                // Existing graph authority cannot be bypassed by a task-only resolution.
                f.sql().execute("INSERT INTO internal_conversations(id,fleet_id,project_id,generation,creator_session_id,request_scope,request_key,digest,config,state) SELECT 'conversation',fleet_id,project_id,generation,'session','fixture','fixture','fixture','{}','active' FROM engagements WHERE id=?1",[&f.engagement]).unwrap();
                f.sql().execute("INSERT INTO task_graphs(id,conversation_id,creator_session_id,creator_dispatch_id,fleet_id,project_id,generation,state,config,created_at) SELECT 'graph','conversation','session','dispatch',fleet_id,project_id,generation,'active','{}',1011 FROM engagements WHERE id=?1",[&f.engagement]).unwrap();
                f.sql().execute("INSERT INTO graph_nodes(graph_id,node_id,task_id,session_id,state) VALUES('graph','node','task','session','active')",[]).unwrap();
            }
            _ => unreachable!(),
        }
        let before = custody(&f);
        assert!(
            f.db.resolve_stopped_dispatch(&agent, &command, now)
                .is_err(),
            "{case}"
        );
        assert_eq!(custody(&f), before, "custody changed: {case}");
    }
}

#[test]
fn native_outcome_inspection_capacity() {
    let (mut f, _, _) = stopped_fixture();
    for ttl in [0, 59_999, 3_600_001] {
        assert!(
            f.db.begin_outcome_inspection(&f.engagement, "dispatch", ttl, 1010)
                .is_err()
        );
    }
    assert!(
        f.db.begin_outcome_inspection("foreign", "dispatch", 60_000, 1010)
            .is_err()
    );
    let mut ids = BTreeSet::new();
    for _ in 0..16 {
        let value =
            f.db.begin_outcome_inspection(&f.engagement, "dispatch", 60_000, 1010)
                .unwrap();
        assert!(ids.insert(value["inspectionId"].as_str().unwrap().to_owned()));
    }
    assert!(matches!(
        f.db.begin_outcome_inspection(&f.engagement, "dispatch", 60_000, 1010),
        Err(Error::Capacity)
    ));
    f.db.begin_outcome_inspection(&f.engagement, "dispatch", 60_000, 61_010)
        .unwrap();
    assert_eq!(f.count("SELECT COUNT(*) FROM outcome_inspections"), 1);
    f.sql()
        .execute("DELETE FROM owned_stop_inspections", [])
        .unwrap();
    assert!(
        f.db.begin_outcome_inspection(&f.engagement, "dispatch", 60_000, 61_010)
            .is_err()
    );
    assert_eq!(f.count("SELECT COUNT(*) FROM outcome_inspections"), 1);
}

#[test]
fn native_outcome_resolution_schema35_upgrade() {
    let (f, digest, _) = stopped_fixture();
    let path = f.root.path().join("state");
    drop(f.db);
    let sql = rusqlite::Connection::open(path.join("domain.sqlite3")).unwrap();
    sql.execute_batch(
        "DROP TABLE outcome_resolutions; DROP TABLE outcome_inspections; ALTER TABLE dispatch_inputs DROP COLUMN addressed; DROP TABLE IF EXISTS dispatch_conversation_reads; PRAGMA user_version=34;",
    )
    .unwrap();
    drop(sql);
    for _ in 0..2 {
        let db = DomainRepository::open(&path).unwrap();
        assert_eq!(
            db.owned_stop_inspection("dispatch", f.cap.fence)
                .unwrap()
                .unwrap()["digest"],
            digest
        );
        drop(db);
        let sql = rusqlite::Connection::open(path.join("domain.sqlite3")).unwrap();
        assert_eq!(
            sql.pragma_query_value(None, "user_version", |r| r.get::<_, u64>(0))
                .unwrap(),
            36
        );
        assert_eq!(
            sql.query_row("SELECT COUNT(*) FROM resource_leases", [], |r| r
                .get::<_, u64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            sql.query_row("SELECT COUNT(*) FROM outcome_inspections", [], |r| r
                .get::<_, u64>(0))
                .unwrap(),
            0
        );
    }
}
