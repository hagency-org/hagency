//! O2 (audit): the console's real-agent proof. The roster shows an agent whose
//! engagement was minted by the production Matrix ingress and made effective
//! and routable by the production verdict path — never a `seed()`ed row. The
//! fake peer delivers the provisioning request into the reception room, the
//! intake admits it, the representative's verdict makes it effective and binds
//! the session route, and only then does the real GET /console/api/agents
//! roster show it, keyed by its minted engagement id.
use super::*;

#[path = "../../../hagency-matrix/tests/common/mod.rs"]
pub mod matrix_common;

use hagency::{App, console::Console};
use hagency_core::replies::RoomPrivacy;
use hagency_matrix::{CancellationToken, Collector, HostConfig, HostIntakePlan, HostRoom};
use serde_json::json;
use sha2::Digest;

const SESSION_ROOM: &str = "!project:example.test";
const PROJECT: &str = "!project_provision:example.test";
const RECEPTION: &str = "!reception:example.test";
const PRIVATE: &str = "!private:example.test";
const OWNER: &str = "@owner:example.test";
const APPROVAL_BOT: &str = "@approval:example.test";

fn fleet_id() -> String {
    format!("hf_{}", "a".repeat(32))
}
fn representative() -> String {
    format!("@{}_representative:example.test", fleet_id())
}
fn agent_mxid() -> String {
    format!(
        "@en_{}:example.test",
        &format!(
            "{:x}",
            sha2::Sha256::digest(
                serde_json::to_vec(&[&fleet_id(), &"request_one".to_string()]).unwrap()
            )
        )[..32]
    )
}

fn member(mxid: &str) -> Value {
    json!({"type": "m.room.member", "state_key": mxid, "content": {"membership": "join"}})
}
fn power_levels(users: Value) -> Value {
    json!({
        "type": "m.room.power_levels",
        "state_key": "",
        "content": {"users": users, "users_default": 0, "invite": 0}
    })
}
fn session_state() -> Value {
    json!([
        member("@worker:example.test"),
        member(OWNER),
        {"type": "m.room.join_rules", "state_key": "", "content": {"join_rule": "invite"}},
        power_levels(json!({}))
    ])
}
fn reception_state() -> Value {
    json!([
        member("@worker:example.test"),
        member(OWNER),
        member(&representative()),
        {"type": "m.room.join_rules", "state_key": "", "content": {"join_rule": "invite"}},
        power_levels(json!({}))
    ])
}
fn project_state() -> Value {
    json!([
        member("@worker:example.test"),
        member(OWNER),
        member(&representative()),
        member(&agent_mxid()),
        {"type": "m.room.join_rules", "state_key": "", "content": {"join_rule": "invite"}},
        power_levels(json!({OWNER: 100, representative(): 50})),
        {"type": "m.room.name", "state_key": "", "content": {"name": "实际项目名称"}},
        {
            "type": "com.hagency.project.binding.v1",
            "state_key": "",
            "content": {
                "v": 1,
                "fleetId": fleet_id(),
                "purpose": "project",
                "projectId": "project_provision",
                "ownerMxid": OWNER,
                "authVersion": 1
            }
        }
    ])
}

fn request_body(request_id: &str, tokens: u64) -> String {
    serde_json::json!({
        "requestId": request_id,
        "requester": OWNER,
        "project": "project_provision",
        "projectRoomId": PROJECT,
        "role": "coding",
        "requestedTokens": tokens,
        "ratePerDay": null,
        "agent": "Provisioned",
        "context": {"agentDefinition": {"resourceId": "resource_27cac5503836765cd10751d2"}}
    })
    .to_string()
}
fn request_event(event_id: &str, body: String) -> Value {
    json!({
        "event_id": event_id,
        "sender": OWNER,
        "type": "m.room.message",
        "origin_server_ts": now(),
        "content": {"msgtype": "com.hagency.engagement.request.v1", "body": body}
    })
}
fn approval_event(event_id: &str, request_id: &str) -> Value {
    json!({
        "event_id": event_id,
        "sender": representative(),
        "type": "m.room.message",
        "origin_server_ts": now(),
        "content": {
            "msgtype": "com.hagency.engagement.approval.v1",
            "body": json!({"requestId": request_id, "decision": "approve"}).to_string()
        }
    })
}
fn provisioning_sync(token: &str, events: Vec<Value>) -> Value {
    json!({
        "next_batch": token,
        "rooms": {
            "join": {
                SESSION_ROOM: {"timeline": {"events": [], "limited": false}, "state": {"events": []}},
                RECEPTION: {"timeline": {"events": events, "limited": false}, "state": {"events": []}}
            }
        },
        "to_device": {"events": []}
    })
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

/// The collector config: the session-route room publishes; the reception room
/// is observed but never routed.
fn provisioning_config(f: &matrix_common::Fixture, endpoint: &str) -> HostConfig {
    let mut config = HostConfig::new(
        f.identity.clone(),
        endpoint,
        matrix_common::TOKEN,
        f.root.path().join("sdk"),
        [42; 32],
        vec![HostRoom {
            room_id: SESSION_ROOM.into(),
            generation: 1,
            privacy: RoomPrivacy::Group {},
        }],
        matrix_common::limits(),
    )
    .unwrap()
    .with_root_pem(include_bytes!(
        "../../../hagency-matrix/tests/fixtures/ca.pem"
    ))
    .unwrap();
    config
        .with_reception_room(HostRoom {
            room_id: RECEPTION.into(),
            generation: 1,
            privacy: RoomPrivacy::Group {},
        })
        .unwrap();
    config
}

/// Seed the enrolled owner-DM room the provisioning ingress reads fail-closed
/// before admit.
fn enroll_owner_room(f: &matrix_common::Fixture) {
    rusqlite::Connection::open(f.root.path().join("domain/domain.sqlite3"))
        .unwrap()
        .execute(
            "INSERT INTO approval_rooms(server_name,room_id,generation,fleet_id,project_id,registration_generation,owner_mxid,bot_mxid,device_id,available,digest,config) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,1,?10,?11)",
            rusqlite::params![
                "example.test",
                PRIVATE,
                1,
                fleet_id(),
                "project_one",
                1,
                OWNER,
                APPROVAL_BOT,
                "APPROVAL_DEVICE",
                "e".repeat(64),
                serde_json::json!({"joined":[OWNER,APPROVAL_BOT],"invite_only":true,"encrypted":true,"available":true}).to_string()
            ],
        )
        .unwrap();
}

/// Assemble the shared store: the provisioning fixture's domain (registration,
/// resource, host engagement) on one root.
async fn ready() -> (matrix_common::Fixture, matrix_common::Fake, Collector) {
    let f = matrix_common::Fixture::new();
    enroll_owner_room(&f);
    let fake = matrix_common::Fake::start(true).await;
    let c = Collector::new(provisioning_config(&f, &fake.endpoint), f.store.clone()).unwrap();
    (f, fake, c)
}

async fn prime(c: &Collector, f: &matrix_common::Fixture, fake: &mut matrix_common::Fake) {
    let cancel = CancellationToken::new();
    let (result, _) = matrix_common::scripted(c.collect(&cancel), async {
        fake.next().await.json(200, matrix_common::who());
        fake.next()
            .await
            .json(200, matrix_common::sync("bootstrap"));
        fake.next().await.json(200, session_state());
        fake.next().await.json(200, reception_state());
    })
    .await;
    result.unwrap();
    f.store
        .resolve_verified_matrix_session(hagency_core::tasks::SessionBinding {
            id: "root".into(),
            engagement_id: f.identity.transport.engagement_id.clone(),
            room_id: SESSION_ROOM.into(),
            thread_root: None,
        })
        .await
        .unwrap();
}

async fn run_provisioning(
    c: &Collector,
    fake: &mut matrix_common::Fake,
    value: Value,
) -> hagency_matrix::IntakeSummary {
    let cancel = CancellationToken::new();
    let intake = c.intake(HostIntakePlan::new(vec!["root".into()]).unwrap(), &cancel);
    let (result, ()) = matrix_common::scripted(intake, async {
        fake.next().await.json(200, matrix_common::who());
        let request = fake.next().await;
        assert!(request.target.contains("sync?"));
        request.json(200, value);
        fake.next().await.json(200, session_state());
        fake.next().await.json(200, reception_state());
        fake.next().await.json(200, project_state());
    })
    .await;
    result.unwrap()
}

/// The minted engagement id for request_one, read from the store the intake
/// admitted it into — never re-derived in the test.
fn minted_engagement_id(f: &matrix_common::Fixture) -> String {
    rusqlite::Connection::open(f.root.path().join("domain/domain.sqlite3"))
        .unwrap()
        .query_row(
            "SELECT id FROM engagements WHERE request_id='request_one'",
            [],
            |r| r.get(0),
        )
        .unwrap()
}

fn effect_row(f: &matrix_common::Fixture) -> Option<(String, String)> {
    rusqlite::Connection::open(f.root.path().join("domain/domain.sqlite3"))
        .unwrap()
        .query_row(
            "SELECT f.kind,f.state FROM effects f JOIN engagements e ON e.id=f.engagement_id WHERE e.request_id='request_one'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .ok()
}

fn route_rows(f: &matrix_common::Fixture) -> u64 {
    rusqlite::Connection::open(f.root.path().join("domain/domain.sqlite3"))
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM matrix_session_routes r JOIN runner_sessions s ON s.id=r.session_id JOIN engagements e ON e.id=s.engagement_id WHERE e.request_id='request_one'",
            [],
            |r| r.get(0),
        )
        .unwrap()
}

#[tokio::test]
async fn native_console_roster_shows_an_ingress_provisioned_agent() {
    let (f, mut fake, c) = ready().await;
    // The console-bearing app reads the SAME store the intake writes — the
    // owned_matrix precedent (Collector + served App on one store) plus the
    // console mount, served in-process through the console fixture's
    // Service + TestClient pattern (no TCP listener, no reqwest).
    let custody = hagency_store::Store::start(
        hagency_store::Repository::open(&f.root.path().join("custody")).unwrap(),
        16,
    )
    .unwrap();
    let asset_dir = f.root.path().join("assets");
    assets(&asset_dir);
    let console = Console::load(&asset_dir.canonicalize().unwrap()).unwrap();
    let address: std::net::SocketAddr = "127.0.0.1:13300".parse().unwrap();
    let app = App::new(custody.clone(), TOKEN.as_bytes(), address)
        .unwrap()
        .with_domain(f.store.clone())
        .with_console(console);
    let service = Service::new(app.router());

    // The fake peer delivers the request into the reception room; the intake
    // admits it; the representative's verdict makes it effective and routable.
    prime(&c, &f, &mut fake).await;
    let summary = run_provisioning(
        &c,
        &mut fake,
        provisioning_sync(
            "provision",
            vec![
                request_event("$request_one", request_body("request_one", 250)),
                approval_event("$approval_one", "request_one"),
            ],
        ),
    )
    .await;
    assert_eq!(summary.admitted, 2);
    let engagement = minted_engagement_id(&f);
    assert!(engagement.starts_with("en_"));
    assert_eq!(
        effect_row(&f),
        Some(("provision".into(), "complete".into()))
    );
    assert_eq!(route_rows(&f), 1);

    // The console roster, through its real HTTP route, names the minted id.
    let cookie = session(&service).await;
    let mut response = get("/console/api/agents", &cookie).send(&service).await;
    assert_eq!(response.status_code, Some(StatusCode::OK));
    let roster: Value = response.take_json().await.unwrap();
    let ids: Vec<&str> = roster["agents"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|a| a["engagement_id"].as_str())
        .collect();
    assert!(
        ids.contains(&engagement.as_str()),
        "roster lacks the minted engagement {engagement}: {ids:?}"
    );

    c.close().await.unwrap();
    custody.shutdown().await.unwrap();
}
