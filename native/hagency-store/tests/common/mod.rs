#![allow(dead_code)] // Shared by separate focused integration test executables.
use hagency_core::{authority::*, project::Resource};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub fn resource(preset: &str, seat: &str, tokens: u64) -> Resource {
    serde_json::from_value(
        json!({"presetId":preset,"seatId":seat,"framework":"codex","model":"gpt-5.6-sol","reasoning":"medium",
        "ceiling":{"tokens":tokens,"period":"monthly"}}),
    )
    .unwrap()
}
pub fn registration() -> Registration {
    let fleet = format!("hf_{}", "a".repeat(32));
    Registration {
        fleet_id: fleet.clone(),
        generation: 1,
        server_name: "example.test".into(),
        reception_room_id: "!reception:example.test".into(),
        representative_mxid: format!("@{fleet}_representative:example.test"),
        approval_bot_mxid: "@approval:example.test".into(),
    }
}
pub fn request(id: &str, name: &str, resource: &Resource, tokens: u64) -> ProjectRequest {
    serde_json::from_value(json!({"v":1,"fleetId":registration().fleet_id,"requestId":id,
        "requesterMxid":"@owner:example.test","sourceRoomId":"!reception:example.test","targetProjectId":"project_one","targetRoomId":"!project:example.test",
        "ownerMxid":"@owner:example.test","ownerDmRoomId":"!private:example.test","role":"coding","requestedTokens":tokens,"ratePerDay":null,"authVersion":1,
        "sourceEventId":format!("${id}"),"agentDefinition":{"name":name,"resourceId":resource.id()}})).unwrap()
}
fn room(id: &str, members: Vec<String>) -> RoomObservation {
    RoomObservation {
        room_id: id.into(),
        joined: BTreeSet::from_iter(members),
        invite_only: true,
        encryption: None,
        powers: BTreeMap::new(),
        default_power: 0,
        invite_power: 0,
        binding: None,
        name: None,
    }
}
pub fn observation(request: &ProjectRequest) -> RequestObservation {
    let reg = registration();
    let reception = room(
        &request.source_room_id,
        vec![
            request.requester_mxid.clone(),
            reg.representative_mxid.clone(),
        ],
    );
    let mut project = room(
        &request.target_room_id,
        vec![
            request.requester_mxid.clone(),
            request.owner_mxid.clone(),
            reg.representative_mxid,
        ],
    );
    project.powers.insert(request.owner_mxid.clone(), 100);
    project.name = Some("实际项目名称".into());
    project.binding = Some(
        json!({"v":1,"fleetId":reg.fleet_id,"purpose":"project","projectId":request.target_project_id,"ownerMxid":request.owner_mxid,"authVersion":1}),
    );
    let mut owner_room = room(
        &request.owner_dm_room_id,
        vec![request.owner_mxid.clone(), reg.approval_bot_mxid],
    );
    owner_room.encryption = Some("m.megolm.v1.aes-sha2".into());
    let mut content = serde_json::to_value(request).unwrap();
    content.as_object_mut().unwrap().remove("ownerDmRoomId");
    content.as_object_mut().unwrap().remove("sourceEventId");
    RequestObservation {
        registration_generation: 1,
        observed_at_ms: 1000,
        source: SourceObservation {
            event_id: request.source_event_id.clone(),
            room_id: request.source_room_id.clone(),
            sender: request.requester_mxid.clone(),
            event_type: "com.hagency.engagement.request.v1".into(),
            content,
        },
        reception,
        project,
        owner_room,
    }
}
pub fn proof(request: &ProjectRequest) -> VerifiedRequest {
    verify_request(&registration(), request.clone(), observation(request)).unwrap()
}
pub fn value<T: serde::Serialize>(value: T) -> Value {
    serde_json::to_value(value).unwrap()
}
/// Reconstruct pre-graph native schema for older migration regression fixtures.
pub fn remove_graph_schema(db: &rusqlite::Connection) {
    remove_reply_schema(db);
    let original: String = db
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type='view' AND name='conversation_peer_inputs'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    db.execute_batch("DROP VIEW admissible_dispatch_peer_inputs; DROP VIEW graph_dispatch_scope; DROP VIEW graph_dispatch_ready; DROP VIEW current_graph_scopes; DROP VIEW live_peer_inputs; DROP VIEW conversation_peer_inputs; DROP TABLE graph_dependencies; DROP TABLE graph_commands; DROP TABLE graph_nodes; DROP TABLE task_graphs;").unwrap();
    db.execute_batch(&original.replacen("conversation_peer_inputs", "live_peer_inputs", 1))
        .unwrap();
}
/// Restore schema 10 without inventing privacy for any earlier native session.
pub fn remove_reply_schema(db: &rusqlite::Connection) {
    remove_ingress_schema(db);
    db.execute_batch("DROP VIEW current_final_replies; DROP VIEW current_matrix_routes; DROP TABLE final_reply_inspections; DROP TABLE final_reply_calls; DROP TABLE final_replies; DROP TABLE matrix_session_routes; DROP TABLE matrix_room_memberships; DROP TABLE matrix_room_scopes; DROP TABLE matrix_transports; DROP INDEX canonical_runner_session; ALTER TABLE runner_sessions DROP COLUMN matrix_generation; CREATE UNIQUE INDEX canonical_runner_session ON runner_sessions(engagement_id,CASE WHEN json_extract(binding,'$.kind')='internal' THEN 'internal' ELSE 'matrix' END,CASE WHEN json_extract(binding,'$.kind')='internal' THEN id ELSE json_extract(binding,'$.room_id') END,COALESCE(json_extract(binding,'$.thread_root'),''));").unwrap();
}

/// Restore schema 11 without manufacturing new ingress provenance.
pub fn remove_ingress_schema(db: &rusqlite::Connection) {
    remove_approval_schema(db);
    db.execute_batch("DROP VIEW current_final_replies; DROP VIEW current_matrix_routes; DROP VIEW task_followup_ready; DROP TABLE verified_task_requests; DROP TABLE matrix_ingress_events; ALTER TABLE matrix_transports DROP COLUMN observed_at; ALTER TABLE matrix_room_scopes DROP COLUMN visibility_since; ALTER TABLE matrix_session_routes DROP COLUMN ingress_since; ALTER TABLE matrix_session_routes DROP COLUMN parent_session_id; ALTER TABLE session_inputs DROP COLUMN config; ALTER TABLE task_inputs DROP COLUMN config; ALTER TABLE task_inputs DROP COLUMN wake; ALTER TABLE task_notices DROP COLUMN verified_route; ALTER TABLE task_notices DROP COLUMN content_digest;").unwrap();
    let routes = include_str!("../../src/migrations/011-final-replies.sql");
    db.execute_batch(reply_route_view(routes)).unwrap();
    db.execute_batch(&routes[routes.find("CREATE VIEW current_final_replies AS").unwrap()..])
        .unwrap();
    let tasks = include_str!("../../src/migrations/005-task-intents.sql");
    db.execute_batch(&tasks[tasks.find("CREATE VIEW task_followup_ready AS").unwrap()..])
        .unwrap();
}

/// Reconstruct schema 12 without backfilling owner approval authority.
pub fn remove_approval_schema(db: &rusqlite::Connection) {
    remove_notice_schema(db);
    db.execute_batch("DROP TRIGGER approval_room_retire_grants; DROP TRIGGER approval_project_retire; DROP TRIGGER approval_registration_retire; DROP TRIGGER approval_engagement_retire; DROP TRIGGER approval_task_retire; DROP VIEW current_approval_bindings; DROP TABLE approval_verdict_receipts; DROP TABLE approval_grants; DROP TABLE owner_approvals; DROP TABLE approval_contexts; DROP TABLE approval_bindings; DROP TABLE approval_rooms;").unwrap();
}

/// SQL statement boundaries survive LF and Windows checkout CRLF spelling.
pub fn reply_route_view(routes: &str) -> &str {
    let start = routes.find("CREATE VIEW current_matrix_routes AS").unwrap();
    let end = routes.find("CREATE TABLE final_replies").unwrap();
    &routes[start..end]
}

/// Rebuild the actual schema13 notice shape; a pre-custody sender could hold a claim.
pub fn remove_notice_schema(db: &rusqlite::Connection) {
    remove_matrix_transport_schema(db);
    db.execute_batch("DROP TABLE notice_send_inspections; DROP INDEX task_notice_ready; ALTER TABLE task_notices RENAME TO task_notices_newer;").unwrap();
    let schema = include_str!("../../src/migrations/005-task-intents.sql");
    let start = schema.find("CREATE TABLE task_notices").unwrap();
    let end = schema.find("-- One predicate").unwrap();
    db.execute_batch(&schema[start..end]).unwrap();
    db.execute_batch("ALTER TABLE task_notices ADD COLUMN verified_route TEXT CHECK(verified_route IS NULL OR json_valid(verified_route)); ALTER TABLE task_notices ADD COLUMN content_digest TEXT; INSERT INTO task_notices(id,task_id,config,state,claim_hash,claim_until,delivery,error_code,not_before,verified_route,content_digest) SELECT id,task_id,config,CASE WHEN state IN ('sending','uncertain') THEN 'claimed' ELSE state END,claim_hash,claim_until,delivery,error_code,not_before,verified_route,content_digest FROM task_notices_newer; DROP TABLE task_notices_newer;").unwrap();
}

/// Restore schema14 without weakening its notice-send custody.
pub fn remove_matrix_transport_schema(db: &rusqlite::Connection) {
    remove_owned_completion_schema(db);
    db.execute_batch(
        "DROP TRIGGER matrix_transport_retire_approvals; DROP VIEW current_matrix_routes;",
    )
    .unwrap();
    let schema = include_str!("../../src/migrations/012-verified-ingress.sql");
    let start = schema.find("CREATE VIEW current_matrix_routes AS").unwrap();
    let end = schema.find("DROP VIEW task_followup_ready;").unwrap();
    db.execute_batch(&schema[start..end]).unwrap();
    db.execute_batch("ALTER TABLE matrix_transports DROP COLUMN available; ALTER TABLE matrix_transports DROP COLUMN invalidation;").unwrap();
}

/// Restore schema15 without manufacturing completion or owner authority.
pub fn remove_owned_completion_schema(db: &rusqlite::Connection) {
    remove_usage_schema(db);
    db.execute_batch("DROP TABLE owned_task_completions;")
        .unwrap();
}

/// Restore schema16 without inventing usage for historical execution paths.
pub fn remove_usage_schema(db: &rusqlite::Connection) {
    remove_attachment_schema(db);
    db.execute_batch("DROP TABLE usage_receipts; DROP TABLE usage_periods; DROP TABLE usage_sources; DROP TABLE usage_clock;").unwrap();
}

/// Remove delivery and upload additions before constructing an older database.
pub fn remove_upload_schema(db: &rusqlite::Connection) {
    db.execute_batch("DROP TABLE IF EXISTS approval_responses; DROP TABLE IF EXISTS received_files; DROP TABLE IF EXISTS file_deliveries; DROP TABLE IF EXISTS file_uploads;")
        .unwrap();
}

pub fn remove_attachment_schema(db: &rusqlite::Connection) {
    remove_upload_schema(db);
    db.execute_batch("DROP TABLE IF EXISTS dispatch_attachment_windows; DROP TABLE IF EXISTS session_attachment_visibility; DROP TABLE IF EXISTS matrix_attachments;").unwrap();
}
