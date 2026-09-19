use super::*;
use hagency_store::resource_publication_revision;
async fn scoped(service: &Service, scope: &str) -> String {
    let mut response = TestClient::post(format!("{BASE}/api/native/v1/console/{scope}"))
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
fn write(path: &str, cookie: &str, edit: bool) -> salvo::test::RequestBuilder {
    let request = if edit {
        TestClient::patch(format!("{BASE}{path}"))
    } else {
        TestClient::post(format!("{BASE}{path}"))
    };
    request
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
fn changes(r: &hagency_core::project::Resource) -> Value {
    json!({"expectedRevision":resource_publication_revision(r).unwrap(),"profileChange":{"kind":"preserve"},"ceilingChange":{"kind":"monthly","tokens":321}})
}
#[tokio::test]
async fn native_console_resource_configuration_authority() {
    let f = Fixture::new("127.0.0.1:13300".parse().unwrap(), None);
    let source = native_resource("private_configuration_authority");
    f.domain.put_resource(source.clone()).await.unwrap();
    let service = f.service();
    let readonly = session(&service).await;
    let path = format!("/console/api/resources/{}/configuration", source.id());
    let input = changes(&source);
    assert_eq!(
        write(&path, &readonly, true)
            .json(&input)
            .send(&service)
            .await
            .status_code,
        Some(StatusCode::FORBIDDEN)
    );
    assert_eq!(
        TestClient::post(format!(
            "{BASE}/api/native/v1/console/resource-configuration-access"
        ))
        .add_header("host", "127.0.0.1:13300", true)
        .bearer_auth(TOKEN)
        .send(&service)
        .await
        .status_code,
        Some(StatusCode::TOO_MANY_REQUESTS)
    );
    tokio::time::sleep(std::time::Duration::from_millis(1010)).await;
    let publication = scoped(&service, "resource-publication-access").await;
    assert_eq!(
        write(&path, &publication, true)
            .json(&input)
            .send(&service)
            .await
            .status_code,
        Some(StatusCode::FORBIDDEN)
    );
    tokio::time::sleep(std::time::Duration::from_millis(1010)).await;
    let manager = scoped(&service, "resource-configuration-access").await;
    let publication_path = format!("/console/api/resources/{}/publication", source.id());
    assert_eq!(
        write(&publication_path, &manager, false)
            .json(&json!({"expectedRevision":input["expectedRevision"],"published":false}))
            .send(&service)
            .await
            .status_code,
        Some(StatusCode::FORBIDDEN)
    );
    assert_eq!(
        get("/api/native/v1/resources", &manager)
            .send(&service)
            .await
            .status_code,
        Some(StatusCode::FORBIDDEN)
    );
    for key in [
        "presetId",
        "seatId",
        "framework",
        "provider",
        "name",
        "executionPolicy",
        "scope",
    ] {
        let mut malformed = input.clone();
        malformed[key] = json!("untrusted");
        assert_eq!(
            write(&path, &manager, true)
                .json(&malformed)
                .send(&service)
                .await
                .status_code,
            Some(StatusCode::BAD_REQUEST)
        );
    }
    assert_eq!(
        write(&path, &manager, true)
            .json(&input)
            .add_header("origin", "https://outside.invalid", true)
            .send(&service)
            .await
            .status_code,
        Some(StatusCode::UNAUTHORIZED)
    );
    let lock = rusqlite::Connection::open(f.root.path().join("state/domain.sqlite3")).unwrap();
    lock.execute_batch("BEGIN IMMEDIATE").unwrap();
    let mut operation = Box::pin(write(&path, &manager, true).json(&input).send(&service));
    assert!(
        std::future::poll_fn(|cx| std::task::Poll::Ready(operation.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    let busy = logout(&manager).send(&service).await;
    assert_eq!(busy.status_code, Some(StatusCode::TOO_MANY_REQUESTS));
    assert!(!busy.headers().contains_key("set-cookie"));
    assert_eq!(
        logout(&readonly).send(&service).await.status_code,
        Some(StatusCode::OK)
    );
    lock.execute_batch("COMMIT").unwrap();
    assert_eq!(operation.await.status_code, Some(StatusCode::OK));
    assert_eq!(
        write(&path, &manager, true)
            .json(&input)
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
        write(&path, &manager, true)
            .json(&input)
            .send(&service)
            .await
            .status_code,
        Some(StatusCode::UNAUTHORIZED)
    );
    f.close().await;
}
#[tokio::test]
async fn native_console_resource_configuration_observations() {
    let f = Fixture::new("127.0.0.1:13300".parse().unwrap(), None);
    let mut resource = native_resource("private_editor_exact_source");
    resource.published = false;
    resource.ceiling = Some(serde_json::from_value(json!({"tokens":null})).unwrap());
    f.domain.put_resource(resource.clone()).await.unwrap();
    let service = f.service();
    let cookie = session(&service).await;
    let path = format!("/console/api/resources/{}/configuration", resource.id());
    let mut response = get(&path, &cookie).send(&service).await;
    assert_eq!(response.status_code, Some(StatusCode::OK));
    let value = response.take_json::<Value>().await.unwrap();
    assert_eq!(value["resource"]["id"], resource.id());
    assert_eq!(value["resource"]["published"], false);
    assert!(value["resource"]["ceiling"].get("period").is_none());
    assert!(value["choices"].as_array().unwrap().len() <= 256);
    for private in [
        "private_editor_exact_source",
        "private_resource_account",
        "presetId",
        "seatId",
        "authHome",
        "apiKey",
        "operator.token",
    ] {
        assert!(!value.to_string().contains(private));
    }
    let mut list = get("/console/api/resources?limit=1", &cookie)
        .send(&service)
        .await;
    assert_eq!(
        list.take_json::<Value>().await.unwrap()["permissions"],
        json!({"publishResource":false,"configureResource":false})
    );
    for query in ["a=b", "resource_id=x", "limit=2&limit=3"] {
        assert_eq!(
            get(&format!("{path}?{query}"), &cookie)
                .send(&service)
                .await
                .status_code,
            Some(StatusCode::BAD_REQUEST)
        );
    }
    resource.model = "unknown-original-model".into();
    resource.reasoning = Some("unknown-original-reasoning".into());
    f.domain.put_resource(resource.clone()).await.unwrap();
    let mut response = get(&path, &cookie).send(&service).await;
    let value = response.take_json::<Value>().await.unwrap();
    assert_eq!(value["resource"]["model"], resource.model);
    assert!(value["modelTier"].is_null());
    for query in [
        "resource_id=x&source_resource_id=y",
        "source_resource_id=x&source_resource_id=y",
        "unknown=x",
    ] {
        assert_eq!(
            get(&format!("/console/resources/new/?{query}"), &cookie)
                .send(&service)
                .await
                .status_code,
            Some(StatusCode::BAD_REQUEST)
        );
    }
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
