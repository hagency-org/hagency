#[path = "usage/fixture.rs"]
mod fixture;
use fixture::*;
use salvo::{
    prelude::*,
    test::{ResponseExt, TestClient},
};
use serde_json::{Value, json};
use std::time::{SystemTime, UNIX_EPOCH};

async fn read(f: &Fixture, query: &str) -> Value {
    let mut response = TestClient::get(format!("{}{}", f.url(), query))
        .add_header("host", "127.0.0.1:13300", true)
        .bearer_auth(TOKEN)
        .send(&f.service)
        .await;
    assert_eq!(response.status_code, Some(StatusCode::OK));
    assert_eq!(response.headers().get("cache-control").unwrap(), "no-store");
    let value = response.take_json::<Value>().await.unwrap();
    let fields: Vec<_> = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        fields,
        [
            "at_ms",
            "ceiling",
            "daily",
            "engagement_id",
            "monthly",
            "summary"
        ]
    );
    let text = value.to_string();
    for private in [
        "private_session",
        "private_task",
        "private_dispatch",
        "private_runner",
        "private_workspace",
        "private_thread",
        "source_id",
        "snapshot_digest",
        "source_digest",
        "ownerDmRoomId",
        "!private:example.test",
        TOKEN,
    ] {
        assert!(
            !text.contains(private),
            "unexpected private field {private}"
        );
    }
    value
}

#[tokio::test]
async fn native_usage_api_authority() {
    let f = Fixture::new(&[(&snapshot(7, 2, 3), 2000)], true, true);
    for token in [None, Some("incorrect")] {
        let request = TestClient::get(f.url()).add_header("host", "127.0.0.1:13300", true);
        let request = match token {
            Some(token) => request.bearer_auth(token),
            None => request,
        };
        let mut response = request.send(&f.service).await;
        assert_eq!(response.status_code, Some(StatusCode::UNAUTHORIZED));
        assert_eq!(
            response.take_json::<Value>().await.unwrap(),
            json!({"ok":false,"code":"operator_auth_required"})
        );
    }
    for (header, value) in [
        ("origin", "https://untrusted.test"),
        ("x-forwarded-for", "127.0.0.1"),
        ("host", "untrusted.test"),
    ] {
        let mut response = TestClient::get(f.url())
            .add_header("host", "127.0.0.1:13300", true)
            .add_header(header, value, true)
            .bearer_auth(TOKEN)
            .send(&f.service)
            .await;
        assert_eq!(response.status_code, Some(StatusCode::FORBIDDEN));
        assert_eq!(
            response.take_json::<Value>().await.unwrap(),
            json!({"ok":false,"code":"local_authority_required"})
        );
    }
    for path in [
        f.url(),
        format!("{}/sources", f.url()),
        format!("{}/observations", f.url()),
    ] {
        let response = TestClient::post(path)
            .add_header("host", "127.0.0.1:13300", true)
            .bearer_auth(TOKEN)
            .json(&json!({"input":999,"verified":true}))
            .send(&f.service)
            .await;
        assert!(matches!(
            response.status_code,
            Some(StatusCode::METHOD_NOT_ALLOWED | StatusCode::NOT_FOUND)
        ));
    }
    let value = read(&f, "?at_ms=2000").await;
    assert_eq!(value["summary"]["latest_counts"]["input"], 7);
    let mut capabilities = TestClient::get(format!("{BASE}/api/native/v1/capabilities"))
        .add_header("host", "127.0.0.1:13300", true)
        .bearer_auth(TOKEN)
        .send(&f.service)
        .await;
    let caps = capabilities.take_json::<Value>().await.unwrap();
    assert_eq!(caps["usage_observations_read"], true);
    for name in [
        "agent_execution",
        "matrix_crypto",
        "palpo_transport",
        "production_api_parity",
    ] {
        assert_eq!(caps[name], false);
    }
    f.close().await;
}

#[tokio::test]
async fn native_usage_api_projection() {
    for source in [false, true] {
        let f = Fixture::new(&[], source, true);
        let report = read(&f, "?at_ms=2000").await;
        assert_eq!(report["summary"]["sources"], u64::from(source));
        assert!(report["daily"].is_null() && report["monthly"].is_null());
        if source {
            assert_eq!(report["summary"]["latest_incomplete_sources"], 1);
            assert!(report["summary"]["latest_counts"]["input"].is_null());
        } else {
            assert!(report["summary"]["latest_counts"].is_null());
            assert!(report["summary"]["known_high_water_lower_bound"].is_null());
        }
        f.close().await;
    }
    let high = snapshot(7, 2, 3);
    let low = snapshot(4, 1, 1);
    let later = snapshot(8, 2, 3);
    for complete in [false, true] {
        let mut records = vec![(high.as_str(), 2000), ("", 2100), (low.as_str(), 2200)];
        if complete {
            records.push((later.as_str(), 2300));
        }
        let f = Fixture::new(&records, true, true);
        let report = read(&f, "?at_ms=2000").await;
        let summary = &report["summary"];
        assert_eq!(
            summary["latest_counts"]["input"],
            if complete { 8 } else { 4 }
        );
        assert_eq!(
            summary["known_high_water_lower_bound"]["input"],
            if complete { 8 } else { 7 }
        );
        assert_eq!(summary["latest_incomplete_sources"], u64::from(!complete));
        assert_eq!(summary["historically_incomplete_sources"], 1);
        assert_eq!(summary["regression_observations"], 1);
        assert_eq!(summary["evidence"], "host_attributed_untrusted_usage");
        assert_eq!(report["daily"]["key"], "1970-01-01");
        assert_eq!(report["monthly"]["key"], "1970-01");
        assert_eq!(report["daily"]["incomplete"], true);
        assert!(report["daily"]["observed_growth"]["input"].is_null());
        assert_eq!(
            report["daily"]["known_growth_lower_bound"]["input"],
            if complete { 8 } else { 7 }
        );
        let absent = read(&f, "?at_ms=2678400000").await;
        assert!(absent["daily"].is_null() && absent["monthly"].is_null());
        let before = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let current = read(&f, "").await;
        let after = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        assert!((before..=after).contains(&current["at_ms"].as_u64().unwrap()));
        assert_eq!(current["summary"], *summary);
        f.close().await;
    }
}

#[tokio::test]
async fn native_usage_api_refusals() {
    let f = Fixture::new(&[], false, true);
    for at in [0_u64, 253402300799999] {
        let value = read(&f, &format!("?at_ms={at}")).await;
        assert_eq!(value["at_ms"], at);
        assert!(value["daily"].is_null() && value["monthly"].is_null());
    }
    let oversized = format!("?at_ms={}", "0".repeat(129));
    for query in [
        "?at_ms=",
        "?at_ms=-1",
        "?at_ms=1.5",
        "?at_ms=2&at_ms=2",
        "?at_ms=2&%61t_ms=3",
        "?unknown=2",
        "?at_ms=2&unknown=3",
        "?at_ms=18446744073709551616",
        "?at_ms=253402300800000",
        oversized.as_str(),
    ] {
        let mut response = TestClient::get(format!("{}{}", f.url(), query))
            .add_header("host", "127.0.0.1:13300", true)
            .bearer_auth(TOKEN)
            .send(&f.service)
            .await;
        assert_eq!(
            response.status_code,
            Some(StatusCode::BAD_REQUEST),
            "query {query}"
        );
        assert_eq!(
            response.take_json::<Value>().await.unwrap(),
            json!({"ok":false,"code":"invalid_usage_query"})
        );
    }
    let mut missing = TestClient::get(format!(
        "{BASE}/api/native/v1/engagements/nonexistent/usage"
    ))
    .add_header("host", "127.0.0.1:13300", true)
    .bearer_auth(TOKEN)
    .send(&f.service)
    .await;
    assert_eq!(missing.status_code, Some(StatusCode::NOT_FOUND));
    assert_eq!(
        missing.take_json::<Value>().await.unwrap(),
        json!({"ok":false,"code":"not_found"})
    );
    f.domain.shutdown().await.unwrap();
    let mut closed = TestClient::get(f.url())
        .add_header("host", "127.0.0.1:13300", true)
        .bearer_auth(TOKEN)
        .send(&f.service)
        .await;
    assert_eq!(closed.status_code, Some(StatusCode::SERVICE_UNAVAILABLE));
    assert_eq!(
        closed.take_json::<Value>().await.unwrap(),
        json!({"ok":false,"code":"usage_unavailable"})
    );
    f.custody.shutdown().await.unwrap();
    let f = Fixture::new(&[], false, false);
    let mut missing = TestClient::get(f.url())
        .add_header("host", "127.0.0.1:13300", true)
        .bearer_auth(TOKEN)
        .send(&f.service)
        .await;
    assert_eq!(missing.status_code, Some(StatusCode::SERVICE_UNAVAILABLE));
    assert_eq!(
        missing.take_json::<Value>().await.unwrap(),
        json!({"ok":false,"code":"domain_unavailable"})
    );
    f.close().await;
}
