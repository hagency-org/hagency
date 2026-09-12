use super::*;
use hagency_core::{approvals::*, replies::*};
use hagency_execution::{ApprovalHost, ApprovalRequests};
use std::collections::BTreeSet;

pub(super) fn bindings(db: &mut DomainRepository, engagement: &str) {
    db.observe_matrix_transport(
        &MatrixTransportObservation {
            engagement_id: engagement.into(),
            registration_generation: 1,
            generation: 1,
            sender_mxid: "@worker:example.test".into(),
            device_id: "WORKER".into(),
        },
        now(),
    )
    .unwrap();
    db.observe_matrix_room(
        &MatrixRoomObservation {
            engagement_id: engagement.into(),
            registration_generation: 1,
            transport_generation: 1,
            generation: 1,
            room_id: "!project:example.test".into(),
            privacy: RoomPrivacy::Group {},
            joined: BTreeSet::from(["@worker:example.test".into(), "@owner:example.test".into()]),
            invite_only: true,
            encrypted: false,
        },
        now(),
    )
    .unwrap();
    db.observe_approval_room(
        &ApprovalRoomObservation {
            engagement_id: engagement.into(),
            registration_generation: 1,
            generation: 1,
            room_id: "!private:example.test".into(),
            device_id: "BOT".into(),
            joined: BTreeSet::from([
                "@owner:example.test".into(),
                "@approval:example.test".into(),
            ]),
            invite_only: true,
            encrypted: true,
            available: true,
        },
        now(),
    )
    .unwrap();
}
pub(super) fn policy() -> ApprovalHost {
    ApprovalHost::new(4, 2, 20_000, 1500).unwrap()
}
pub(super) fn operation(
    f: &Fixture,
    mode: &str,
    policy: ApprovalHost,
) -> (Operation, ApprovalRequests) {
    let host = f.host(mode, "work", false).with_approvals(policy).unwrap();
    let mut operation = Operation::start(f.domain.clone(), f.cap.clone(), host, limits()).unwrap();
    let notices = operation.take_approval_requests().unwrap();
    assert!(operation.take_approval_requests().is_none());
    hagency_execution::diagnostics::reset();
    (operation, notices)
}
/// ADR-046 stage-1: when an owned approval cancels, name the primitive, the
/// offending entry, and every phase it had reached.
pub(super) fn cancellation_trace() -> String {
    hagency_execution::diagnostics::last_cancellation_trace()
}
/// Await one committed approval request notice. Never a bare "notice channel
/// closed": a closed channel or the expired deadline means the operation
/// ended early or never parked, so the operation's report and the retained
/// state are printed before panicking. The six-second deadline is unchanged;
/// hosted Ubuntu hit this twice with no evidence.
pub(super) async fn notice(
    f: &Fixture,
    operation: &mut Operation,
    notices: &mut ApprovalRequests,
) -> hagency_execution::ApprovalNotice {
    match tokio::time::timeout(Duration::from_secs(6), notices.recv()).await {
        Ok(Some(notice)) => notice,
        missed => {
            let report = operation.wait().await;
            let (protocol, canonical, settlement, failure, runtime) = match &report {
                Ok(report) => (
                    format!("{:?}", report.protocol),
                    format!("{:?}", report.canonical_status),
                    format!("{:?}", report.settlement),
                    format!("{:?}", report.failure),
                    format!("{:?}", report.runtime_observation()),
                ),
                Err(failure) => (
                    "no-report".to_owned(),
                    "no-report".to_owned(),
                    "no-report".to_owned(),
                    format!("{failure:?}"),
                    "no-report".to_owned(),
                ),
            };
            panic!(
                "approval notice never arrived (channel closed: {:?}); operation report \
                 protocol {protocol} canonical {canonical} settlement {settlement} \
                 failure {failure} runtime {runtime}; dispatch {}; marker {}; {}",
                missed.is_ok(),
                f.state(),
                f.marker().exists(),
                retained_state(f),
            );
        }
    }
}
/// The retained-state dump: liveness replica, contexts, routes, bindings and
/// requests, exactly as `choose()` pioneered for hosted-only verdict
/// refusals. Shared so every failure path leaves the same evidence.
pub(super) fn retained_state(f: &Fixture) -> String {
    let sql = f.sql();
    let liveness = sql
        .query_row(
            "SELECT d.state,d.fence,d.lease_until,d.capability_until,(SELECT COUNT(*) FROM resource_leases l WHERE l.dispatch_id=d.id),(SELECT group_concat(w.id||':'||w.dirty) FROM workspace_resources w),(SELECT json_extract(t.config,'$.execution_epoch') FROM canonical_tasks t WHERE t.id=d.task_id) FROM runner_dispatches d WHERE d.id='dispatch'",
            [],
            |r| {
                Ok(format!(
                    "state {} fence {} lease_until {} capability_until {} leases {} workspaces {:?} task_epoch {:?}",
                    r.get::<_, String>(0)?,
                    r.get::<_, u64>(1)?,
                    r.get::<_, Option<u64>>(2)?.map_or("null".into(), |v| v.to_string()),
                    r.get::<_, Option<u64>>(3)?.map_or("null".into(), |v| v.to_string()),
                    r.get::<_, u64>(4)?,
                    r.get::<_, Option<String>>(5)?,
                    r.get::<_, Option<i64>>(6)?
                ))
            },
        )
        .unwrap_or_else(|e| format!("liveness query failed: {e}"));
    let contexts = sql
        .prepare("SELECT id,config FROM approval_contexts")
        .and_then(|mut statement| {
            statement
                .query_map([], |r| {
                    Ok(format!(
                        "{}={}",
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()
        })
        .map(|rows| rows.join(" | "))
        .unwrap_or_else(|e| format!("context query failed: {e}"));
    let resources = sql
        .prepare("SELECT dispatch_id,resource_id,exclusive FROM resource_leases")
        .and_then(|mut statement| {
            statement
                .query_map([], |r| {
                    Ok(format!(
                        "{}:{}:{}",
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, i64>(2)?
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()
        })
        .map(|rows| rows.join(","))
        .unwrap_or_else(|e| format!("lease query failed: {e}"));
    // Generic row dump for the tables the writer re-derives its binding
    // and route from; hosted Windows is the only platform refusing here.
    let dump = |table: &str| -> String {
        sql.prepare(&format!("SELECT * FROM {table}"))
            .and_then(|mut statement| {
                let names: Vec<String> = statement
                    .column_names()
                    .iter()
                    .map(|n| n.to_string())
                    .collect();
                let rows = statement
                    .query_map([], |r| {
                        let mut cells = Vec::new();
                        for (index, name) in names.iter().enumerate() {
                            let text = match r.get_ref(index)? {
                                rusqlite::types::ValueRef::Null => "null".to_string(),
                                rusqlite::types::ValueRef::Integer(v) => v.to_string(),
                                rusqlite::types::ValueRef::Real(v) => v.to_string(),
                                rusqlite::types::ValueRef::Text(v) => {
                                    String::from_utf8_lossy(v).chars().take(400).collect()
                                }
                                rusqlite::types::ValueRef::Blob(v) => {
                                    format!("<{} bytes>", v.len())
                                }
                            };
                            cells.push(format!("{name}={text}"));
                        }
                        Ok(cells.join(" "))
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(rows.join(" | "))
            })
            .unwrap_or_else(|e| format!("{table} query failed: {e}"))
    };
    let routes = dump("matrix_session_routes");
    let bindings = dump("approval_bindings");
    let current_bindings = dump("current_approval_bindings");
    let rooms = dump("approval_rooms");
    let intended = dump("dispatch_resources");
    let tasks = dump("canonical_tasks");
    let requests = dump("owner_approvals");
    // The route check consults the current_matrix_routes view; dump it
    // and every table it joins so an empty view names the missing join.
    let current_routes = dump("current_matrix_routes");
    let sessions = dump("runner_sessions");
    let transports = dump("matrix_transports");
    let scopes = dump("matrix_room_scopes");
    let memberships = dump("matrix_room_memberships");
    // Replica of the writer's liveness predicate with this test's clock,
    // split so a hosted refusal names the failing term.
    let replica = |sql_text: &str| -> String {
        sql.query_row(
            sql_text,
            rusqlite::params!["dispatch", 1u64, "work", now(), true],
            |r| r.get::<_, bool>(0),
        )
        .map(|v| v.to_string())
        .unwrap_or_else(|e| format!("error: {e}"))
    };
    let live_full = replica(
        "SELECT EXISTS(SELECT 1 FROM runner_dispatches d JOIN resource_leases lease ON lease.dispatch_id=d.id AND lease.resource_id=?3 JOIN workspace_resources resource ON resource.id=lease.resource_id AND resource.dirty=0 JOIN dispatch_resources intended ON intended.dispatch_id=d.id AND intended.resource_id=lease.resource_id WHERE d.id=?1 AND d.fence=?2 AND d.state IN ('started','parked') AND d.lease_until>?4 AND d.capability_until>?4 AND (?5=0 OR (lease.exclusive=1 AND intended.exclusive=1)))",
    );
    let live_dispatch = replica(
        "SELECT EXISTS(SELECT 1 FROM runner_dispatches d WHERE d.id=?1 AND d.fence=?2 AND d.state IN ('started','parked') AND d.lease_until>?4 AND d.capability_until>?4 AND ?3=?3 AND ?5=?5)",
    );
    let live_joins = replica(
        "SELECT EXISTS(SELECT 1 FROM runner_dispatches d JOIN resource_leases lease ON lease.dispatch_id=d.id AND lease.resource_id=?3 JOIN workspace_resources resource ON resource.id=lease.resource_id AND resource.dirty=0 JOIN dispatch_resources intended ON intended.dispatch_id=d.id AND intended.resource_id=lease.resource_id WHERE d.id=?1 AND d.fence=?2 AND ?4=?4 AND ?5=?5)",
    );
    format!(
        "{liveness}; live_full {live_full} live_dispatch {live_dispatch} live_joins \
         {live_joins}; leases [{resources}]; intended [{intended}]; contexts \
         [{contexts}]; routes [{routes}]; bindings [{bindings}]; current_bindings \
         [{current_bindings}]; rooms [{rooms}]; tasks [{tasks}]; requests \
         [{requests}]; current_routes [{current_routes}]; sessions [{sessions}]; \
         transports [{transports}]; scopes [{scopes}]; memberships [{memberships}]"
    )
}
pub(super) async fn choose(f: &Fixture, id: &str, choice: ApprovalChoice) {
    let card = f.domain.private_approval(id.into()).await.unwrap();
    let expires_at = card.expires_at;
    let observation = OwnerVerdictObservation {
        request_id: id.into(),
        request_digest: card.digest,
        binding_generation: card.binding_generation,
        server_name: "example.test".into(),
        room_id: card.room_id,
        sender_mxid: card.owner_mxid,
        event_id: format!("${id}"),
        encrypted: true,
        choice,
    };
    // One attempt: hosted Windows once refused every verdict here because the
    // runner reported a verbatim callback cwd (ADR-116 amendment). A refusal
    // is a real defect, so it is reported with the retained state below.
    let result = f.domain.observe_owner_verdict(observation).await;
    if let Err(error) = result {
        // Hosted Windows has refused verdicts here without any local
        // reproduction; report the retained state instead of a bare unwrap.
        let summary = f.domain.approval_summary(id.into()).await.map(|s| s.state);
        panic!(
            "verdict refused: {error:?}; approval {summary:?}; dispatch {}; card expires_at {expires_at} now {}; marker {}; {}",
            f.state(),
            now(),
            f.marker().exists(),
            retained_state(f),
        );
    }
}
pub(super) fn responses(f: &Fixture) -> Vec<serde_json::Value> {
    fs::read_to_string(f.work.join("owned-dispatch.requests"))
        .unwrap()
        .lines()
        .map(|v| serde_json::from_str::<serde_json::Value>(v).unwrap())
        .filter(|v| v.get("result").is_some())
        .collect()
}
pub(super) fn unconfirmed(f: &Fixture) {
    assert_eq!(
        f.count("SELECT COUNT(*) FROM owner_approvals WHERE state='applied'"),
        0
    );
    assert_eq!(
        f.count("SELECT COUNT(*) FROM approval_responses WHERE write_accepted=1"),
        responses(f).len() as u64
    );
}
pub(super) async fn marker(f: &Fixture, extension: &str) {
    let until = tokio::time::Instant::now() + Duration::from_secs(5);
    while !f.work.join(format!("owned-dispatch.{extension}")).exists() {
        assert!(
            tokio::time::Instant::now() < until,
            "missing actual fixture marker {extension}"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}
