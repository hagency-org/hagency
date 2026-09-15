use super::*;

// The engagement retire slice (ADR-150, spec `task-rust-engagement-retire`):
// the operator's retirement decision and cleanup retry through the console
// routes, asserting the store's own guarantees — the state guard, the
// decision digest replay, the same-transaction provision-cancel and
// retire-schedule, the `engagement_ends` stamp, the failed-only reset — plus
// the console's scope gate. No sweeper, timer or startup pass appears
// anywhere; the failed-waits scenario asserts that absence over elapsed time.
//
// The fixture seeds an `active` engagement (`private_usage_pool`) whose
// provision effect is already `complete`, the exact Given of the first
// scenario.

/// Drive one console mutation: POST with the lifecycle (or read-only) session.
async fn retire_post(service: &Service, path: &str, cookie: &str, body: &Value) -> Response {
    post(path, cookie).json(body).send(service).await
}

/// Read the seeded state's effect rows directly.
fn effects(state: &std::path::Path) -> Vec<(String, String, Option<String>, u64)> {
    let db = rusqlite::Connection::open(state.join("domain.sqlite3")).unwrap();
    let mut stmt = db
        .prepare("SELECT kind,state,outcome_digest,fence FROM effects ORDER BY rowid")
        .unwrap();
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

/// Scenario: an active engagement is retired and its obligations transfer.
#[tokio::test]
async fn native_engagement_retire_active_is_revoked() {
    let f = Fixture::new("127.0.0.1:13300".parse().unwrap(), None);
    let service = f.service();
    let state = f.root.path().join("state");
    // A read-only session cannot retire: the mutation needs AgentLifecycle.
    let read_only = session(&service).await;
    let refused = retire_post(
        &service,
        &format!("/console/api/engagements/{}/retire", f.engagement),
        &read_only,
        &json!({"commandId": "cmd_retire_1"}),
    )
    .await;
    assert_eq!(refused.status_code, Some(StatusCode::FORBIDDEN));
    // Ticket issuance is rate-limited to one per second.
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    let cookie = lifecycle_session(&service).await;
    let mut response = retire_post(
        &service,
        &format!("/console/api/engagements/{}/retire", f.engagement),
        &cookie,
        &json!({"commandId": "cmd_retire_1"}),
    )
    .await;
    assert_eq!(response.status_code, Some(StatusCode::OK));
    let body = response.take_json::<Value>().await.unwrap();
    assert_eq!(body["id"], f.engagement);
    assert_eq!(body["state"], "revoked");
    assert_eq!(body["cleanup"], "pending");
    assert_eq!(body.as_object().unwrap().len(), 3);
    let rows = effects(&state);
    let provision = rows.iter().find(|r| r.0 == "provision").unwrap();
    assert_eq!(provision.1, "cancelled");
    let retire = rows.iter().find(|r| r.0 == "retire").unwrap();
    assert_eq!(retire.1, "pending");
    let ends: u32 = rusqlite::Connection::open(state.join("domain.sqlite3"))
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM engagement_ends WHERE engagement_id=?1",
            [&f.engagement],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(ends, 1);
    f.close().await;
}

/// Scenario: an already-ended engagement cannot be retired again.
#[tokio::test]
async fn native_engagement_retire_requires_a_live_engagement() {
    let f = Fixture::new("127.0.0.1:13300".parse().unwrap(), None);
    let service = f.service();
    let state = f.root.path().join("state");
    let cookie = lifecycle_session(&service).await;
    let first = retire_post(
        &service,
        &format!("/console/api/engagements/{}/retire", f.engagement),
        &cookie,
        &json!({"commandId": "cmd_once"}),
    )
    .await;
    assert_eq!(first.status_code, Some(StatusCode::OK));
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let before = effects(&state);
    let mut second = retire_post(
        &service,
        &format!("/console/api/engagements/{}/retire", f.engagement),
        &cookie,
        &json!({"commandId": "cmd_second"}),
    )
    .await;
    assert_eq!(second.status_code, Some(StatusCode::CONFLICT));
    let refusal = second.take_json::<Value>().await.unwrap();
    assert_eq!(refusal["code"], "engagement_not_live");
    assert_eq!(effects(&state), before, "no effect rows changed");
    f.close().await;
}

/// Scenario: a replayed retirement command is idempotent.
#[tokio::test]
async fn native_engagement_retire_replays_the_recorded_decision() {
    let f = Fixture::new("127.0.0.1:13300".parse().unwrap(), None);
    let service = f.service();
    let state = f.root.path().join("state");
    let cookie = lifecycle_session(&service).await;
    let first = retire_post(
        &service,
        &format!("/console/api/engagements/{}/retire", f.engagement),
        &cookie,
        &json!({"commandId": "cmd_same"}),
    )
    .await;
    assert_eq!(first.status_code, Some(StatusCode::OK));
    let before = effects(&state);
    let mut replay = retire_post(
        &service,
        &format!("/console/api/engagements/{}/retire", f.engagement),
        &cookie,
        &json!({"commandId": "cmd_same"}),
    )
    .await;
    assert_eq!(replay.status_code, Some(StatusCode::OK));
    let body = replay.take_json::<Value>().await.unwrap();
    assert_eq!(body["id"], f.engagement);
    assert_eq!(body["state"], "revoked");
    assert_eq!(effects(&state), before, "no second write on replay");
    f.close().await;
}

/// Scenario: a reused command id with different content is a conflict.
#[tokio::test]
async fn native_engagement_retire_rejects_a_changed_replay() {
    let f = Fixture::new("127.0.0.1:13300".parse().unwrap(), None);
    let service = f.service();
    let state = f.root.path().join("state");
    let cookie = lifecycle_session(&service).await;
    let first = retire_post(
        &service,
        &format!("/console/api/engagements/{}/retire", f.engagement),
        &cookie,
        &json!({"commandId": "cmd_dup"}),
    )
    .await;
    assert_eq!(first.status_code, Some(StatusCode::OK));
    let before = effects(&state);
    // The same command id names a different act: the cleanup retry's digest
    // differs from the revoke digest, so the store conflicts.
    let mut changed = retire_post(
        &service,
        &format!("/console/api/engagements/{}/cleanup-retry", f.engagement),
        &cookie,
        &json!({"commandId": "cmd_dup"}),
    )
    .await;
    assert_eq!(changed.status_code, Some(StatusCode::CONFLICT));
    let refusal = changed.take_json::<Value>().await.unwrap();
    assert_eq!(refusal["code"], "command_conflict");
    assert_eq!(effects(&state), before, "no write on a conflicting replay");
    f.close().await;
}

/// Scenario: an unknown engagement is not found.
#[tokio::test]
async fn native_engagement_retire_unknown_is_not_found() {
    let f = Fixture::new("127.0.0.1:13300".parse().unwrap(), None);
    let service = f.service();
    let cookie = lifecycle_session(&service).await;
    let mut response = retire_post(
        &service,
        "/console/api/engagements/no_such_engagement/retire",
        &cookie,
        &json!({"commandId": "cmd_unknown"}),
    )
    .await;
    assert_eq!(response.status_code, Some(StatusCode::NOT_FOUND));
    let body = response.take_json::<Value>().await.unwrap();
    assert_eq!(body["code"], "not_found");
    f.close().await;
}

/// Seed a failed retire effect directly: the Given the spec names while the
/// retire driver is unwired — a `retire`-kind effect row in state `failed`.
fn seed_failed_retire(state: &std::path::Path, engagement: &str) {
    let db = rusqlite::Connection::open(state.join("domain.sqlite3")).unwrap();
    db.execute(
        "UPDATE effects SET state='failed',outcome_digest='digest_failed_retire' WHERE engagement_id=?1 AND kind='retire'",
        [engagement],
    )
    .unwrap();
    assert_eq!(db.changes(), 1);
}

/// Scenario: an operator retries a failed retirement.
#[tokio::test]
async fn native_engagement_retry_cleanup_requeues_a_failed_retire() {
    let f = Fixture::new("127.0.0.1:13300".parse().unwrap(), None);
    let service = f.service();
    let state = f.root.path().join("state");
    let cookie = lifecycle_session(&service).await;
    let retire = retire_post(
        &service,
        &format!("/console/api/engagements/{}/retire", f.engagement),
        &cookie,
        &json!({"commandId": "cmd_then_fail"}),
    )
    .await;
    assert_eq!(retire.status_code, Some(StatusCode::OK));
    seed_failed_retire(&state, &f.engagement);
    let mut response = retire_post(
        &service,
        &format!("/console/api/engagements/{}/cleanup-retry", f.engagement),
        &cookie,
        &json!({"commandId": "cmd_retry"}),
    )
    .await;
    assert_eq!(response.status_code, Some(StatusCode::OK));
    let body = response.take_json::<Value>().await.unwrap();
    assert_eq!(body["state"], "revoked", "the decision is unchanged");
    let rows = effects(&state);
    let retried = rows.iter().find(|r| r.0 == "retire").unwrap();
    assert_eq!(retried.1, "pending");
    assert!(retried.2.is_none(), "the outcome digest is cleared");
    f.close().await;
}

/// Scenario: a cleanup retry with nothing failed is refused.
#[tokio::test]
async fn native_engagement_retry_cleanup_requires_a_failed_retire() {
    let f = Fixture::new("127.0.0.1:13300".parse().unwrap(), None);
    let service = f.service();
    let state = f.root.path().join("state");
    let cookie = lifecycle_session(&service).await;
    // Not revoked at all: the fixture engagement is active.
    let active = retire_post(
        &service,
        &format!("/console/api/engagements/{}/cleanup-retry", f.engagement),
        &cookie,
        &json!({"commandId": "cmd_retry_active"}),
    )
    .await;
    assert_eq!(active.status_code, Some(StatusCode::CONFLICT));
    // Revoked but the retire effect is pending, not failed.
    let retire = retire_post(
        &service,
        &format!("/console/api/engagements/{}/retire", f.engagement),
        &cookie,
        &json!({"commandId": "cmd_retire_pending"}),
    )
    .await;
    assert_eq!(retire.status_code, Some(StatusCode::OK));
    let before = effects(&state);
    let pending = retire_post(
        &service,
        &format!("/console/api/engagements/{}/cleanup-retry", f.engagement),
        &cookie,
        &json!({"commandId": "cmd_retry_pending"}),
    )
    .await;
    assert_eq!(pending.status_code, Some(StatusCode::CONFLICT));
    assert_eq!(effects(&state), before, "no effects row modified");
    f.close().await;
}

/// Scenario: a failed retirement waits for the operator — no sweeper, no
/// timer, no startup pass. The route module adds no driver; this asserts the
/// failed row stays failed with time passing in-process.
#[tokio::test]
async fn native_engagement_failed_retire_waits_for_an_operator() {
    let f = Fixture::new("127.0.0.1:13300".parse().unwrap(), None);
    let service = f.service();
    let state = f.root.path().join("state");
    let cookie = lifecycle_session(&service).await;
    let retire = retire_post(
        &service,
        &format!("/console/api/engagements/{}/retire", f.engagement),
        &cookie,
        &json!({"commandId": "cmd_wait"}),
    )
    .await;
    assert_eq!(retire.status_code, Some(StatusCode::OK));
    seed_failed_retire(&state, &f.engagement);
    let before = effects(&state);
    tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
    let after = effects(&state);
    assert_eq!(after, before, "no automatic retry write occurred");
    let failed = after.iter().find(|r| r.0 == "retire").unwrap();
    assert_eq!(failed.1, "failed");
    f.close().await;
}
