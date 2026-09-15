use super::*;

/// The engagements read and document: the same authority matrix the usage
/// console test applies, the read's exact wire shape (including the token
/// column added with the page), pagination, and the document served with the
/// loader's allowlist + key mapping exercised.
#[tokio::test]
async fn native_console_engagements_read() {
    let f = Fixture::new("127.0.0.1:13300".parse().unwrap(), None);
    let service = f.service();
    let anonymous = TestClient::get(format!("{BASE}/console/api/engagements"))
        .add_header("host", "127.0.0.1:13300", true)
        .send(&service)
        .await;
    assert_eq!(anonymous.status_code, Some(StatusCode::UNAUTHORIZED));
    let cookie = session(&service).await;
    for (name, value) in [
        ("host", "evil.test"),
        ("origin", "https://evil.test"),
        ("sec-fetch-site", "cross-site"),
        ("sec-fetch-site", "none"),
        ("x-forwarded-for", "127.0.0.1"),
        ("forwarded", "for=127.0.0.1"),
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
    for query in [
        "?limit=0",
        "?limit=17",
        "?limit=bad",
        "?after=%31",
        "?after=x&after=y",
        "?unknown=1",
    ] {
        let response = get(&format!("/console/api/engagements{query}"), &cookie)
            .send(&service)
            .await;
        assert_eq!(
            response.status_code,
            Some(StatusCode::BAD_REQUEST),
            "{query}"
        );
    }
    // The read's exact shape: the seeded engagement with every wire key,
    // including the requested-tokens column the page renders.
    let mut response = get("/console/api/engagements", &cookie)
        .send(&service)
        .await;
    assert_eq!(response.status_code, Some(StatusCode::OK));
    let value = response.take_json::<Value>().await.unwrap();
    let rows = value["engagements"].as_array().unwrap();
    assert!(!rows.is_empty(), "the fixture's engagements publish");
    let row = &rows[0];
    for key in [
        "id",
        "agentName",
        "projectName",
        "role",
        "requestedTokens",
        "state",
        "cleanup",
    ] {
        assert!(row.get(key).is_some(), "missing wire key {key}");
    }
    assert!(row["requestedTokens"].as_u64().is_some());
    assert!(row["id"].as_str().is_some_and(|id| !id.is_empty()));
    // E4 on the wire: the astral project name. The verifier truncated the
    // 260-character input to 255 Unicode SCALAR values (authority.rs:286) —
    // 510 UTF-16 code units, a name that exceeds a UTF-16 bound but never
    // the scalar one. The client validator counts code points (verified by
    // a node unit against the real validator), so both sides agree.
    let astral = rows
        .iter()
        .find(|r| r["agentName"] == "AlertWorker")
        .expect("the astral-name engagement publishes");
    let name = astral["projectName"].as_str().unwrap();
    assert_eq!(name.chars().count(), 255, "255 Unicode scalar values");
    assert_eq!(name.encode_utf16().count(), 510, "510 UTF-16 units");
    assert!(name.chars().all(|c| c == '𝕏'));
    // E3: the pagination ladder at ?limit=1. The cursor is the LAST-SERVED
    // engagement's server-assigned id (console/usage.rs:78) — an opaque
    // ordering key the store compares lexically (`WHERE id>?1 ORDER BY id`,
    // domain.rs). It names no session, no authority and no resource: a bare
    // cursor grants nothing, and every page re-authorizes through the
    // console session exactly as page one did.
    let mut response = get("/console/api/engagements?limit=1", &cookie)
        .send(&service)
        .await;
    assert_eq!(response.status_code, Some(StatusCode::OK));
    let value = response.take_json::<Value>().await.unwrap();
    let page_one = value["engagements"].as_array().unwrap();
    assert_eq!(page_one.len(), 1, "page one carries exactly one row");
    let first_id = page_one[0]["id"].as_str().unwrap().to_owned();
    let cursor = value["next_after"]
        .as_str()
        .expect("non-null opaque next_after on page one")
        .to_owned();
    assert!(!cursor.is_empty());
    assert_eq!(cursor, first_id, "the cursor is the last-served id");
    let mut seen = vec![first_id];
    let mut cursor = Some(cursor);
    // Walk to exhaustion: three seeded engagements → three one-row pages,
    // then an EMPTY page with a null cursor.
    for expected in 0..4 {
        let Some(after) = cursor.clone() else {
            break;
        };
        let mut response = get(
            &format!("/console/api/engagements?limit=1&after={after}"),
            &cookie,
        )
        .send(&service)
        .await;
        assert_eq!(response.status_code, Some(StatusCode::OK));
        let value = response.take_json::<Value>().await.unwrap();
        let rows = value["engagements"].as_array().unwrap();
        if expected < 2 {
            assert_eq!(rows.len(), 1, "content page {}", expected + 2);
            let id = rows[0]["id"].as_str().unwrap().to_owned();
            assert!(!seen.contains(&id), "each page serves a NEW engagement");
            seen.push(id.clone());
            cursor = value["next_after"].as_str().map(str::to_owned);
        } else {
            assert!(rows.is_empty(), "the page after the last row is empty");
            assert!(
                value["next_after"].is_null(),
                "the empty page carries a null cursor"
            );
            cursor = None;
        }
    }
    assert_eq!(seen.len(), 3, "all three seeded engagements were served");
    f.close().await;
}

/// The document route rule: `/console/engagements/` serves the staged
/// document, takes NO query (the page is a paginated list; selection is
/// in-page), and unknown paths under the console stay refused.
#[tokio::test]
async fn native_console_engagements_document() {
    let f = Fixture::new("127.0.0.1:13300".parse().unwrap(), None);
    let service = f.service();
    let mut response = TestClient::get(format!("{BASE}/console/engagements/"))
        .add_header("host", "127.0.0.1:13300", true)
        .send(&service)
        .await;
    assert_eq!(response.status_code, Some(StatusCode::OK));
    assert!(
        response
            .take_string()
            .await
            .unwrap()
            .contains("engagements document fixture")
    );
    // The no-query rule: any query on the engagements document is refused.
    for path in [
        "/console/engagements/?engagement_id=x",
        "/console/engagements/?anything=1",
    ] {
        let response = TestClient::get(format!("{BASE}{path}"))
            .add_header("host", "127.0.0.1:13300", true)
            .send(&service)
            .await;
        assert_eq!(
            response.status_code,
            Some(StatusCode::BAD_REQUEST),
            "{path}"
        );
    }
    // Non-document assets never relax the origin rule.
    let response = TestClient::get(format!("{BASE}/console/_next/static/x.js"))
        .add_header("host", "127.0.0.1:13300", true)
        .add_header("sec-fetch-site", "cross-site", true)
        .send(&service)
        .await;
    assert_eq!(response.status_code, Some(StatusCode::FORBIDDEN));
    f.close().await;
}

// ---- G10 refusal half: the operator's console verdict route ----

/// Read one engagement's row directly from the fixture's store, plus the
/// decision receipt count for the named command id.
fn engagement_row(
    f: &Fixture,
    id: &str,
    command_id: &str,
) -> (String, serde_json::Value, u64, u64, u64) {
    let sql = rusqlite::Connection::open(f.root.path().join("state/domain.sqlite3")).unwrap();
    let (state, projection): (String, String) = sql
        .query_row(
            "SELECT state,projection FROM engagements WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    let effects: u64 = sql
        .query_row(
            "SELECT COUNT(*) FROM effects WHERE engagement_id=?1",
            [id],
            |r| r.get(0),
        )
        .unwrap();
    let ends: u64 = sql
        .query_row(
            "SELECT COUNT(*) FROM engagement_ends WHERE engagement_id=?1",
            [id],
            |r| r.get(0),
        )
        .unwrap();
    let decisions: u64 = sql
        .query_row(
            "SELECT COUNT(*) FROM decisions WHERE id=?1",
            [command_id],
            |r| r.get(0),
        )
        .unwrap_or(0);
    (
        state,
        serde_json::from_str(&projection).unwrap(),
        effects,
        ends,
        decisions,
    )
}

/// Post a refusal under a lifecycle session.
async fn refuse(service: &Service, cookie: &str, id: &str, command_id: &str) -> Response {
    post(&format!("/console/api/agents/{id}/refuse"), cookie)
        .json(&json!({"commandId": command_id}))
        .send(service)
        .await
}

#[tokio::test]
async fn native_engagement_refuse_pending_is_rejected() {
    let f = Fixture::new("127.0.0.1:13300".parse().unwrap(), None);
    let service = f.service();
    let lifecycle = lifecycle_session(&service).await;
    let pending = f.new_engagement().await;
    let mut response = refuse(&service, &lifecycle, &pending, "refuse_one").await;
    assert_eq!(response.status_code, Some(StatusCode::OK));
    let body: Value = response.take_json().await.unwrap();
    assert_eq!(body["engagement"]["state"], "rejected");
    let (state, projection, effects, ends, _decisions) = engagement_row(&f, &pending, "refuse_one");
    assert_eq!(state, "rejected");
    assert_eq!(projection["state"], "rejected");
    assert_eq!(projection["cleanup"], "not_required");
    assert_eq!(ends, 1, "the refusal stamps the engagement_ends row");
    assert_eq!(effects, 0, "a pending refusal writes no effects row");
    f.close().await;
}

#[tokio::test]
async fn native_engagement_refuse_requires_pending() {
    let f = Fixture::new("127.0.0.1:13300".parse().unwrap(), None);
    let service = f.service();
    let lifecycle = lifecycle_session(&service).await;
    // The fixture's primary engagement is active — any non-pending state must
    // refuse with the conflict word and write nothing.
    let (state, projection, effects, ends, _) = engagement_row(&f, &f.engagement, "refuse_active");
    let mut response = refuse(&service, &lifecycle, &f.engagement, "refuse_active").await;
    assert_eq!(response.status_code, Some(StatusCode::CONFLICT));
    let body: Value = response.take_json().await.unwrap();
    assert_eq!(body["code"], "engagement_not_pending");
    let after = engagement_row(&f, &f.engagement, "refuse_active");
    assert_eq!(after.0, state, "no state write");
    assert_eq!(after.1, projection, "the projection is unchanged");
    assert_eq!(after.2, effects, "no effects row written");
    assert_eq!(after.3, ends, "no engagement_ends row written");
    f.close().await;
}

#[tokio::test]
async fn native_engagement_refuse_replays_the_recorded_decision() {
    let f = Fixture::new("127.0.0.1:13300".parse().unwrap(), None);
    let service = f.service();
    let lifecycle = lifecycle_session(&service).await;
    let pending = f.new_engagement().await;
    let mut first = refuse(&service, &lifecycle, &pending, "refuse_replay").await;
    assert_eq!(first.status_code, Some(StatusCode::OK));
    let one: Value = first.take_json().await.unwrap();
    let mut second = refuse(&service, &lifecycle, &pending, "refuse_replay").await;
    assert_eq!(second.status_code, Some(StatusCode::OK));
    let two: Value = second.take_json().await.unwrap();
    assert_eq!(one, two, "the replay returns the recorded engagement");
    let sql = rusqlite::Connection::open(f.root.path().join("state/domain.sqlite3")).unwrap();
    let decisions: u64 = sql
        .query_row(
            "SELECT COUNT(*) FROM decisions WHERE id='refuse_replay'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(decisions, 1, "one receipt for the replayed refusal");
    let ends: u64 = sql
        .query_row(
            "SELECT COUNT(*) FROM engagement_ends WHERE engagement_id=?1",
            [&pending],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(ends, 1);
    f.close().await;
}

#[tokio::test]
async fn native_engagement_refuse_rejects_a_changed_replay() {
    let f = Fixture::new("127.0.0.1:13300".parse().unwrap(), None);
    let service = f.service();
    let lifecycle = lifecycle_session(&service).await;
    let pending = f.new_engagement().await;
    let first = refuse(&service, &lifecycle, &pending, "refuse_conflict").await;
    assert_eq!(first.status_code, Some(StatusCode::OK));
    // The SAME command id against a DIFFERENT engagement: the stored digest
    // decision_digest("reject", first_id) differs from this one — a conflict,
    // never a second write. `new_engagement` replays its fixed request id (the
    // second call would return the same, now-rejected engagement), so the
    // second subject is one of the seed's other pending engagements.
    let other: String = {
        let sql = rusqlite::Connection::open(f.root.path().join("state/domain.sqlite3")).unwrap();
        sql.query_row(
            "SELECT id FROM engagements WHERE state='pending' AND id<>?1 LIMIT 1",
            [&pending],
            |r| r.get(0),
        )
        .unwrap()
    };
    let mut changed = refuse(&service, &lifecycle, &other, "refuse_conflict").await;
    assert_eq!(changed.status_code, Some(StatusCode::CONFLICT));
    let body: Value = changed.take_json().await.unwrap();
    assert_eq!(body["code"], "decision_conflict");
    let (state, _, _, _, decisions) = engagement_row(&f, &other, "refuse_conflict");
    assert_eq!(state, "pending", "no write on the changed replay");
    assert_eq!(
        decisions, 1,
        "no second receipt under the reused command id"
    );
    f.close().await;
}

#[tokio::test]
async fn native_engagement_refuse_unknown_is_not_found() {
    let f = Fixture::new("127.0.0.1:13300".parse().unwrap(), None);
    let service = f.service();
    let lifecycle = lifecycle_session(&service).await;
    let mut response = refuse(
        &service,
        &lifecycle,
        "engagement_nonexistent",
        "refuse_missing",
    )
    .await;
    assert_eq!(response.status_code, Some(StatusCode::NOT_FOUND));
    let body: Value = response.take_json().await.unwrap();
    assert_eq!(body["code"], "not_found");
    let sql = rusqlite::Connection::open(f.root.path().join("state/domain.sqlite3")).unwrap();
    let decisions: u64 = sql
        .query_row(
            "SELECT COUNT(*) FROM decisions WHERE id='refuse_missing'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(decisions, 0, "an unknown id records no decision");
    f.close().await;
}

#[tokio::test]
async fn native_engagement_refuse_schedules_no_retirement() {
    let f = Fixture::new("127.0.0.1:13300".parse().unwrap(), None);
    let service = f.service();
    let lifecycle = lifecycle_session(&service).await;
    let pending = f.new_engagement().await;
    let response = refuse(&service, &lifecycle, &pending, "refuse_clean").await;
    assert_eq!(response.status_code, Some(StatusCode::OK));
    let sql = rusqlite::Connection::open(f.root.path().join("state/domain.sqlite3")).unwrap();
    let retire_effects: u64 = sql
        .query_row(
            "SELECT COUNT(*) FROM effects WHERE engagement_id=?1 AND kind='retire'",
            [&pending],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(retire_effects, 0, "no retire-kind effect inserted");
    let provision: u64 = sql
        .query_row(
            "SELECT COUNT(*) FROM effects WHERE engagement_id=?1 AND kind='provision'",
            [&pending],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(provision, 0, "no provision effect to touch");
    let (_, projection, _, _, _) = engagement_row(&f, &pending, "refuse_clean");
    assert_eq!(projection["cleanup"], "not_required");
    f.close().await;
}
