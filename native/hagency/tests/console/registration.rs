use super::*;

// The G11 console route (spec `task-rust-project-side-registration`): an
// operator registers the fleet through `POST /console/api/project-sides`,
// gated by the lifecycle scope, and the route reaches the store's sole writer.
// The store's own guarantees are proven at the facade
// (hagency-store/tests/registration.rs); here we prove the route — the scope
// gate, the write, and that the answer reports the saved record and never the
// operator token.

/// The distinct fleet this test registers (the console fixture seeds `hf_aaa…`).
fn test_fleet() -> String {
    format!("hf_{}", "b".repeat(32))
}
fn registration_json(generation: u64) -> Value {
    let fleet = test_fleet();
    json!({
        "fleetId": fleet,
        "generation": generation,
        "serverName": "example.test",
        "receptionRoomId": "!reception2:example.test",
        "representativeMxid": format!("@{fleet}_representative:example.test"),
        "approvalBotMxid": "@approval2:example.test",
    })
}
fn fleet_rows(state: &std::path::Path) -> Vec<(String, u64)> {
    let db = rusqlite::Connection::open(state.join("domain.sqlite3")).unwrap();
    let mut stmt = db
        .prepare("SELECT fleet_id,generation FROM registrations ORDER BY rowid")
        .unwrap();
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

/// Scenario: an operator registers the fleet through the console route — the
/// write lands and the answer reports the saved record, never the token.
#[tokio::test]
async fn native_registration_route_writes_the_fleet_row() {
    let f = Fixture::new("127.0.0.1:13300".parse().unwrap(), None);
    let service = f.service();
    let state = f.root.path().join("state");
    let fleet = test_fleet();
    let before = fleet_rows(&state);
    assert!(
        !before.iter().any(|(id, _)| id == &fleet),
        "the test fleet starts unregistered"
    );
    // A read-only session cannot register: the mutation needs AgentLifecycle.
    let read_only = session(&service).await;
    let refused = post("/console/api/project-sides", &read_only)
        .json(&registration_json(1))
        .send(&service)
        .await;
    assert_eq!(refused.status_code, Some(StatusCode::FORBIDDEN));
    assert!(
        !fleet_rows(&state).iter().any(|(id, _)| id == &fleet),
        "a read-only session writes nothing"
    );
    // Ticket issuance is rate-limited to one per second.
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    let cookie = lifecycle_session(&service).await;
    let mut created = post("/console/api/project-sides", &cookie)
        .json(&registration_json(1))
        .send(&service)
        .await;
    assert_eq!(created.status_code, Some(StatusCode::OK));
    let answer = created.take_json::<Value>().await.unwrap();
    assert_eq!(answer["ok"], json!(true));
    assert_eq!(answer["side"]["id"], json!(fleet));
    assert_eq!(answer["side"]["generation"], json!(1));
    let text = serde_json::to_string(&answer).unwrap();
    assert!(
        !text.contains(TOKEN),
        "the answer never carries the operator token"
    );
    let after = fleet_rows(&state);
    assert!(
        after.iter().any(|(id, g)| id == &fleet && *g == 1),
        "exactly one row for the fleet at generation 1"
    );
    // Idempotent re-registration through the route is a no-op.
    let again = post("/console/api/project-sides", &cookie)
        .json(&registration_json(1))
        .send(&service)
        .await;
    assert_eq!(again.status_code, Some(StatusCode::OK));
    assert_eq!(
        fleet_rows(&state)
            .iter()
            .filter(|(id, _)| id == &fleet)
            .count(),
        1,
        "no second row"
    );
    f.close().await;
}

/// Scenario: a stale generation is refused at the route with the row unchanged,
/// and an invalid record is refused before any write.
#[tokio::test]
async fn native_registration_route_refuses_stale_and_invalid() {
    let f = Fixture::new("127.0.0.1:13300".parse().unwrap(), None);
    let service = f.service();
    let state = f.root.path().join("state");
    let fleet = test_fleet();
    let cookie = lifecycle_session(&service).await;
    let created = post("/console/api/project-sides", &cookie)
        .json(&registration_json(2))
        .send(&service)
        .await;
    assert_eq!(created.status_code, Some(StatusCode::OK));
    // A non-advancing generation is a conflict; the stored row still says 2.
    let stale = post("/console/api/project-sides", &cookie)
        .json(&registration_json(1))
        .send(&service)
        .await;
    assert_eq!(stale.status_code, Some(StatusCode::CONFLICT));
    assert_eq!(
        fleet_rows(&state)
            .iter()
            .find(|(id, _)| id == &fleet)
            .map(|(_, g)| *g),
        Some(2)
    );
    // An invalid record (malformed fleet id) is a bad request and writes nothing.
    let other_fleet = "not-a-fleet";
    let mut invalid = registration_json(1);
    invalid["fleetId"] = json!(other_fleet);
    let refused = post("/console/api/project-sides", &cookie)
        .json(&invalid)
        .send(&service)
        .await;
    assert_eq!(refused.status_code, Some(StatusCode::BAD_REQUEST));
    assert!(
        !fleet_rows(&state).iter().any(|(id, _)| id == other_fleet),
        "no row for the invalid record"
    );
    f.close().await;
}
