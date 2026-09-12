#[path = "console/browser.rs"]
#[cfg(feature = "native-console-browser")]
mod browser;
#[path = "console/configuration.rs"]
mod configuration;
#[path = "console/fixture.rs"]
mod fixture;
#[path = "console/resources.rs"]
mod resources;
use fixture::*;
use salvo::{
    prelude::*,
    test::{ResponseExt, TestClient},
};
use serde_json::{Value, json};

async fn issue(service: &Service) -> String {
    let mut response = TestClient::post(format!("{BASE}/api/native/v1/console/access"))
        .add_header("host", "127.0.0.1:13300", true)
        .bearer_auth(TOKEN)
        .send(service)
        .await;
    assert_eq!(response.status_code, Some(StatusCode::OK));
    response.take_json::<Value>().await.unwrap()["ticket"]
        .as_str()
        .unwrap()
        .to_owned()
}
async fn exchange(service: &Service, ticket: &str) -> Response {
    TestClient::post(format!("{BASE}/console/session"))
        .add_header("host", "127.0.0.1:13300", true)
        .add_header("origin", BASE, true)
        .add_header("sec-fetch-site", "same-origin", true)
        .json(&json!({"ticket":ticket}))
        .send(service)
        .await
}
async fn session(service: &Service) -> String {
    let ticket = issue(service).await;
    let response = exchange(service, &ticket).await;
    assert_eq!(response.status_code, Some(StatusCode::OK));
    let cookie = response
        .headers()
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap();
    for flag in [
        "HttpOnly",
        "SameSite=Strict",
        "Path=/console",
        "Max-Age=900",
    ] {
        assert!(cookie.contains(flag));
    }
    cookie.split(';').next().unwrap().to_owned()
}
fn get(path: &str, cookie: &str) -> salvo::test::RequestBuilder {
    TestClient::get(format!("{BASE}{path}"))
        .add_header("host", "127.0.0.1:13300", true)
        .add_header("sec-fetch-site", "same-origin", true)
        .add_header("cookie", cookie, true)
}

#[tokio::test]
async fn native_console_authority() {
    let f = Fixture::new("127.0.0.1:13300".parse().unwrap(), None);
    let service = f.service();
    let anonymous = TestClient::get(format!("{BASE}/console/api/engagements"))
        .add_header("host", "127.0.0.1:13300", true)
        .send(&service)
        .await;
    assert_eq!(anonymous.status_code, Some(StatusCode::UNAUTHORIZED));
    let denied = TestClient::post(format!("{BASE}/api/native/v1/console/access"))
        .add_header("host", "127.0.0.1:13300", true)
        .bearer_auth(TOKEN)
        .add_header("origin", BASE, true)
        .send(&service)
        .await;
    assert_eq!(denied.status_code, Some(StatusCode::FORBIDDEN));
    let ticket = issue(&service).await;
    let rate = TestClient::post(format!("{BASE}/api/native/v1/console/access"))
        .add_header("host", "127.0.0.1:13300", true)
        .bearer_auth(TOKEN)
        .send(&service)
        .await;
    assert_eq!(rate.status_code, Some(StatusCode::TOO_MANY_REQUESTS));
    let response = exchange(&service, &ticket).await;
    assert_eq!(response.status_code, Some(StatusCode::OK));
    let cookie = response
        .headers()
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    assert_eq!(
        exchange(&service, &ticket).await.status_code,
        Some(StatusCode::UNAUTHORIZED)
    );
    for (name, value) in [
        ("host", "evil.test"),
        ("origin", "https://evil.test"),
        ("sec-fetch-site", "cross-site"),
        ("x-forwarded-for", "127.0.0.1"),
        ("cookie", "hagency_console=bad"),
        ("cookie", &format!("{cookie}; {cookie}")),
    ] {
        let response = get("/console/api/engagements", &cookie)
            .add_header(name, value, true)
            .send(&service)
            .await;
        assert!(matches!(
            response.status_code,
            Some(StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN)
        ));
    }
    let raw = get("/api/native/v1/engagements", &cookie)
        .bearer_auth(TOKEN)
        .send(&service)
        .await;
    assert_eq!(raw.status_code, Some(StatusCode::FORBIDDEN));
    for fetch in ["none", "cross-site"] {
        let response = TestClient::get(format!("{BASE}/console/usage/#access=unused"))
            .add_header("host", "127.0.0.1:13300", true)
            .add_header("sec-fetch-site", fetch, true)
            .send(&service)
            .await;
        assert_eq!(response.status_code, Some(StatusCode::OK));
    }
    let logout = TestClient::delete(format!("{BASE}/console/session"))
        .add_header("host", "127.0.0.1:13300", true)
        .add_header("origin", BASE, true)
        .add_header("sec-fetch-site", "same-origin", true)
        .add_header("cookie", &cookie, true)
        .send(&service)
        .await;
    assert_eq!(logout.status_code, Some(StatusCode::OK));
    assert_eq!(
        get("/console/api/engagements", &cookie)
            .send(&service)
            .await
            .status_code,
        Some(StatusCode::UNAUTHORIZED)
    );
    f.close().await;
}

#[tokio::test]
async fn native_console_assets() {
    let f = Fixture::new("127.0.0.1:13300".parse().unwrap(), None);
    let path = f.root.path().join("assets").canonicalize().unwrap();
    std::fs::write(path.join("usage/index.html"), b"changed after admission").unwrap();
    assert!(hagency::console::Console::load(&path).is_err());
    let mut response = TestClient::get(format!("{BASE}/console/usage/"))
        .add_header("host", "127.0.0.1:13300", true)
        .send(&f.service())
        .await;
    assert!(
        response
            .take_string()
            .await
            .unwrap()
            .contains("retained asset fixture")
    );
    for path in [
        "/console/operator.token",
        "/console/agents/FixtureName",
        "/console/api/hagency/agents",
        "/console/_next/static/unknown.js",
        "/console/usage/?unknown=1",
    ] {
        let response = TestClient::get(format!("{BASE}{path}"))
            .add_header("host", "127.0.0.1:13300", true)
            .send(&f.service())
            .await;
        assert!(response.status_code.unwrap().is_client_error());
    }
    #[cfg(unix)]
    {
        let alias = f.root.path().canonicalize().unwrap().join("alias");
        std::os::unix::fs::symlink(&path, &alias).unwrap();
        assert!(hagency::console::Console::load(&alias).is_err());
        std::fs::remove_file(path.join("usage/index.html")).unwrap();
        std::os::unix::fs::symlink(path.join("manifest.json"), path.join("usage/index.html"))
            .unwrap();
        assert!(hagency::console::Console::load(&path).is_err());
    }
    for change in [
        "size",
        "hash",
        "count",
        "duplicate",
        "unknown",
        "manifest_bytes",
    ] {
        let dir = f.root.path().join(format!("assets_{change}"));
        assets(&dir);
        let dir = dir.canonicalize().unwrap();
        let manifest = dir.join("manifest.json");
        let mut value: Value = serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
        match change {
            "size" => value["assets"][0]["size"] = json!(4 * 1024 * 1024 + 1),
            "hash" => value["assets"][0]["sha256"] = json!("0".repeat(64)),
            "count" => value["assets"] = json!(vec![value["assets"][0].clone(); 513]),
            "duplicate" => value["assets"] = json!(vec![value["assets"][0].clone(); 2]),
            "unknown" => value["secret"] = json!("not_an_asset_field"),
            "manifest_bytes" => {}
            _ => unreachable!(),
        }
        let bytes = if change == "manifest_bytes" {
            vec![b' '; 128 * 1024 + 1]
        } else {
            serde_json::to_vec(&value).unwrap()
        };
        std::fs::write(manifest, bytes).unwrap();
        assert!(
            hagency::console::Console::load(&dir).is_err(),
            "invalid {change} set was admitted"
        );
    }
    f.close().await;
}

#[tokio::test]
async fn native_console_usage() {
    let f = Fixture::new("127.0.0.1:13300".parse().unwrap(), None);
    let service = f.service();
    let cookie = session(&service).await;
    let mut listing = get("/console/api/engagements?limit=1", &cookie)
        .send(&service)
        .await;
    let value = listing.take_json::<Value>().await.unwrap();
    assert_eq!(value["engagements"][0]["id"], f.engagement);
    assert_private(&value);
    let path = format!("/console/api/engagements/{}/usage", f.engagement);
    let mut response = get(&path, &cookie).send(&service).await;
    assert_eq!(response.status_code, Some(StatusCode::OK));
    let value = response.take_json::<Value>().await.unwrap();
    assert_private(&value);
    assert_eq!(value["summary"]["latest_counts"]["input"], 4);
    assert_eq!(value["summary"]["known_high_water_lower_bound"]["input"], 7);
    assert_eq!(value["summary"]["regression_observations"], 1);
    assert_eq!(value["daily"]["incomplete"], true);
    let new_id = f.new_engagement().await;
    let mut response = get(&format!("/console/api/engagements/{new_id}/usage"), &cookie)
        .send(&service)
        .await;
    let value = response.take_json::<Value>().await.unwrap();
    assert_eq!(value["summary"]["sources"], 0);
    assert!(value["summary"]["latest_counts"].is_null() && value["daily"].is_null());
    for suffix in [
        "?at_ms=1&at_ms=2",
        "?at_ms=bad",
        "?x=1",
        "?at_ms=18446744073709551616",
    ] {
        assert_eq!(
            get(&format!("{path}{suffix}"), &cookie)
                .send(&service)
                .await
                .status_code,
            Some(StatusCode::BAD_REQUEST)
        );
    }
    for query in ["limit=17", "limit=0", "limit=1&limit=2", "unknown=1"] {
        assert_eq!(
            get(&format!("/console/api/engagements?{query}"), &cookie)
                .send(&service)
                .await
                .status_code,
            Some(StatusCode::BAD_REQUEST)
        );
    }
    // Hold the actual SQLite writer, then poll the authenticated HTTP read into
    // its queue wait. Retirement occurs after admission and before its response.
    let lock = rusqlite::Connection::open(f.root.path().join("state/domain.sqlite3")).unwrap();
    lock.execute_batch("BEGIN IMMEDIATE").unwrap();
    let mut write = Box::pin(f.domain.register(common::registration()));
    assert!(
        std::future::poll_fn(|cx| std::task::Poll::Ready(write.as_mut().poll(cx)))
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
    write.await.unwrap();
    assert_eq!(
        waiting.await.status_code,
        Some(StatusCode::SERVICE_UNAVAILABLE)
    );
    assert_eq!(
        get(&path, &cookie).send(&service).await.status_code,
        Some(StatusCode::SERVICE_UNAVAILABLE)
    );
    f.close().await;
}
