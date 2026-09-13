use super::*;

/// The forbidden credential vocabulary: the two Matrix registration tokens
/// and their camelCase spellings — asserted against the RAW response bytes
/// and recursively over every decoded JSON string value, so a token that
/// leaked inside an otherwise-allowed key fails here, not only in the
/// parsed key set.
const FORBIDDEN: [&str; 4] = ["as_token", "hs_token", "asToken", "hsToken"];

fn assert_no_credential_text(value: &Value) {
    match value {
        Value::String(text) => {
            for word in FORBIDDEN {
                assert!(
                    !text.contains(word),
                    "a JSON string value carries {word}: {text}"
                );
            }
        }
        Value::Array(items) => items.iter().for_each(assert_no_credential_text),
        Value::Object(map) => map.values().for_each(assert_no_credential_text),
        _ => {}
    }
}

/// The project-side projection carries no credential in any byte of its
/// response (ADR-132): the store config is seeded with token-shaped values
/// first, then the read is asserted over the serialized body — exactly six
/// keys per item, projects of exactly `id` and `room_id`, the withheld
/// owner fields absent, and the server-owned `unavailable` list naming
/// every column native has no source for.
#[tokio::test]
async fn native_console_project_side_projection_omits_credentials() {
    let f = Fixture::new("127.0.0.1:13300".parse().unwrap(), None);
    let service = f.service();
    let anonymous = TestClient::get(format!("{BASE}/console/api/project-sides"))
        .add_header("host", "127.0.0.1:13300", true)
        .send(&service)
        .await;
    assert_eq!(anonymous.status_code, Some(StatusCode::UNAUTHORIZED));
    let cookie = session(&service).await;
    // A list observation takes no selection: every query parameter refused.
    for query in ["?limit=1", "?id=palpo.test", "?after=%31"] {
        let response = get(&format!("/console/api/project-sides{query}"), &cookie)
            .send(&service)
            .await;
        assert_eq!(
            response.status_code,
            Some(StatusCode::BAD_REQUEST),
            "{query}"
        );
    }
    // Seed the forward guard: token-shaped values inside the registration
    // config — the only place native could ever grow one.
    let as_token = "as_token_7c1d3f9a2e8b4056";
    let hs_token = "hs_token_0b4e6d8c1f3a7295";
    let raw = rusqlite::Connection::open(f.root.path().join("state/domain.sqlite3")).unwrap();
    raw.execute(
        "UPDATE registrations SET config=json_set(config,'$.as_token',?1,'$.hs_token',?2)",
        rusqlite::params![as_token, hs_token],
    )
    .unwrap();
    drop(raw);
    let mut response = get("/console/api/project-sides", &cookie)
        .send(&service)
        .await;
    assert_eq!(response.status_code, Some(StatusCode::OK));
    assert_eq!(response.headers().get("cache-control").unwrap(), "no-store");
    // The negative runs on the RAW body first: any byte carrying the
    // seeded values or the credential words fails, whatever key wraps them.
    let body = response.take_string().await.unwrap();
    assert!(!body.contains(as_token), "no as_token value in any byte");
    assert!(!body.contains(hs_token), "no hs_token value in any byte");
    for word in FORBIDDEN {
        assert!(!body.contains(word), "no {word} anywhere in the body");
    }
    let value: Value = serde_json::from_str(&body).unwrap();
    assert_no_credential_text(&value);
    assert_eq!(
        value.as_object().unwrap().len(),
        3,
        "the envelope carries exactly at_ms, unavailable, sides"
    );
    assert!(value["at_ms"].as_u64().unwrap() > 0);
    let names: Vec<&str> = value["unavailable"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "label",
            "api_base_url",
            "credential_kind",
            "has_credential",
            "awaiting_install",
            "sender_localpart",
            "appservice_url",
            "namespace",
            "access_state",
            "access_detail",
            "allocated_tokens",
            "project_name",
            "owner",
        ],
        "the server names every column it has no source for"
    );
    let sides = value["sides"].as_array().unwrap();
    assert_eq!(sides.len(), 1, "the fixture registers one fleet");
    let side = &sides[0];
    let keys = [
        "id",
        "representative",
        "generation",
        "reception_room_id",
        "registered",
        "projects",
    ];
    let object = side.as_object().unwrap();
    assert_eq!(object.len(), keys.len(), "exactly six keys");
    for key in keys {
        assert!(object.contains_key(key), "the wire item carries {key}");
    }
    assert_eq!(side["id"], "example.test", "the id IS the server name");
    assert_eq!(
        side["representative"],
        common::registration().representative_mxid
    );
    assert_eq!(side["generation"], 1);
    assert_eq!(side["reception_room_id"], "!reception:example.test");
    assert_eq!(side["registered"], true, "row and config generations agree");
    let projects = side["projects"].as_array().unwrap();
    assert_eq!(
        projects.len(),
        1,
        "the fixture's engagements share one project"
    );
    assert_eq!(
        serde_json::to_value(&projects[0]).unwrap(),
        json!({"id":"project_one","room_id":"!project:example.test"}),
        "projects entries are exactly id and room_id"
    );
    let text = serde_json::to_string(&value).unwrap();
    assert!(!text.contains("@owner:example.test"), "owner mxid withheld");
    assert!(!text.contains("!private:example.test"), "owner DM withheld");
    assert_private(&value);
    f.close().await;
}

/// A foreign origin cannot read the project sides: the boundary refuses
/// with `console_origin_required` before any store read, the session hoop
/// refuses the transport-level forgeries with the access-required word —
/// and no refusal body carries a side item. The page keeps the
/// five-document exception: it serves as a non-document, so a cross-site
/// navigation is refused and no query string is accepted at all.
#[tokio::test]
async fn native_console_project_side_refuses_foreign_origin() {
    let f = Fixture::new("127.0.0.1:13300".parse().unwrap(), None);
    let service = f.service();
    let cookie = session(&service).await;
    for (name, value, status, code) in [
        (
            "host",
            "evil.test",
            StatusCode::FORBIDDEN,
            "console_origin_required",
        ),
        (
            "origin",
            "https://evil.test",
            StatusCode::UNAUTHORIZED,
            "console_access_required",
        ),
        (
            "sec-fetch-site",
            "cross-site",
            StatusCode::UNAUTHORIZED,
            "console_access_required",
        ),
        (
            "sec-fetch-site",
            "none",
            StatusCode::UNAUTHORIZED,
            "console_access_required",
        ),
        (
            "x-forwarded-for",
            "127.0.0.1",
            StatusCode::FORBIDDEN,
            "console_origin_required",
        ),
        (
            "forwarded",
            "for=127.0.0.1",
            StatusCode::FORBIDDEN,
            "console_origin_required",
        ),
    ] {
        let mut response = get("/console/api/project-sides", &cookie)
            .add_header(name, value, true)
            .send(&service)
            .await;
        assert_eq!(response.status_code, Some(status), "{name}: {value}");
        let body = response.take_json::<Value>().await.unwrap();
        assert_eq!(body["code"], code, "{name}: {value}");
        assert!(
            body.get("sides").is_none(),
            "{name}: {value} serves no side item"
        );
    }
    // The non-document rule: cross-site navigation refused, any query on
    // the page path refused.
    let cross = TestClient::get(format!("{BASE}/console/project-sides/"))
        .add_header("host", "127.0.0.1:13300", true)
        .add_header("sec-fetch-site", "cross-site", true)
        .send(&service)
        .await;
    assert_eq!(cross.status_code, Some(StatusCode::FORBIDDEN));
    let queried = get("/console/project-sides/?anything=1", &cookie)
        .send(&service)
        .await;
    assert_eq!(queried.status_code, Some(StatusCode::BAD_REQUEST));
    // No mutation exists: the router refuses a non-GET outright.
    let post = TestClient::post(format!("{BASE}/console/api/project-sides"))
        .add_header("host", "127.0.0.1:13300", true)
        .add_header("origin", BASE, true)
        .add_header("sec-fetch-site", "same-origin", true)
        .add_header("cookie", &cookie, true)
        .send(&service)
        .await;
    assert!(matches!(
        post.status_code,
        Some(StatusCode::NOT_FOUND | StatusCode::METHOD_NOT_ALLOWED)
    ));
    f.close().await;
}
