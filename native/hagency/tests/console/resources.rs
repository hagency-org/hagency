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
