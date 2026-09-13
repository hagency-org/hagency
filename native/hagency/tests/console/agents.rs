use super::*;

/// The agent roster observation (ADR-126): the read is a bounded
/// projection of the engagement rows — every item carries EXACTLY the
/// seven declared keys, no nested object, no private field — and the
/// null-not-zero rule is pinned on the seeded rows with no attempt
/// (`AlertWorker`, `PageWorker` are `pending` with `last_activity_ms:
/// null`, while `UsageWorker` is `active` with the attempt clock).
#[tokio::test]
async fn native_console_agent_roster_observation() {
    let f = Fixture::new("127.0.0.1:13300".parse().unwrap(), None);
    let service = f.service();
    let anonymous = TestClient::get(format!("{BASE}/console/api/agents"))
        .add_header("host", "127.0.0.1:13300", true)
        .send(&service)
        .await;
    assert_eq!(anonymous.status_code, Some(StatusCode::UNAUTHORIZED));
    let cookie = session(&service).await;
    // A roster takes no selection: every query parameter is refused.
    for query in ["?limit=1", "?after=x", "?name=UsageWorker", "?limit=%31"] {
        let response = get(&format!("/console/api/agents{query}"), &cookie)
            .send(&service)
            .await;
        assert_eq!(
            response.status_code,
            Some(StatusCode::BAD_REQUEST),
            "{query}"
        );
    }
    let mut response = get("/console/api/agents", &cookie).send(&service).await;
    assert_eq!(response.status_code, Some(StatusCode::OK));
    assert_eq!(response.headers().get("cache-control").unwrap(), "no-store");
    let value = response.take_json::<Value>().await.unwrap();
    assert_eq!(
        value.as_object().unwrap().len(),
        3,
        "the envelope carries exactly at_ms, unavailable, agents"
    );
    assert!(value["at_ms"].as_u64().unwrap() > 0);
    let unavailable = value["unavailable"].as_array().unwrap();
    let names: Vec<&str> = unavailable.iter().map(|v| v.as_str().unwrap()).collect();
    assert_eq!(
        names,
        [
            "consumed",
            "last_seen",
            "online",
            "tmux",
            "pane",
            "credential_home",
            "workspace_path",
            "seat",
        ],
        "the server names every column it has no source for"
    );
    let agents = value["agents"].as_array().unwrap();
    assert_eq!(agents.len(), 3, "one row per seeded engagement");
    let keys = [
        "name",
        "framework",
        "role",
        "state",
        "engagement_id",
        "requested_tokens",
        "last_activity_ms",
    ];
    let mut by_name: Vec<(String, &Value)> = agents
        .iter()
        .map(|a| (a["name"].as_str().unwrap().to_owned(), a))
        .collect();
    by_name.sort_by(|a, b| a.0.cmp(&b.0));
    let names: Vec<&str> = by_name.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["AlertWorker", "PageWorker", "UsageWorker"]);
    for (_, agent) in &by_name {
        let object = agent.as_object().unwrap();
        assert_eq!(object.len(), keys.len(), "exactly seven keys");
        for key in keys {
            assert!(object.contains_key(key), "the wire item carries {key}");
            assert!(
                !object[key].is_object() && !object[key].is_array(),
                "no nested object or array travels on a roster item"
            );
        }
        assert_eq!(agent["framework"], "codex", "the single native framework");
        assert_eq!(agent["role"], "coding");
        assert!(
            agent["engagement_id"]
                .as_str()
                .is_some_and(engagement_id_shape),
            "the engagement id keeps its opaque shape"
        );
        assert_eq!(agent["requested_tokens"], 100);
    }
    let usage = &by_name[2].1;
    assert_eq!(usage["state"], "active");
    assert_eq!(
        usage["last_activity_ms"], 1002,
        "the newest attempt clock — last dispatch activity, not last seen"
    );
    // The null-not-zero rule: engagements with no attempt row report
    // unknown, never an invented zero clock. AlertWorker was approved but
    // its effect was never observed (reserved); PageWorker is admit-only
    // (pending) — neither has a session, dispatch or attempt row.
    assert_eq!(
        by_name[0].1["state"], "reserved",
        "AlertWorker approved, effect unobserved"
    );
    assert_eq!(by_name[1].1["state"], "pending", "PageWorker admit-only");
    for (name, agent) in [&by_name[0], &by_name[1]] {
        assert!(
            agent["last_activity_ms"].is_null(),
            "{name} carries null, not zero"
        );
    }
    assert_private(&value);
    f.close().await;
}

/// A foreign origin cannot read the roster: the boundary refuses with
/// `console_origin_required` before any store read, and the session hoop
/// refuses the transport-level forgeries with the access-required word —
/// neither serves a single agent item.
#[tokio::test]
async fn native_console_agent_roster_refuses_foreign_origin() {
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
        let mut response = get("/console/api/agents", &cookie)
            .add_header(name, value, true)
            .send(&service)
            .await;
        assert_eq!(response.status_code, Some(status), "{name}: {value}");
        let body = response.take_json::<Value>().await.unwrap();
        assert_eq!(body["code"], code, "{name}: {value}");
        assert!(
            body.get("agents").is_none(),
            "{name}: {value} serves no agent item"
        );
    }
    // The document rule keeps its five-name exception: the roster page is a
    // NON-document, so a cross-site navigation is refused with the origin
    // word and the page takes no query string at all.
    let cross = TestClient::get(format!("{BASE}/console/agents/"))
        .add_header("host", "127.0.0.1:13300", true)
        .add_header("sec-fetch-site", "cross-site", true)
        .send(&service)
        .await;
    assert_eq!(cross.status_code, Some(StatusCode::FORBIDDEN));
    let queried = get("/console/agents/?anything=1", &cookie)
        .send(&service)
        .await;
    assert_eq!(queried.status_code, Some(StatusCode::BAD_REQUEST));
    // No mutation exists: the router refuses a non-GET outright.
    let post = TestClient::post(format!("{BASE}/console/api/agents"))
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

/// The store's opaque engagement id: `en_` + 32 hex characters
/// (`authority.rs:123`) — it names no session, no authority and no path.
fn engagement_id_shape(value: &str) -> bool {
    value.len() == 35
        && value.starts_with("en_")
        && value[3..].bytes().all(|b| b.is_ascii_hexdigit())
}
