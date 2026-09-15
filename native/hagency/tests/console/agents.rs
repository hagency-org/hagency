use super::*;
use hagency_core::tasks::SessionBinding;
use hagency_store::resource_publication_revision;

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
        4,
        "the envelope carries exactly at_ms, unavailable, agents, permissions"
    );
    assert!(value["at_ms"].as_u64().unwrap() > 0);
    assert!(
        value["permissions"]["manageLifecycle"].as_bool() == Some(false),
        "a read-only session serves no lifecycle permission"
    );
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

/// CL-S2 (ADR-130) scope selector: the lifecycle gate refuses a read-only
/// session on all three acts with `agent_lifecycle_scope_required` and no
/// engagement row changes, and a lifecycle session performs only start/stop/
/// preset — it is refused by publication and configuration with THEIR scope
/// words, never its own.
#[tokio::test]
async fn native_console_agent_lifecycle_is_scoped() {
    let f = Fixture::new("127.0.0.1:13300".parse().unwrap(), None);
    let service = f.service();
    let readonly = session(&service).await;
    // The console's issuance budget is one per second ACROSS scopes
    // (authority.rs `issue_scope`): the read-only issue above and the
    // lifecycle issue below cannot land in the same second without the
    // second answering busy (429). Drive them one at a time — the house
    // pattern the resources authority test uses — never a retry loop.
    tokio::time::sleep(std::time::Duration::from_millis(1010)).await;
    let lifecycle = lifecycle_session(&service).await;
    let id = &f.engagement;
    // Read-only: all three acts refused before any store work.
    for path in [
        format!("/console/api/agents/{id}/start"),
        format!("/console/api/agents/{id}/stop"),
        format!("/console/api/agents/{id}/preset"),
    ] {
        let mut builder = post(&path, &readonly);
        if path.ends_with("/preset") {
            builder = builder.json(&json!({"presetId":"private_usage_pool"}));
        }
        let mut response = builder.send(&service).await;
        assert_eq!(
            response.status_code,
            Some(StatusCode::FORBIDDEN),
            "read-only {path}"
        );
        let body = response.take_json::<Value>().await.unwrap();
        assert_eq!(
            body["code"], "agent_lifecycle_scope_required",
            "read-only {path}"
        );
    }
    // No engagement row changed: no stop row, the roster still holds it.
    let raw = rusqlite::Connection::open(f.root.path().join("state/domain.sqlite3")).unwrap();
    let stops: i64 = raw
        .query_row("SELECT COUNT(*) FROM dispatch_stops", [], |r| r.get(0))
        .unwrap();
    assert_eq!(stops, 0, "a refused stop writes no stop row");
    assert!(
        f.domain
            .agent_roster()
            .await
            .unwrap()
            .iter()
            .any(|r| r.engagement_id == *id),
        "the engagement row survives the refusals"
    );
    drop(raw);
    // The scoped session's own acts hold: start is refused as already-live,
    // stop fences — both with the LIFECYCLE session, not a widened scope.
    let mut response = post(&format!("/console/api/agents/{id}/start"), &lifecycle)
        .send(&service)
        .await;
    assert_eq!(response.status_code, Some(StatusCode::CONFLICT));
    assert_eq!(
        response.take_json::<Value>().await.unwrap()["code"],
        "agent_already_live"
    );
    // Neighbouring mutations refuse the lifecycle session with THEIR words.
    let source = native_resource("private_lifecycle_scope_source");
    f.domain.put_resource(source.clone()).await.unwrap();
    let revision = resource_publication_revision(&source).unwrap();
    let mut response = post(
        &format!("/console/api/resources/{}/publication", source.id()),
        &lifecycle,
    )
    .json(&json!({"expectedRevision":revision,"published":false}))
    .send(&service)
    .await;
    assert_eq!(response.status_code, Some(StatusCode::FORBIDDEN));
    assert_eq!(
        response.take_json::<Value>().await.unwrap()["code"],
        "resource_publication_scope_required"
    );
    let mut response = TestClient::patch(format!(
        "{BASE}/console/api/resources/{}/configuration",
        source.id()
    ))
    .add_header("host", "127.0.0.1:13300", true)
    .add_header("origin", BASE, true)
    .add_header("sec-fetch-site", "same-origin", true)
    .add_header("cookie", &lifecycle, true)
    .json(&json!({
        "expectedRevision":revision,
        "profileChange":{"kind":"preserve"},
        "ceilingChange":{"kind":"clear"}
    }))
    .send(&service)
    .await;
    assert_eq!(response.status_code, Some(StatusCode::FORBIDDEN));
    assert_eq!(
        response.take_json::<Value>().await.unwrap()["code"],
        "resource_configuration_scope_required"
    );
    // F1 (review r1): the account mutation refuses the lifecycle session
    // with ITS OWN word too — MA-S3a's surface is present on this lineage
    // post-rebase, so the scenario's clause is asserted, not dropped. The
    // prepare gate runs before any body is read or store work begins.
    let mut response = post("/console/api/accounts", &lifecycle)
        .send(&service)
        .await;
    assert_eq!(response.status_code, Some(StatusCode::FORBIDDEN));
    assert_eq!(
        response.take_json::<Value>().await.unwrap()["code"],
        "account_scope_required"
    );
    f.close().await;
}

/// CL-S2 (ADR-130) at-most-once selector, driven at the HTTP surface: a
/// start against the already-active fixture engagement refuses with the
/// named already-live word and spawns nothing; two stops resolve the SAME
/// dispatch id and fence through the unsettled stop row, writing no second
/// row, and both still report `stop_pending` — `stopped` stays false.
#[tokio::test]
async fn native_console_agent_start_stop_is_at_most_once() {
    let f = Fixture::new("127.0.0.1:13300".parse().unwrap(), None);
    let service = f.service();
    let lifecycle = lifecycle_session(&service).await;
    let id = &f.engagement;
    let mut response = post(&format!("/console/api/agents/{id}/start"), &lifecycle)
        .send(&service)
        .await;
    assert_eq!(response.status_code, Some(StatusCode::CONFLICT));
    assert_eq!(
        response.take_json::<Value>().await.unwrap()["code"],
        "agent_already_live"
    );
    let stop = || async {
        let mut response = post(&format!("/console/api/agents/{id}/stop"), &lifecycle)
            .send(&service)
            .await;
        assert_eq!(response.status_code, Some(StatusCode::OK));
        response.take_json::<Value>().await.unwrap()
    };
    let first = stop().await;
    for key in ["stopped", "stop_pending", "dispatch_id", "fence", "state"] {
        assert!(
            first.get(key).is_some(),
            "the stop wire object carries {key}"
        );
    }
    assert_eq!(first["stop_pending"], true);
    assert_eq!(first["stopped"], false);
    let second = stop().await;
    assert_eq!(second["dispatch_id"], first["dispatch_id"]);
    assert_eq!(second["fence"], first["fence"]);
    assert_eq!(second["stop_pending"], true);
    assert_eq!(second["stopped"], false);
    let raw = rusqlite::Connection::open(f.root.path().join("state/domain.sqlite3")).unwrap();
    let count: i64 = raw
        .query_row("SELECT COUNT(*) FROM dispatch_stops", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1, "the second stop writes no second row");
    let settled: Option<u64> = raw
        .query_row(
            "SELECT settled_at FROM dispatch_stops WHERE dispatch_id=?1",
            [first["dispatch_id"].as_str().unwrap()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(settled, None, "no production path settles the stop");
    drop(raw);
    f.close().await;
}

/// CL-S2 (ADR-130) preset-apply boundedness selector: an unpublished id
/// refuses with the named not-published word and writes no row, a published
/// id applies (a pointer — the response is exactly `{ok, presetId}`), and a
/// second apply refuses with the named one-pending word.
#[tokio::test]
async fn native_console_agent_preset_apply_is_bounded() {
    let f = Fixture::new("127.0.0.1:13300".parse().unwrap(), None);
    let service = f.service();
    let lifecycle = lifecycle_session(&service).await;
    let id = &f.engagement;
    let published = native_resource("private_preset_apply_published");
    let unpublished = native_resource("private_preset_apply_unpublished");
    f.domain.put_resource(published.clone()).await.unwrap();
    f.domain.put_resource(unpublished.clone()).await.unwrap();
    f.domain
        .edit_resource(unpublished.clone(), Some(false))
        .await
        .unwrap();
    // Unpublished id refuses; no row is written.
    let mut response = post(&format!("/console/api/agents/{id}/preset"), &lifecycle)
        .json(&json!({"presetId": unpublished.id()}))
        .send(&service)
        .await;
    assert_eq!(response.status_code, Some(StatusCode::NOT_FOUND));
    assert_eq!(
        response.take_json::<Value>().await.unwrap()["code"],
        "preset_not_published"
    );
    // Published id applies — a pointer, exactly two keys.
    let mut response = post(&format!("/console/api/agents/{id}/preset"), &lifecycle)
        .json(&json!({"presetId": published.id()}))
        .send(&service)
        .await;
    assert_eq!(response.status_code, Some(StatusCode::OK));
    let applied = response.take_json::<Value>().await.unwrap();
    assert_eq!(applied["ok"], true);
    assert_eq!(applied["presetId"], published.id());
    assert_eq!(
        applied.as_object().unwrap().len(),
        2,
        "a completed apply points only at the preset id, never widening a field"
    );
    // A second apply while one is pending refuses with the named word.
    let mut response = post(&format!("/console/api/agents/{id}/preset"), &lifecycle)
        .json(&json!({"presetId": published.id()}))
        .send(&service)
        .await;
    assert_eq!(response.status_code, Some(StatusCode::CONFLICT));
    assert_eq!(
        response.take_json::<Value>().await.unwrap()["code"],
        "agent_lifecycle_apply_pending"
    );
    f.close().await;
}

/// G5a / ADR-148 — operator recovery of an orphaned dispatch through the
/// console route. Seed an orphan (outcome_unknown, quarantined session, dirty
/// workspace, lease held, NO dispatch_stops row) beside the owned fixture
/// dispatch, then POST recover-dispatch as the lifecycle operator and assert
/// the store's recovery rows. The route calls DomainStore::recover_dispatch;
/// the orphan state, evidence record and row clears are the store's own.
async fn seed_orphan_dispatch(f: &Fixture, engagement: &str, stopped: bool) {
    // Session and task go through the fixture's OWN DomainStore (the running
    // worker holds the SQLite lock, so a second DomainRepository::open would
    // fail Locked). Only the orphan-specific flags are then forced by SQL.
    f.domain
        .register_session(SessionBinding {
            id: "orphan_session".into(),
            engagement_id: engagement.into(),
            room_id: "!project:example.test".into(),
            thread_root: Some("$orphan_thread".into()),
        })
        .await
        .unwrap();
    f.domain
        .create_canonical_task(
            "orphan_task".into(),
            "orphan_session".into(),
            "Orphan work".into(),
            2000,
        )
        .await
        .unwrap();
    let mut db = rusqlite::Connection::open(f.root.path().join("state").join("domain.sqlite3")).unwrap();
    let tx = db.transaction().unwrap();
    tx.execute(
        "UPDATE runner_sessions SET quarantined=1 WHERE id='orphan_session'",
        [],
    )
    .unwrap();
    // The input column must carry the full serialized DispatchInput production
    // enqueue writes (recover_dispatch parses it at execution.rs:1079 and
    // compares resources/payload against the replacement at :1080-1084).
    tx.execute(
        "INSERT INTO runner_dispatches(id,session_id,task_id,input,digest,state) \
         VALUES('orphan_dispatch','orphan_session','orphan_task',\
         '{\"id\":\"orphan_dispatch\",\"session_id\":\"orphan_session\",\"task_id\":\"orphan_task\",\"resources\":[{\"id\":\"orphan_workspace\",\"exclusive\":true}],\"payload\":{\"instruction\":\"original work\"}}',\
         'orphan_digest','outcome_unknown')",
        [],
    ).unwrap();
    tx.execute(
        "INSERT INTO workspace_resources(id,dirty) VALUES('orphan_workspace',1)",
        [],
    ).unwrap();
    tx.execute(
        "INSERT INTO dispatch_resources(dispatch_id,resource_id,exclusive) VALUES('orphan_dispatch','orphan_workspace',1)",
        [],
    ).unwrap();
    tx.execute(
        "INSERT INTO resource_leases(resource_id,dispatch_id,exclusive) VALUES('orphan_workspace','orphan_dispatch',1)",
        [],
    ).unwrap();
    if stopped {
        // Both evidence and settled_at NULL satisfies the CHECK
        // ((evidence IS NULL)=(settled_at IS NULL)); recover_dispatch's stop-row
        // refusal (execution.rs:1044-1050) only needs the row to exist.
        tx.execute(
            "INSERT INTO dispatch_stops(dispatch_id,fence,reason,created_at) VALUES('orphan_dispatch',0,'operator stop',1)",
            [],
        ).unwrap();
    }
    tx.commit().unwrap();
}

fn recovery_body() -> Value {
    json!({
        "original": "orphan_dispatch",
        "replacement": {
            "id": "orphan_replacement",
            "session_id": "orphan_session",
            "task_id": "orphan_task",
            "resources": [{"id":"orphan_workspace","exclusive":true}],
            "payload": {"instruction":"resume inspected orphan"},
        },
        "evidence": "operator inspected workspace and stopped owner",
    })
}

/// A lifecycle operator recovers the orphan: the route reaches recover_dispatch
/// and the store clears the lease/quarantine/dirty, supersedes the orphan and
/// writes the recovery record with the evidence. A read-only session is refused.
#[tokio::test]
async fn native_console_agent_recover_dispatch_recovers_orphan() {
    let f = Fixture::new("127.0.0.1:13300".parse().unwrap(), None);
    let service = f.service();
    let state = f.root.path().join("state");
    seed_orphan_dispatch(&f, &f.engagement, false).await;
    // A read-only ticket cannot recover: the mutation needs Scope::AgentLifecycle.
    let read_only = session(&service).await;
    let refused = post(
        &format!("/console/api/agents/{}/recover-dispatch", f.engagement),
        &read_only,
    )
    .json(&recovery_body())
    .send(&service)
    .await;
    assert_eq!(refused.status_code, Some(StatusCode::FORBIDDEN));
    // Ticket issuance is rate-limited to one per second (authority.rs issued slot).
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    let cookie = lifecycle_session(&service).await;
    let mut response = post(
        &format!("/console/api/agents/{}/recover-dispatch", f.engagement),
        &cookie,
    )
    .json(&recovery_body())
    .send(&service)
    .await;
    let status = response.status_code;
    assert_eq!(status, Some(StatusCode::OK));
    let body = response.take_json::<Value>().await.unwrap();
    assert_eq!(body["ok"], true);
    let db = rusqlite::Connection::open(state.join("domain.sqlite3")).unwrap();
    let leases: u32 = db
        .query_row("SELECT COUNT(*) FROM resource_leases WHERE dispatch_id='orphan_dispatch'", [], |r| r.get(0))
        .unwrap();
    let quarantined: bool = db
        .query_row("SELECT quarantined FROM runner_sessions WHERE id='orphan_session'", [], |r| r.get(0))
        .unwrap();
    let dirty: bool = db
        .query_row("SELECT dirty FROM workspace_resources WHERE id='orphan_workspace'", [], |r| r.get(0))
        .unwrap();
    let replacement_state: String = db
        .query_row("SELECT state FROM runner_dispatches WHERE id='orphan_replacement'", [], |r| r.get(0))
        .unwrap();
    let evidence: String = db
        .query_row("SELECT evidence FROM dispatch_recoveries WHERE original_id='orphan_dispatch'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(leases, 0, "the orphan's lease is deleted");
    assert!(!quarantined, "the session quarantine is cleared");
    assert!(!dirty, "the workspace dirty flag is cleared");
    assert_eq!(
        replacement_state, "queued",
        "the replacement, not the orphan, is enqueued for resume"
    );
    assert_eq!(evidence, "operator inspected workspace and stopped owner");
    f.close().await;
}

/// The stop-row refusal (execution.rs:1044-1050): a dispatch the operator stopped
/// through the conversation-stop flow owns the stop-fenced case; operator
/// recovery refuses it outright, and no row changes.
#[tokio::test]
async fn native_console_agent_recover_dispatch_refuses_stopped_dispatch() {
    let f = Fixture::new("127.0.0.1:13300".parse().unwrap(), None);
    let service = f.service();
    let state = f.root.path().join("state");
    seed_orphan_dispatch(&f, &f.engagement, true).await;
    let cookie = lifecycle_session(&service).await;
    let mut response = post(
        &format!("/console/api/agents/{}/recover-dispatch", f.engagement),
        &cookie,
    )
    .json(&recovery_body())
    .send(&service)
    .await;
    assert_eq!(response.status_code, Some(StatusCode::CONFLICT));
    let body = response.take_json::<Value>().await.unwrap();
    assert_eq!(body["code"], "dispatch_not_recoverable");
    let db = rusqlite::Connection::open(state.join("domain.sqlite3")).unwrap();
    let quarantined: bool = db
        .query_row("SELECT quarantined FROM runner_sessions WHERE id='orphan_session'", [], |r| r.get(0))
        .unwrap();
    let leases: u32 = db
        .query_row("SELECT COUNT(*) FROM resource_leases WHERE dispatch_id='orphan_dispatch'", [], |r| r.get(0))
        .unwrap();
    assert!(quarantined, "the stop-fenced dispatch keeps its quarantine");
    assert_eq!(leases, 1, "the stop-fenced dispatch keeps its lease");
    f.close().await;
}
