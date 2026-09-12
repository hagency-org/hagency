use hagency::App;
use hagency_matrix::CancellationToken;
use hagency_store::{DomainRepository, DomainStore, Repository, Store};
use salvo::{
    prelude::*,
    test::{ResponseExt, TestClient},
};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
const TOKEN: &str = "fixture_operator_token_32_bytes_minimum";
const BASE: &str = "http://127.0.0.1:13300";

/// Brief 19: wire the app the way `Bootstrap::serve` does — writers plus the
/// shared ceiling-sweep handle and its tick channel — so the readiness
/// read observes the real serving shape.
async fn readiness_app(
    state: &std::path::Path,
    sweep: Option<Duration>,
) -> (Arc<Service>, Option<CancellationToken>) {
    let custody = Store::start(Repository::open(state).unwrap(), 16).unwrap();
    let domain = DomainStore::start(DomainRepository::open(state).unwrap(), 16).unwrap();
    let mut app = App::new(
        custody.clone(),
        TOKEN.as_bytes(),
        "127.0.0.1:13300".parse().unwrap(),
    )
    .unwrap()
    .with_domain(domain.clone());
    let cancel = match sweep {
        None => None,
        Some(period) => {
            let cancel = CancellationToken::new();
            let (handle, tick) =
                hagency::bootstrap::start_ceiling_sweep(domain.clone(), cancel.clone(), period);
            app = app.with_ceiling_sweep(Arc::new(handle), tick);
            Some(cancel)
        }
    };
    (Arc::new(Service::new(app.router())), cancel)
}

fn component_states(value: &Value) -> std::collections::BTreeMap<String, String> {
    value["components"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            (
                c["name"].as_str().unwrap().to_owned(),
                c["state"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

#[tokio::test]
async fn native_health_readiness_ready() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    // A short period plus a settled first tick keeps the rollup
    // deterministic: the loop is alive and the last tick is a real sweep.
    let (service, cancel) = readiness_app(&state, Some(Duration::from_millis(50))).await;
    let mut response = TestClient::get(format!("{BASE}/health"))
        .add_header("host", "127.0.0.1:13300", true)
        .send(&*service)
        .await;
    assert_eq!(response.status_code, Some(StatusCode::OK));
    let value = response.take_json::<Value>().await.unwrap();
    assert_eq!(value["status"], "ok");
    assert_eq!(value["implementation"], "rust");
    assert!(
        value.get("stage").is_none(),
        "the foundation-era stage word is gone"
    );
    let states = component_states(&value);
    assert_eq!(states["domain_writer"], "open");
    assert_eq!(states["custody_store"], "open");
    // Before the first tick the sweep is `unstarted`; after it, `alive`/`swept`.
    // Both are ready words — readiness never depends on a sweep HAVING run.
    assert!(matches!(
        states["ceiling_sweep"].as_str(),
        "alive" | "unstarted"
    ));
    assert!(matches!(
        states["ceiling_sweep_last_tick"].as_str(),
        "swept" | "unstarted"
    ));
    cancel.expect("the ready case wires the sweep").cancel();
}

/// F1: a refused tick is a LIVE loop that was refused, not a dead one — the
/// tick's outcome word never feeds readiness. Injected per the brief ("a
/// test that injects a refused tick"): the tick channel carries the loop's
/// own refusal word for a saturated writer, and the handle is a REAL live
/// task (a spawn that runs until cancelled), so liveness is observed, not
/// mocked. The remaining outcome words are enumerated against the same
/// predicate by the F3 test.
#[tokio::test]
async fn native_health_readiness_refused_tick_is_ready() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    let custody = Store::start(Repository::open(&state).unwrap(), 16).unwrap();
    let domain = DomainStore::start(DomainRepository::open(&state).unwrap(), 16).unwrap();
    // The injected refusal: the exact word the loop publishes when the
    // writer queue is saturated (bootstrap.rs:440), on the exact channel
    // type the loop reports through.
    let (seed, tick) =
        tokio::sync::watch::channel(hagency::bootstrap::CeilingSweepTick::Refused("busy"));
    let cancel = CancellationToken::new();
    // A genuinely-live task: it runs and never finishes until cancelled, so
    // the liveness probe observes a real task, exactly as it would the loop.
    let live = tokio::spawn({
        let gate = cancel.clone();
        async move {
            gate.cancelled().await;
        }
    });
    let shared = Arc::new(live);
    let app = App::new(
        custody.clone(),
        TOKEN.as_bytes(),
        "127.0.0.1:13300".parse().unwrap(),
    )
    .unwrap()
    .with_domain(domain.clone())
    .with_ceiling_sweep(shared.clone(), tick);
    let service = Arc::new(Service::new(app.router()));
    // /health: ALWAYS 200 while the process is live (F2) — the refused tick
    // is body detail, never a 503.
    let mut response = TestClient::get(format!("{BASE}/health"))
        .add_header("host", "127.0.0.1:13300", true)
        .send(&*service)
        .await;
    assert_eq!(response.status_code, Some(StatusCode::OK));
    let value = response.take_json::<Value>().await.unwrap();
    assert_eq!(
        value["status"], "ok",
        "a refused tick never fails readiness"
    );
    let states = component_states(&value);
    assert_eq!(states["ceiling_sweep"], "alive", "the loop is live");
    assert_eq!(
        states["ceiling_sweep_last_tick"], "refused_busy",
        "the refusal stays on the wire for diagnosis"
    );
    // /ready: also ready — the same predicate, one vocabulary.
    let response = TestClient::get(format!("{BASE}/ready"))
        .add_header("host", "127.0.0.1:13300", true)
        .send(&*service)
        .await;
    assert_eq!(response.status_code, Some(StatusCode::OK));
    drop(seed);
    cancel.cancel();
    // The handle lives inside the Arc the app borrowed; wait on liveness
    // (is_finished) instead of awaiting the Arc, which is not a future.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !shared.is_finished() && std::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    custody.shutdown().await.unwrap();
    domain.shutdown().await.unwrap();
}

/// F3: ONE vocabulary — every enum variant's word and ready answer, so the
/// wire words and the predicate can never disagree again.
#[test]
fn native_health_readiness_enumerates_every_state() {
    use hagency::ComponentState;
    let every: Vec<(ComponentState, &str, bool)> = vec![
        (ComponentState::Open, "open", true),
        (ComponentState::Alive, "alive", true),
        (ComponentState::Disabled, "disabled", true),
        (ComponentState::Ready, "ready", true),
        (ComponentState::Unstarted, "unstarted", true),
        (ComponentState::Running, "running", true),
        (
            ComponentState::Tick(hagency::TickOutcome::Swept),
            "swept",
            true,
        ),
        (
            ComponentState::Tick(hagency::TickOutcome::RefusedBusy),
            "refused_busy",
            true,
        ),
        (
            ComponentState::Tick(hagency::TickOutcome::RefusedOutcomeUnknown),
            "refused_outcome_unknown",
            true,
        ),
        (
            ComponentState::Tick(hagency::TickOutcome::Refused),
            "refused",
            true,
        ),
        (ComponentState::Closed, "closed", false),
        (ComponentState::Stopped, "stopped", false),
        (ComponentState::Unavailable, "unavailable", false),
        (ComponentState::OutcomeUnknown, "outcome_unknown", false),
        (ComponentState::NotStarted, "not_started", false),
    ];
    for (state, word, is_ready) in every {
        assert!(state.is_ready() == is_ready, "{word} readiness mismatch");
        assert_eq!(state.word(), word, "the wire word is derived from the enum");
    }
}

#[tokio::test]
async fn native_health_readiness_names_stopped_sweep() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    // A long period means the only transition under test is the stop.
    let custody = Store::start(Repository::open(&state).unwrap(), 16).unwrap();
    let domain = DomainStore::start(DomainRepository::open(&state).unwrap(), 16).unwrap();
    let cancel = CancellationToken::new();
    let (handle, tick) = hagency::bootstrap::start_ceiling_sweep(
        domain.clone(),
        cancel.clone(),
        Duration::from_secs(3600),
    );
    let shared = Arc::new(handle);
    cancel.cancel();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !shared.is_finished() && std::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(
        shared.is_finished(),
        "the sweep loop must stop after cancellation"
    );
    let app = App::new(
        custody.clone(),
        TOKEN.as_bytes(),
        "127.0.0.1:13300".parse().unwrap(),
    )
    .unwrap()
    .with_domain(domain.clone())
    .with_ceiling_sweep(shared, tick);
    let service = Arc::new(Service::new(app.router()));
    // F2: /health keeps the retained contract — 200 whenever the process is
    // live, the stopped sweep as body detail; /ready is the 503 boundary.
    let mut response = TestClient::get(format!("{BASE}/health"))
        .add_header("host", "127.0.0.1:13300", true)
        .send(&*service)
        .await;
    assert_eq!(response.status_code, Some(StatusCode::OK));
    let value = response.take_json::<Value>().await.unwrap();
    assert_eq!(
        value["status"], "unavailable",
        "the body still tells the truth"
    );
    let states = component_states(&value);
    assert_eq!(states["ceiling_sweep"], "stopped", "named by component");
    assert_eq!(states["ceiling_sweep_last_tick"], "unstarted");
    // One bad component never lies about the healthy ones.
    assert_eq!(states["domain_writer"], "open");
    assert_eq!(states["custody_store"], "open");
    // /ready: 503 with the same component list — never a silent 200.
    let mut response = TestClient::get(format!("{BASE}/ready"))
        .add_header("host", "127.0.0.1:13300", true)
        .send(&*service)
        .await;
    assert_eq!(response.status_code, Some(StatusCode::SERVICE_UNAVAILABLE));
    let value = response.take_json::<Value>().await.unwrap();
    assert_eq!(value["status"], "unavailable");
    assert_eq!(
        component_states(&value)["ceiling_sweep"],
        "stopped",
        "the same body on the refusing boundary"
    );
}

#[tokio::test]
async fn native_health_readiness_names_closed_domain_writer() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    let custody = Store::start(Repository::open(&state).unwrap(), 16).unwrap();
    let domain = DomainStore::start(DomainRepository::open(&state).unwrap(), 16).unwrap();
    domain.shutdown().await.unwrap();
    let app = App::new(
        custody.clone(),
        TOKEN.as_bytes(),
        "127.0.0.1:13300".parse().unwrap(),
    )
    .unwrap()
    .with_domain(domain.clone());
    let service = Arc::new(Service::new(app.router()));
    // F2: /health keeps the retained contract — 200 while the process is
    // live, the closed writer as body detail; /ready is the 503 boundary.
    let mut response = TestClient::get(format!("{BASE}/health"))
        .add_header("host", "127.0.0.1:13300", true)
        .send(&*service)
        .await;
    assert_eq!(response.status_code, Some(StatusCode::OK));
    let value = response.take_json::<Value>().await.unwrap();
    assert_eq!(value["status"], "unavailable");
    let states = component_states(&value);
    assert_eq!(states["domain_writer"], "closed", "named by component");
    assert_eq!(states["custody_store"], "open");
    // No sweep was wired: `disabled` is a ready word, so the 503 is honest
    // about exactly one component.
    assert_eq!(states["ceiling_sweep"], "disabled");
    let mut response = TestClient::get(format!("{BASE}/ready"))
        .add_header("host", "127.0.0.1:13300", true)
        .send(&*service)
        .await;
    assert_eq!(
        response.status_code,
        Some(StatusCode::SERVICE_UNAVAILABLE),
        "never a silent 200 on /ready"
    );
    let value = response.take_json::<Value>().await.unwrap();
    assert_eq!(component_states(&value)["domain_writer"], "closed");
    custody.shutdown().await.unwrap();
}

#[tokio::test]
async fn native_resource_management_is_authenticated() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    let custody = Store::start(Repository::open(&state).unwrap(), 16).unwrap();
    let domain = DomainStore::start(DomainRepository::open(&state).unwrap(), 16).unwrap();
    let app = App::new(
        custody.clone(),
        TOKEN.as_bytes(),
        "127.0.0.1:13300".parse().unwrap(),
    )
    .unwrap()
    .with_domain(domain.clone());
    let service = Service::new(app.router());
    let mut roles = TestClient::get(format!("{BASE}/api/native/v1/roles"))
        .add_header("host", "127.0.0.1:13300", true)
        .bearer_auth(TOKEN)
        .send(&service)
        .await;
    assert!(
        roles
            .take_json::<Value>()
            .await
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["available"] == false)
    );
    let resource = json!({"presetId":"private_preset","seatId":"private_seat","framework":"codex","model":"gpt-5.6-sol","reasoning":"medium","ceiling":{"tokens":100}});
    let mut supplied_roles = resource.clone();
    supplied_roles["roles"] = json!(["architect"]);
    let denied = TestClient::post(format!("{BASE}/api/native/v1/resources"))
        .add_header("host", "127.0.0.1:13300", true)
        .bearer_auth(TOKEN)
        .json(&supplied_roles)
        .send(&service)
        .await;
    assert_eq!(denied.status_code, Some(StatusCode::BAD_REQUEST));
    let denied = TestClient::post(format!("{BASE}/api/native/v1/resources"))
        .add_header("host", "127.0.0.1:13300", true)
        .json(&resource)
        .send(&service)
        .await;
    assert_eq!(denied.status_code, Some(StatusCode::UNAUTHORIZED));
    let denied = TestClient::post(format!("{BASE}/api/native/v1/resources"))
        .add_header("host", "127.0.0.1:13300", true)
        .bearer_auth(TOKEN)
        .add_header("origin", "https://attacker.test", true)
        .json(&resource)
        .send(&service)
        .await;
    assert_eq!(denied.status_code, Some(StatusCode::FORBIDDEN));
    let mut created = TestClient::post(format!("{BASE}/api/native/v1/resources"))
        .add_header("host", "127.0.0.1:13300", true)
        .bearer_auth(TOKEN)
        .json(&resource)
        .send(&service)
        .await;
    assert_eq!(created.status_code, Some(StatusCode::OK));
    let created: Value = created.take_json().await.unwrap();
    assert!(created["id"].as_str().unwrap().starts_with("resource_"));
    assert!(created.get("presetId").is_none());
    assert!(created.get("seatId").is_none());
    assert!(
        created["roles"]
            .as_array()
            .unwrap()
            .contains(&json!("coding"))
    );
    assert!(
        !created["roles"]
            .as_array()
            .unwrap()
            .contains(&json!("architect"))
    );
    let mut listed = TestClient::get(format!("{BASE}/api/native/v1/resources"))
        .add_header("host", "127.0.0.1:13300", true)
        .bearer_auth(TOKEN)
        .send(&service)
        .await;
    assert_eq!(listed.take_json::<Value>().await.unwrap(), json!([created]));
    let unpublished = TestClient::post(format!("{BASE}/api/native/v1/roles/coding/publication"))
        .add_header("host", "127.0.0.1:13300", true)
        .bearer_auth(TOKEN)
        .json(&json!({"published":false}))
        .send(&service)
        .await;
    assert_eq!(unpublished.status_code, Some(StatusCode::OK));
    let mut listed = TestClient::get(format!("{BASE}/api/native/v1/resources"))
        .add_header("host", "127.0.0.1:13300", true)
        .bearer_auth(TOKEN)
        .send(&service)
        .await;
    assert!(
        !listed.take_json::<Value>().await.unwrap()[0]["roles"]
            .as_array()
            .unwrap()
            .contains(&json!("coding"))
    );
    TestClient::post(format!("{BASE}/api/native/v1/roles/coding/publication"))
        .add_header("host", "127.0.0.1:13300", true)
        .bearer_auth(TOKEN)
        .json(&json!({"published":true}))
        .send(&service)
        .await;
    let mut budget = TestClient::get(format!(
        "{BASE}/api/native/v1/resources/{}/budget",
        created["id"].as_str().unwrap()
    ))
    .add_header("host", "127.0.0.1:13300", true)
    .bearer_auth(TOKEN)
    .send(&service)
    .await;
    let budget: Value = budget.take_json().await.unwrap();
    assert_eq!(budget["remainingTokens"], 100);
    assert!(budget["seat"]["remaining"].is_null());
    let mut withdrawn = resource.clone();
    withdrawn["published"] = json!(false);
    assert_eq!(
        TestClient::post(format!("{BASE}/api/native/v1/resources"))
            .add_header("host", "127.0.0.1:13300", true)
            .bearer_auth(TOKEN)
            .json(&withdrawn)
            .send(&service)
            .await
            .status_code,
        Some(StatusCode::OK)
    );
    let mut listed = TestClient::get(format!("{BASE}/api/native/v1/resources"))
        .add_header("host", "127.0.0.1:13300", true)
        .bearer_auth(TOKEN)
        .send(&service)
        .await;
    assert_eq!(listed.take_json::<Value>().await.unwrap(), json!([]));
    // A normal edit that omits publication must retain an explicit withdrawal.
    assert_eq!(
        TestClient::post(format!("{BASE}/api/native/v1/resources"))
            .add_header("host", "127.0.0.1:13300", true)
            .bearer_auth(TOKEN)
            .json(&resource)
            .send(&service)
            .await
            .status_code,
        Some(StatusCode::OK)
    );
    let mut configured = TestClient::get(format!("{BASE}/api/native/v1/resource-configurations"))
        .add_header("host", "127.0.0.1:13300", true)
        .bearer_auth(TOKEN)
        .send(&service)
        .await;
    let configured: Value = configured.take_json().await.unwrap();
    assert_eq!(configured[0]["config"]["published"], false);
    assert_eq!(configured[0]["config"]["presetId"], "private_preset");
    let denied = TestClient::get(format!("{BASE}/api/native/v1/resource-configurations"))
        .add_header("host", "127.0.0.1:13300", true)
        .send(&service)
        .await;
    assert_eq!(denied.status_code, Some(StatusCode::UNAUTHORIZED));
    let mut republished = resource.clone();
    republished["published"] = json!(true);
    TestClient::post(format!("{BASE}/api/native/v1/resources"))
        .add_header("host", "127.0.0.1:13300", true)
        .bearer_auth(TOKEN)
        .json(&republished)
        .send(&service)
        .await;
    let mut listed = TestClient::get(format!("{BASE}/api/native/v1/resources"))
        .add_header("host", "127.0.0.1:13300", true)
        .bearer_auth(TOKEN)
        .send(&service)
        .await;
    assert_eq!(
        listed
            .take_json::<Value>()
            .await
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        1
    );
    for endpoint in ["requests", "engagements/approve", "effects/complete"] {
        assert_eq!(
            TestClient::post(format!("{BASE}/api/native/v1/{endpoint}"))
                .add_header("host", "127.0.0.1:13300", true)
                .bearer_auth(TOKEN)
                .json(&json!({"ownerVerified":true}))
                .send(&service)
                .await
                .status_code,
            Some(StatusCode::NOT_FOUND)
        );
    }
    let invalid = TestClient::get(format!("{BASE}/api/native/v1/resources?limit=100000"))
        .add_header("host", "127.0.0.1:13300", true)
        .bearer_auth(TOKEN)
        .send(&service)
        .await;
    assert_eq!(invalid.status_code, Some(StatusCode::BAD_REQUEST));
    domain.shutdown().await.unwrap();
    custody.shutdown().await.unwrap();
}
fn body() -> Value {
    json!({"binding":"fixture", "generation":1, "id":"request_1", "lane":"work", "kind":"request", "payload":{"name":"小白"}})
}
fn setup() -> (tempfile::TempDir, Store, Service) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::start(Repository::open(&dir.path().join("state")).unwrap(), 16).unwrap();
    let app = App::new(
        store.clone(),
        TOKEN.as_bytes(),
        "127.0.0.1:13300".parse().unwrap(),
    )
    .unwrap();
    (dir, store, Service::new(app.router()))
}
#[tokio::test]
async fn http_auth_and_limits() {
    let (_dir, store, service) = setup();
    for token in ["", "wrong"] {
        let response = TestClient::post(format!("{BASE}/api/native/v1/custody"))
            .add_header("host", "127.0.0.1:13300", true)
            .bearer_auth(token)
            .json(&body())
            .send(&service)
            .await;
        assert_eq!(response.status_code, Some(StatusCode::UNAUTHORIZED));
    }
    for (header, value) in [
        ("origin", "https://attacker.test"),
        ("sec-fetch-site", "same-origin"),
        ("x-forwarded-for", "127.0.0.1"),
        ("host", "attacker.test:13300"),
    ] {
        let response = TestClient::post(format!("{BASE}/api/native/v1/custody"))
            .add_header("host", "127.0.0.1:13300", true)
            .bearer_auth(TOKEN)
            .add_header(header, value, true)
            .json(&body())
            .send(&service)
            .await;
        assert_eq!(response.status_code, Some(StatusCode::FORBIDDEN));
    }
    let mut accepted = TestClient::post(format!("{BASE}/api/native/v1/custody"))
        .add_header("host", "127.0.0.1:13300", true)
        .bearer_auth(TOKEN)
        .json(&body())
        .send(&service)
        .await;
    assert_eq!(accepted.status_code, Some(StatusCode::ACCEPTED));
    assert_eq!(accepted.headers().get("cache-control").unwrap(), "no-store");
    let receipt: Value = accepted.take_json().await.unwrap();
    assert_eq!(receipt["state"], "received");
    assert!(receipt.get("payload").is_none());
    let mut changed = body();
    changed["payload"]["name"] = json!("other");
    let conflict = TestClient::post(format!("{BASE}/api/native/v1/custody"))
        .add_header("host", "127.0.0.1:13300", true)
        .bearer_auth(TOKEN)
        .json(&changed)
        .send(&service)
        .await;
    assert_eq!(conflict.status_code, Some(StatusCode::CONFLICT));
    let huge = "x".repeat(hagency_core::custody::MAX_DELIVERY_BYTES + 1);
    let response = TestClient::post(format!("{BASE}/api/native/v1/custody"))
        .add_header("host", "127.0.0.1:13300", true)
        .bearer_auth(TOKEN)
        .raw_json(huge)
        .send(&service)
        .await;
    assert_eq!(response.status_code, Some(StatusCode::PAYLOAD_TOO_LARGE));
    store.shutdown().await.unwrap();
}
#[tokio::test]
async fn unimplemented_capabilities_are_explicit() {
    let (_dir, store, service) = setup();
    let mut response = TestClient::get(format!("{BASE}/api/native/v1/capabilities"))
        .add_header("host", "127.0.0.1:13300", true)
        .bearer_auth(TOKEN)
        .send(&service)
        .await;
    assert_eq!(response.status_code, Some(StatusCode::OK));
    let value: Value = response.take_json().await.unwrap();
    for key in [
        "agent_execution",
        "palpo_transport",
        "matrix_crypto",
        "production_api_parity",
    ] {
        assert_eq!(value[key], false);
    }
    let response = TestClient::post(format!("{BASE}/api/engagements/approve"))
        .add_header("host", "127.0.0.1:13300", true)
        .bearer_auth(TOKEN)
        .json(&body())
        .send(&service)
        .await;
    assert_eq!(response.status_code, Some(StatusCode::NOT_FOUND));
    store.shutdown().await.unwrap();
}
#[tokio::test(flavor = "current_thread")]
async fn bounded_work_keeps_health_responsive() {
    // Holding the actual SQLite writer lock simulates a slow external filesystem.
    // The single Tokio thread must remain responsive while the dedicated DB thread waits.
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    let repository = Repository::open(&state).unwrap();
    let store = Store::start(repository, 1).unwrap();
    let blocker = rusqlite::Connection::open(state.join("custody.sqlite3")).unwrap();
    blocker.execute_batch("BEGIN IMMEDIATE").unwrap();
    let service = Arc::new(Service::new(
        App::new(
            store.clone(),
            TOKEN.as_bytes(),
            "127.0.0.1:13300".parse().unwrap(),
        )
        .unwrap()
        .router(),
    ));
    let mut jobs = tokio::task::JoinSet::new();
    // Multiple bounded inputs exercise admission while the real writer is locked.
    for index in 0..24 {
        let service = service.clone();
        jobs.spawn(async move {
            let mut input = body();
            input["id"] = json!(format!("request_{index}"));
            input["payload"]["discussion"] = json!("x".repeat(16 * 1024));
            TestClient::post(format!("{BASE}/api/native/v1/custody"))
                .add_header("host", "127.0.0.1:13300", true)
                .bearer_auth(TOKEN)
                .json(&input)
                .send(&*service)
                .await
                .status_code
        });
    }
    tokio::task::yield_now().await;
    let response = tokio::time::timeout(
        Duration::from_millis(500),
        TestClient::get(format!("{BASE}/health")).send(&*service),
    )
    .await
    .expect("health blocked by custody work");
    assert_eq!(response.status_code, Some(StatusCode::OK));
    blocker.execute_batch("ROLLBACK").unwrap();
    let mut accepted = 0;
    let mut busy = 0;
    while let Some(result) = jobs.join_next().await {
        match result.unwrap() {
            Some(StatusCode::ACCEPTED) => accepted += 1,
            Some(StatusCode::SERVICE_UNAVAILABLE) => busy += 1,
            other => panic!("unexpected request outcome: {other:?}"),
        }
    }
    assert!(accepted > 0);
    assert!(busy > 0);
    store.shutdown().await.unwrap();
}
