use super::*;
use hagency_store::resource_publication_revision;

async fn management(service: &Service) -> String {
    let mut response = TestClient::post(format!(
        "{BASE}/api/native/v1/console/resource-publication-access"
    ))
    .add_header("host", "127.0.0.1:13300", true)
    .bearer_auth(TOKEN)
    .send(service)
    .await;
    assert_eq!(response.status_code, Some(StatusCode::OK));
    let ticket = response.take_json::<Value>().await.unwrap()["ticket"]
        .as_str()
        .unwrap()
        .to_owned();
    let response = exchange(service, &ticket).await;
    assert_eq!(response.status_code, Some(StatusCode::OK));
    response
        .headers()
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned()
}
fn command(path: &str, cookie: &str) -> salvo::test::RequestBuilder {
    TestClient::post(format!("{BASE}{path}"))
        .add_header("host", "127.0.0.1:13300", true)
        .add_header("origin", BASE, true)
        .add_header("sec-fetch-site", "same-origin", true)
        .add_header("cookie", cookie, true)
}
fn logout(cookie: &str) -> salvo::test::RequestBuilder {
    TestClient::delete(format!("{BASE}/console/session"))
        .add_header("host", "127.0.0.1:13300", true)
        .add_header("origin", BASE, true)
        .add_header("sec-fetch-site", "same-origin", true)
        .add_header("cookie", cookie, true)
}
#[tokio::test]
async fn native_console_resource_authority() {
    let f = Fixture::new("127.0.0.1:13300".parse().unwrap(), None);
    let resource = native_resource("private_resource_pool");
    f.domain.put_resource(resource.clone()).await.unwrap();
    let service = f.service();
    let readonly = session(&service).await;
    let path = format!("/console/api/resources/{}/publication", resource.id());
    let body = json!({"expectedRevision":resource_publication_revision(&resource).unwrap(),"published":false});
    assert_eq!(
        command(&path, &readonly)
            .json(&body)
            .send(&service)
            .await
            .status_code,
        Some(StatusCode::FORBIDDEN)
    );
    // The issuer shares its existing rate budget across both fixed scopes.
    assert_eq!(
        TestClient::post(format!(
            "{BASE}/api/native/v1/console/resource-publication-access"
        ))
        .add_header("host", "127.0.0.1:13300", true)
        .bearer_auth(TOKEN)
        .send(&service)
        .await
        .status_code,
        Some(StatusCode::TOO_MANY_REQUESTS)
    );
    tokio::time::sleep(std::time::Duration::from_millis(1010)).await;
    let manager = management(&service).await;
    let bad =
        json!({"expectedRevision":body["expectedRevision"],"published":false,"scope":"operator"});
    assert_eq!(
        command(&path, &manager)
            .json(&bad)
            .send(&service)
            .await
            .status_code,
        Some(StatusCode::BAD_REQUEST)
    );
    let lock = rusqlite::Connection::open(f.root.path().join("state/domain.sqlite3")).unwrap();
    lock.execute_batch("BEGIN IMMEDIATE").unwrap();
    let mut operation = Box::pin(command(&path, &manager).json(&body).send(&service));
    assert!(
        std::future::poll_fn(|cx| std::task::Poll::Ready(operation.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    let mut busy = logout(&manager).send(&service).await;
    assert_eq!(busy.status_code, Some(StatusCode::TOO_MANY_REQUESTS));
    assert!(!busy.headers().contains_key("set-cookie"));
    assert_eq!(
        busy.take_json::<Value>().await.unwrap()["code"],
        "console_busy"
    );
    // Unrelated session authority still succeeds without waiting on SQLite.
    assert_eq!(
        logout(&readonly).send(&service).await.status_code,
        Some(StatusCode::OK)
    );
    lock.execute_batch("COMMIT").unwrap();
    assert_eq!(operation.await.status_code, Some(StatusCode::OK));
    assert_eq!(
        command(&path, &manager)
            .json(&body)
            .send(&service)
            .await
            .status_code,
        Some(StatusCode::CONFLICT)
    );
    assert_eq!(
        logout(&manager).send(&service).await.status_code,
        Some(StatusCode::OK)
    );
    assert_eq!(
        command(&path, &manager)
            .json(&body)
            .send(&service)
            .await
            .status_code,
        Some(StatusCode::UNAUTHORIZED)
    );
    f.close().await;
}
#[tokio::test]
async fn native_console_resource_observations() {
    let f = Fixture::new("127.0.0.1:13300".parse().unwrap(), None);
    let mut resource = native_resource("private_resource_missing");
    resource.ceiling = None;
    resource.published = false;
    f.domain.put_resource(resource.clone()).await.unwrap();
    let service = f.service();
    let cookie = session(&service).await;
    let mut response = get("/console/api/resources?limit=16", &cookie)
        .send(&service)
        .await;
    assert_eq!(response.status_code, Some(StatusCode::OK));
    let rows = response.take_json::<Value>().await.unwrap();
    assert_private(&rows);
    let text = rows.to_string();
    for private in [
        "presetId",
        "seatId",
        "private_resource_account",
        "authHome",
        "\"config\"",
    ] {
        assert!(!text.contains(private));
    }
    assert_eq!(rows["permissions"]["publishResource"], false);
    assert_eq!(rows["roles"].as_array().unwrap().len(), 6);
    // G5: the roles table keeps the EIGHT-key set exactly (the client's
    // exact-key conjunction at native-api.js:10-11 applied to roles) — the
    // three derived keys are present on every row and no ninth key is.
    let keys = [
        "role",
        "explicitPublication",
        "available",
        "crossFamily",
        "defaultTier",
        "families",
        "fillable",
        "overTier",
    ];
    for role in rows["roles"].as_array().unwrap() {
        let object = role.as_object().unwrap();
        assert_eq!(
            object.len(),
            keys.len(),
            "exactly eight keys on the role row"
        );
        for key in keys {
            assert!(object.contains_key(key), "the role row carries {key}");
        }
        assert!(
            role["families"]
                .as_array()
                .unwrap()
                .iter()
                .all(|f| f.is_string()),
            "families is an array of strings"
        );
        assert!(role["fillable"].as_u64().is_some());
        assert!(role["overTier"].as_u64().is_some());
    }
    let row = rows["resources"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == resource.id())
        .unwrap();
    assert_eq!(row["published"], false);
    assert!(row["ceiling"].is_null());
    let path = format!("/console/api/resources/{}/budget", resource.id());
    let mut response = get(&path, &cookie).send(&service).await;
    let budget = response.take_json::<Value>().await.unwrap();
    assert!(budget["pool"]["ceiling"].is_null());
    assert!(budget["pool"]["remaining"].is_null());
    assert_eq!(budget["seat"]["status"], "undeclared");
    assert!(budget["seat"]["quota"].is_null());
    // Brief 18 — the `draw` object's UNKNOWN arm: no ceiling, no engagement,
    // no measurement. Every unknown figure is null, never zero, and nothing
    // competes for a ceiling that does not exist.
    let draw = &budget["draw"];
    assert_eq!(draw["committed"], 0, "no engagements, no commitment");
    assert!(draw["measured"].is_null(), "unmeasured is null, not zero");
    assert!(draw["consumed"].is_null());
    assert_eq!(draw["drawn"], 0);
    assert!(draw["binding"].is_null(), "nothing competes, nothing binds");
    assert!(draw["ceilingTokens"].is_null());
    assert!(draw["remainingBeforeCeiling"].is_null());
    assert_eq!(draw["period"], "monthly", "monthly unless declared daily");
    // The MEASURED arm: the fixture's usage pool carries a real bound usage
    // source with observations, so measured/consumed are known figures. The
    // assertions are self-consistent rather than brittle constants: drawn is
    // exactly max(committed, measured), the binding draw is named the way
    // ADR-122's refusal names it (engagement-store.js:82-83), and the
    // remaining figure is the ceiling minus the draw.
    let usage_pool = common::resource("private_usage_pool", "private_usage_seat", 1000);
    let mut response = get(
        &format!("/console/api/resources/{}/budget", usage_pool.id()),
        &cookie,
    )
    .send(&service)
    .await;
    assert_eq!(response.status_code, Some(StatusCode::OK));
    let value = response.take_json::<Value>().await.unwrap();
    let draw = &value["draw"];
    let committed = draw["committed"].as_u64().unwrap();
    let measured = draw["measured"]
        .as_u64()
        .expect("the seeded observation measures the current period");
    let consumed = draw["consumed"].as_u64().unwrap();
    assert!(consumed >= measured, "display total covers the fresh draw");
    assert_eq!(draw["ceilingTokens"], 1000);
    let drawn = draw["drawn"].as_u64().unwrap();
    assert_eq!(
        drawn,
        committed.max(measured),
        "the draw is the larger figure"
    );
    assert_eq!(
        draw["binding"],
        if measured > committed {
            "measured spend"
        } else {
            "committed allocations"
        },
        "the binding draw is named like the over-commit refusal"
    );
    assert_eq!(draw["remainingBeforeCeiling"], 1000 - drawn);
    assert_eq!(draw["period"], "monthly");
    // Brief 20 (E1): the two committed predicates are DIFFERENT SQL and
    // agree by an invariant, not by coincidence — pin it with a seat shared
    // across two presets. `pool.committed` folds `preset_id=?` grouped;
    // `draw.committed` folds bare `resource_id=?`; they select the same
    // engagement set only because `resource_id = public_resource_id(preset_id)`
    // is injective (`project.rs:67-69`). `seat.committed` (`preset_id=? OR
    // seat_id=?`) is deliberately the LARGER cross-preset figure. With a
    // second preset on the SAME seat holding an approved 250-token
    // engagement: the pool's own committed figures stay 100 (both
    // predicates agree), while the seat figure is 350.
    let shared_seat_pool = common::resource("private_shared_seat_pool", "private_usage_seat", 1000);
    f.domain
        .put_resource(shared_seat_pool.clone())
        .await
        .unwrap();
    let shared_request = common::request(
        "shared_seat_request",
        "SharedSeatWorker",
        &shared_seat_pool,
        250,
    );
    // The async store takes proofs BY VALUE (domain_worker.rs:2623/:2635);
    // a second `common::proof` of the same request yields the same id and
    // digest, so admit and approve address the same engagement.
    f.domain
        .admit(common::proof(&shared_request), 1000)
        .await
        .unwrap();
    f.domain
        .approve(
            "approve_shared_seat".to_owned(),
            common::proof(&shared_request),
            1000,
        )
        .await
        .unwrap();
    let mut response = get(
        &format!("/console/api/resources/{}/budget", usage_pool.id()),
        &cookie,
    )
    .send(&service)
    .await;
    assert_eq!(response.status_code, Some(StatusCode::OK));
    let value = response.take_json::<Value>().await.unwrap();
    assert_eq!(
        value["draw"]["committed"], value["pool"]["committed"],
        "the two committed predicates agree across a shared seat"
    );
    assert_eq!(
        value["draw"]["committed"].as_u64().unwrap(),
        100,
        "the pool's own draw is untouched by the other preset's engagement"
    );
    assert_eq!(
        value["seat"]["committed"].as_u64().unwrap(),
        350,
        "the seat figure is deliberately the cross-preset roll-up"
    );
    // And the mirror read on the shared-seat resource itself.
    let mut response = get(
        &format!("/console/api/resources/{}/budget", shared_seat_pool.id()),
        &cookie,
    )
    .send(&service)
    .await;
    assert_eq!(response.status_code, Some(StatusCode::OK));
    let value = response.take_json::<Value>().await.unwrap();
    assert_eq!(value["draw"]["committed"], 250);
    assert_eq!(value["pool"]["committed"], 250);
    assert_eq!(value["seat"]["committed"].as_u64().unwrap(), 350);
    for query in ["limit=17", "limit=0", "limit=1&limit=2", "unknown=1"] {
        assert_eq!(
            get(&format!("/console/api/resources?{query}"), &cookie)
                .send(&service)
                .await
                .status_code,
            Some(StatusCode::BAD_REQUEST)
        );
    }

    let mut bounded_resource = native_resource("private_partial_pool");
    bounded_resource.ceiling = Some(
        serde_json::from_value(
            json!({"tokens":null,"period":"a_native_period_longer_than_thirty_two_bytes"}),
        )
        .unwrap(),
    );
    f.domain
        .put_resource(bounded_resource.clone())
        .await
        .unwrap();
    f.domain.put_seat(serde_json::from_value(json!({"id":"private_resource_account","declaration":{"quotaTokens":8000,"period":"monthly"}})).unwrap()).await.unwrap();
    let mut partial = get(
        &format!("/console/api/resources/{}/budget", bounded_resource.id()),
        &cookie,
    )
    .send(&service)
    .await;
    let partial = partial.take_json::<Value>().await.unwrap();
    assert!(partial["pool"]["ceiling"].is_null());
    assert_eq!(
        partial["pool"]["period"],
        "a_native_period_longer_than_thirty_two_bytes"
    );
    assert_eq!(partial["seat"]["quota"], 8000);
    assert_eq!(partial["seat"]["status"], "period_mismatch");
    let mut page = get("/console/api/resources?limit=1", &cookie)
        .send(&service)
        .await;
    let page = page.take_json::<Value>().await.unwrap();
    assert_eq!(page["resources"].as_array().unwrap().len(), 1);
    assert!(page["next_after"].is_string());
    bounded_resource.ceiling =
        Some(serde_json::from_value(json!({"tokens":null,"period":"x".repeat(40000)})).unwrap());
    f.domain
        .put_resource(bounded_resource.clone())
        .await
        .unwrap();
    bounded_resource.preset_id = "private_second_large_period".into();
    f.domain.put_resource(bounded_resource).await.unwrap();
    assert_eq!(
        get("/console/api/resources?limit=16", &cookie)
            .send(&service)
            .await
            .status_code,
        Some(StatusCode::SERVICE_UNAVAILABLE)
    );
    let lock = rusqlite::Connection::open(f.root.path().join("state/domain.sqlite3")).unwrap();
    lock.execute_batch("BEGIN IMMEDIATE").unwrap();
    let mut ahead = Box::pin(f.domain.put_resource(resource));
    assert!(
        std::future::poll_fn(|cx| std::task::Poll::Ready(ahead.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    let mut waiting = Box::pin(get(&path, &cookie).send(&service));
    assert!(
        std::future::poll_fn(|cx| std::task::Poll::Ready(waiting.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    f.console.retire();
    lock.execute_batch("COMMIT").unwrap();
    ahead.await.unwrap();
    assert_eq!(
        waiting.await.status_code,
        Some(StatusCode::SERVICE_UNAVAILABLE)
    );
    f.close().await;
}

/// G5: every catalogue value comes from ONE predicate and the model
/// family. Three extra resources join the fixture's two gpt-medium pools:
/// an octos/kimi-k3 resource (a REAL model family, but non-provisionable —
/// `provisionable()` is claude|codex only), and a codex resource whose
/// model matches no policy tier (`model() == (None, None)` — it cannot
/// qualify at all, so it adds no family and is never over-tier). The
/// derivations must ignore both, count families as MODEL families (never
/// the framework), and change when a resource is withdrawn — a hardcoded
/// or client-side recomputation cannot pass the second read.
#[tokio::test]
async fn native_console_catalogue_fillability_is_derived() {
    let f = Fixture::new("127.0.0.1:13300".parse().unwrap(), None);
    let kimi = serde_json::from_value::<hagency_core::project::Resource>(json!({
        "presetId":"catalogue_kimi","seatId":"catalogue_kimi_seat","framework":"octos",
        "model":"kimi-k3","ceiling":{"tokens":1000,"period":"monthly"}}))
    .unwrap();
    let mystery = serde_json::from_value::<hagency_core::project::Resource>(json!({
        "presetId":"catalogue_mystery","seatId":"catalogue_mystery_seat","framework":"codex",
        "model":"mystery_model_v9","ceiling":{"tokens":1000,"period":"monthly"}}))
    .unwrap();
    f.domain.put_resource(kimi).await.unwrap();
    f.domain.put_resource(mystery).await.unwrap();
    let service = f.service();
    let cookie = session(&service).await;
    let roles = |value: &Value| {
        value["roles"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| {
                (
                    r["role"].as_str().unwrap().to_owned(),
                    r["fillable"].as_u64().unwrap(),
                    r["overTier"].as_u64().unwrap(),
                    r["families"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|f| f.as_str().unwrap().to_owned())
                        .collect::<Vec<_>>(),
                    r["available"].as_bool().unwrap(),
                )
            })
            .collect::<Vec<_>>()
    };
    let read = || async {
        let mut response = get("/console/api/resources?limit=16", &cookie)
            .send(&service)
            .await;
        assert_eq!(response.status_code, Some(StatusCode::OK));
        response.take_json::<Value>().await.unwrap()
    };
    let value = read().await;
    for (role, fillable, over_tier, families, available) in roles(&value) {
        // The fixture's two gpt pools carry reasoning "medium" — the
        // policy's MEDIUM row, NOT strong. They qualify exactly the
        // medium/lightweight-default roles; the strong-default roles
        // (architect, review) correctly report fillable 0 with no family.
        // kimi is non-provisionable and the mystery model has no tier, so
        // neither counts toward any role.
        let (expect_fillable, expect_over) = match role.as_str() {
            "architect" | "review" => (0, 0),
            "documentation" => (2, 2),
            _ => (2, 0), // coding/testing/integration: medium is not above medium
        };
        assert_eq!(
            fillable, expect_fillable,
            "{role}: only the tiered, provisionable resources count"
        );
        if expect_fillable > 0 {
            assert_eq!(
                families,
                ["gpt"],
                "{role}: the MODEL family, never the framework"
            );
        } else {
            assert!(
                families.is_empty(),
                "{role}: no qualifying resource, no family"
            );
        }
        assert!(
            !families.iter().any(|f| f == "codex" || f == "octos"),
            "{role}: a framework name is never a family"
        );
        assert_eq!(over_tier, expect_over, "{role}: strictly stronger only");
        let expect_available = if role == "review" {
            false // cross-family: needs two families among ACTIVE engagements; none seeded here
        } else {
            expect_fillable > 0
        };
        assert_eq!(
            available, expect_available,
            "{role}: available agrees with the predicate"
        );
    }
    // The negative arm: withdraw BOTH gpt pools through the store and the
    // counts must fall — fillable 0 with the key still present, families
    // empty, nothing over-tier, and available now false. No client-side
    // or hardcoded value survives this read. Each withdrawal must carry
    // the row's OWN seat and ceiling (prepare_resource_write treats
    // seat/framework/model/provider changes specially).
    for (preset, seat, tokens) in [
        ("private_usage_pool", "private_usage_seat", 1000),
        ("private_alert_pool", "private_alert_seat", 50),
    ] {
        f.domain
            .edit_resource(common::resource(preset, seat, tokens), Some(false))
            .await
            .unwrap();
    }
    let value = read().await;
    for (role, fillable, over_tier, families, available) in roles(&value) {
        assert_eq!(fillable, 0, "{role}: zero is served, never an omitted key");
        assert!(
            families.is_empty(),
            "{role}: no qualifying resource, no family"
        );
        assert_eq!(over_tier, 0);
        assert!(!available, "{role}: fillable 0 exactly as available false");
    }
    f.close().await;
}

/// G5: the catalogue omits every profile field native does not persist.
/// The five forbidden keys are asserted over the RAW body bytes, not only
/// the parsed key set — and the forward guard proves fail-closed: a
/// key-shaped value seeded straight into the stored config makes the read
/// REFUSE (deny_unknown_fields → schema error → 503) rather than serve it.
#[tokio::test]
async fn native_console_catalogue_omits_unpersisted_profile_fields() {
    let f = Fixture::new("127.0.0.1:13300".parse().unwrap(), None);
    let service = f.service();
    let cookie = session(&service).await;
    let mut response = get("/console/api/resources?limit=16", &cookie)
        .send(&service)
        .await;
    assert_eq!(response.status_code, Some(StatusCode::OK));
    let body = response.take_string().await.unwrap();
    for key in [
        "\"name\"",
        "rateCapPerDay",
        "apiBaseUrl",
        "apiKeySet",
        "extraArgs",
    ] {
        assert!(!body.contains(key), "the raw body carries no {key} key");
    }
    // The forward guard: seed a stored key value where none may exist.
    let secret = "sk_live_guard_7c1d3fa2e9b4";
    let pool = common::resource("private_usage_pool", "private_usage_seat", 1000);
    let raw = rusqlite::Connection::open(f.root.path().join("state/domain.sqlite3")).unwrap();
    raw.execute(
        "UPDATE resources SET config=json_set(config,'$.apiKeySet',?1) WHERE json_extract(config,'$.presetId')=?2",
        rusqlite::params![secret, pool.preset_id],
    )
    .unwrap();
    drop(raw);
    let mut response = get("/console/api/resources?limit=16", &cookie)
        .send(&service)
        .await;
    assert_eq!(
        response.status_code,
        Some(StatusCode::SERVICE_UNAVAILABLE),
        "an unpersisted key is a schema fault, never a served column"
    );
    let refused = response.take_string().await.unwrap();
    assert!(
        !refused.contains(secret),
        "the stored key value reaches no byte"
    );
    assert!(!refused.contains("apiKeySet"));
    f.close().await;
}
