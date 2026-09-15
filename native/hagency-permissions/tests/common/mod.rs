#![allow(dead_code)]
use hagency_core::{authority::*, project::Resource};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
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
pub fn resource() -> Resource {
    serde_json::from_value(json!({"presetId":"pool","seatId":"seat","framework":"codex","model":"gpt-5.6-sol","reasoning":"medium","ceiling":{"tokens":1000,"period":"monthly"}})).unwrap()
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
pub fn proof(pool: &Resource) -> VerifiedRequest {
    let reg = registration();
    let request:ProjectRequest=serde_json::from_value(json!({"v":1,"fleetId":reg.fleet_id,"requestId":"request","requesterMxid":"@owner:example.test","sourceRoomId":"!reception:example.test","targetProjectId":"project_one","targetRoomId":"!project:example.test","ownerMxid":"@owner:example.test","ownerDmRoomId":"!private:example.test","role":"coding","requestedTokens":20,"ratePerDay":null,"authVersion":1,"sourceEventId":"$request","agentDefinition":{"name":"agent","resourceId":pool.id()}})).unwrap();
    let reception = room(
        &request.source_room_id,
        vec![
            request.requester_mxid.clone(),
            reg.representative_mxid.clone(),
        ],
    );
    let mut project = room(
        &request.target_room_id,
        vec![request.owner_mxid.clone(), reg.representative_mxid.clone()],
    );
    project.powers.insert(request.owner_mxid.clone(), 100);
    project.name = Some("Fixture".into());
    project.binding = Some(
        json!({"v":1,"fleetId":reg.fleet_id,"purpose":"project","projectId":request.target_project_id,"ownerMxid":request.owner_mxid,"authVersion":1}),
    );
    let mut owner_room = room(
        &request.owner_dm_room_id,
        vec![request.owner_mxid.clone(), reg.approval_bot_mxid.clone()],
    );
    owner_room.encryption = Some("m.megolm.v1.aes-sha2".into());
    let mut content = serde_json::to_value(&request).unwrap();
    content.as_object_mut().unwrap().remove("ownerDmRoomId");
    content.as_object_mut().unwrap().remove("sourceEventId");
    verify_request(
        &reg,
        request.clone(),
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
        },
    )
    .unwrap()
}
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis()
        .try_into()
        .unwrap()
}
pub fn value(v: &impl serde::Serialize) -> Value {
    serde_json::to_value(v).unwrap()
}
