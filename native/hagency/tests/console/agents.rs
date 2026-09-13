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

/// CL-S2 commit 1 — the store surface (ADR-130): `stop_dispatch_for_agent`
/// resolves the engagement's dispatch through the live set or the unsettled
/// stop row, fences only that dispatch, and is at-most-once by construction:
/// a second call resolves the SAME dispatch id and fence, writes no second
/// stop row, and still reports `stop_pending` — `stopped` stays false
/// because no production path settles (settlement is the host's, uncallable
/// from runtime-facing commands). Driven against the real DomainStore
/// writer, not the HTTP surface.
#[tokio::test]
async fn native_console_stop_dispatch_for_agent_is_at_most_once() {
    let f = Fixture::new("127.0.0.1:13300".parse().unwrap(), None);
    // The fixture's engagement carries a STARTED dispatch (uncertain), so
    // the fence writes a dispatch_stops row and quarantines the session —
    // never the settled word.
    let first = f
        .domain
        .stop_dispatch_for_agent(f.engagement.clone(), 2000)
        .await
        .unwrap();
    let keys = ["stopped", "stop_pending", "dispatch_id", "fence", "state"];
    assert_eq!(first.as_object().unwrap().len(), keys.len());
    for key in keys {
        assert!(
            first.as_object().unwrap().contains_key(key),
            "carries {key}"
        );
    }
    assert_eq!(first["stopped"], false, "no production path settles");
    assert_eq!(first["stop_pending"], true, "an honest stop is pending");
    assert_eq!(
        first["state"], "outcome_unknown",
        "the uncertain fence word"
    );
    let dispatch = first["dispatch_id"].as_str().unwrap().to_owned();
    let fence = first["fence"].as_u64().unwrap();
    assert!(!dispatch.is_empty());
    // At-most-once: the second call resolves the same row, writes nothing
    // new, and reports the same five keys.
    let second = f
        .domain
        .stop_dispatch_for_agent(f.engagement.clone(), 2001)
        .await
        .unwrap();
    assert_eq!(second["dispatch_id"], first["dispatch_id"]);
    assert_eq!(second["fence"], first["fence"]);
    assert_eq!(second["stop_pending"], true);
    assert_eq!(second["stopped"], false);
    let raw = rusqlite::Connection::open(f.root.path().join("state/domain.sqlite3")).unwrap();
    let count: i64 = raw
        .query_row(
            "SELECT COUNT(*) FROM dispatch_stops WHERE dispatch_id=?1",
            [&dispatch],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1, "exactly one stop row — no second write");
    let settled: Option<u64> = raw
        .query_row(
            "SELECT settled_at FROM dispatch_stops WHERE dispatch_id=?1",
            [&dispatch],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(settled, None, "the store never settles");
    drop(raw);
    // An engagement with no live dispatch and no unsettled stop row refuses
    // with the store's NotFound — nothing to resolve, nothing fenced.
    let absent = format!("{}_absent", f.engagement);
    assert!(matches!(
        f.domain.stop_dispatch_for_agent(absent, 2002).await,
        Err(hagency_store::Error::NotFound)
    ));
    let _ = fence;
    f.close().await;
}
