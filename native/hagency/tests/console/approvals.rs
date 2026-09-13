use super::*;

/// Seed two owner approvals behind the store's real schema (the console
/// fixture does not stage approvals, and `fixture.rs` is outside this spec's
/// allowed set). The rows carry withheld content — a description holding the
/// owner mxid/room and a config holding tool/preview/card bytes — so the
/// projection's named-column discipline is what keeps them out.
fn seed_approvals(state: &std::path::Path, engagement: &str) {
    let mut db = rusqlite::Connection::open(state.join("domain.sqlite3")).unwrap();
    let tx = db.transaction().unwrap();
    tx.execute(
        "INSERT INTO approval_contexts(id,dispatch_id,fence,engagement_id,digest,config) \
         VALUES('ctx_obs','private_dispatch',0,?1,'obs_digest','{}')",
        [engagement],
    )
    .unwrap();
    let withheld = serde_json::json!({
        "tool": "SECRET_TOOL_NAME",
        "preview": "SECRET_INPUT_PREVIEW_TEXT",
        "card": "SECRET_CARD_DOCUMENT_TEXT",
    })
    .to_string();
    // pending, no choice, reusable: state word only.
    tx.execute(
        "INSERT INTO owner_approvals(id,source_key,context_id,digest,config,scope_key,scope_kind,description,state,choice,grant_id,expires_at) \
         VALUES('oa_pending','src_pending','ctx_obs','obs_digest',?1,'task:echo','task','OWNER @owner:example.test ROOM !private:example.test','pending',NULL,NULL,12000)",
        [&withheld],
    )
    .unwrap();
    // decided, choice once: state and choice words both present.
    tx.execute(
        "INSERT INTO owner_approvals(id,source_key,context_id,digest,config,scope_key,scope_kind,description,state,choice,grant_id,expires_at) \
         VALUES('oa_decided','src_decided','ctx_obs','obs_digest',?1,NULL,NULL,'SECRET_TOOL_NAME','decided','\"once\"',NULL,12000)",
        [&withheld],
    )
    .unwrap();
    tx.commit().unwrap();
}

/// C2b A3: the observation projection omits owner identity and tool detail.
/// Every row carries exactly the seven declared keys with no nested object,
/// and the raw body contains no owner mxid, owner room or tool-name byte.
#[tokio::test]
async fn native_console_approval_observation() {
    let f = Fixture::new("127.0.0.1:13300".parse().unwrap(), None);
    let service = f.service();
    seed_approvals(&f.root.path().join("state"), &f.engagement);
    let cookie = session(&service).await;
    for path in [
        "/console/api/approvals",
        "/console/api/approvals/oa_pending",
        "/console/api/approvals/oa_decided",
    ] {
        let mut response = get(path, &cookie).send(&service).await;
        assert_eq!(response.status_code, Some(StatusCode::OK), "{path}");
        let body = response.take_string().await.unwrap();
        // No withheld byte crosses: owner mxid, owner room, tool name, preview,
        // card or the description's owner/room markers.
        for secret in [
            "@owner:example.test",
            "!private:example.test",
            "SECRET_TOOL_NAME",
            "SECRET_INPUT_PREVIEW_TEXT",
            "SECRET_CARD_DOCUMENT_TEXT",
        ] {
            assert!(!body.contains(secret), "{path} leaked {secret}");
        }
        let value: Value = serde_json::from_str(&body).unwrap();
        // The escaped-prone value class (input-preview and card-document text)
        // is asserted over the DECODED string values, not the raw body: JSON
        // escaping could otherwise let the raw search pass while the decoded
        // text still carries the secret. The metacharacter-free names are the
        // class asserted over raw bytes above.
        fn decoded_strings(value: &Value, out: &mut String) {
            match value {
                Value::String(s) => out.push_str(s),
                Value::Array(items) => items.iter().for_each(|v| decoded_strings(v, out)),
                Value::Object(map) => map.values().for_each(|v| decoded_strings(v, out)),
                _ => {}
            }
        }
        let mut decoded = String::new();
        decoded_strings(&value, &mut decoded);
        for secret in ["SECRET_INPUT_PREVIEW_TEXT", "SECRET_CARD_DOCUMENT_TEXT"] {
            assert!(
                !decoded.contains(secret),
                "{path} decoded a withheld escaped-prone value {secret}"
            );
        }
        let rows: Vec<&Value> = if path.ends_with("/approvals") {
            value["approvals"].as_array().unwrap().iter().collect()
        } else {
            vec![&value]
        };
        assert!(!rows.is_empty(), "{path} served no rows");
        for row in rows {
            let object = row.as_object().unwrap();
            let mut keys = object.keys().collect::<Vec<_>>();
            keys.sort();
            assert_eq!(
                keys,
                vec![
                    "choice",
                    "engagementId",
                    "expiresAt",
                    "id",
                    "projectRoomId",
                    "reusableScope",
                    "state",
                ],
                "{path} widened the projection"
            );
            for (key, value) in object {
                assert!(
                    !value.is_object() && !value.is_array(),
                    "{path} key {key} carries a nested object"
                );
            }
        }
    }
    f.close().await;
}

/// C2b A4: a foreign origin cannot observe approvals. A foreign host is
/// refused at the origin boundary with the console's origin word, and the
/// non-document page refuses a cross-site fetch with the same word — neither
/// serves an approval row.
#[tokio::test]
async fn native_console_approval_refuses_foreign_origin() {
    let f = Fixture::new("127.0.0.1:13300".parse().unwrap(), None);
    let service = f.service();
    seed_approvals(&f.root.path().join("state"), &f.engagement);
    let cookie = session(&service).await;
    let mut response = get("/console/api/approvals", &cookie)
        .add_header("host", "evil.test", true)
        .send(&service)
        .await;
    assert_eq!(response.status_code, Some(StatusCode::FORBIDDEN));
    let value = response.take_json::<Value>().await.unwrap();
    assert_eq!(value["code"], "console_origin_required");
    // The /console/approvals page is a NON-document: a cross-site fetch is
    // refused with the origin word before any asset is served.
    let mut response = TestClient::get(format!("{BASE}/console/approvals"))
        .add_header("host", "127.0.0.1:13300", true)
        .add_header("sec-fetch-site", "cross-site", true)
        .send(&service)
        .await;
    assert_eq!(response.status_code, Some(StatusCode::FORBIDDEN));
    let value = response.take_json::<Value>().await.unwrap();
    assert_eq!(value["code"], "console_origin_required");
    // The foreign-host refusal serves no approval row byte.
    let mut response = get("/console/api/approvals/oa_decided", &cookie)
        .add_header("host", "evil.test", true)
        .send(&service)
        .await;
    assert_eq!(response.status_code, Some(StatusCode::FORBIDDEN));
    assert!(
        !response.take_string().await.unwrap().contains("oa_decided"),
        "the foreign-origin refusal served an approval row"
    );
    f.close().await;
}

/// C2b A5: an approval with no delivery status serves its state and choice
/// words and never a card or preview. The delivery stage is the deferred
/// route's own field and does not exist in the seven-key set.
#[tokio::test]
async fn native_console_approval_undelivered_shows_status_not_a_card() {
    let f = Fixture::new("127.0.0.1:13300".parse().unwrap(), None);
    let service = f.service();
    seed_approvals(&f.root.path().join("state"), &f.engagement);
    let cookie = session(&service).await;
    let mut response = get("/console/api/approvals/oa_decided", &cookie)
        .send(&service)
        .await;
    assert_eq!(response.status_code, Some(StatusCode::OK));
    let body = response.take_string().await.unwrap();
    let value: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(value["state"], "decided", "the state word is served");
    assert_eq!(value["choice"], "once", "the choice word is served");
    // No key could carry card bytes, a preview or a delivery stage word.
    for forbidden in ["card", "preview", "delivery", "stage", "status"] {
        assert!(
            !value.as_object().unwrap().contains_key(forbidden),
            "the seven-key set grew a {forbidden} field"
        );
    }
    assert!(!body.contains("SECRET_CARD_DOCUMENT_TEXT"));
    assert!(!body.contains("SECRET_INPUT_PREVIEW_TEXT"));
    f.close().await;
}

/// C2b A6: a read-only session can observe but cannot decide. Both routes
/// serve under the read-only ticket, and no mutation verdict or consume
/// route exists on the approvals path — the absence is asserted, not assumed.
#[tokio::test]
async fn native_console_approval_observation_is_read_only() {
    let f = Fixture::new("127.0.0.1:13300".parse().unwrap(), None);
    let service = f.service();
    seed_approvals(&f.root.path().join("state"), &f.engagement);
    let cookie = session(&service).await;
    let list = get("/console/api/approvals", &cookie).send(&service).await;
    assert_eq!(list.status_code, Some(StatusCode::OK));
    let single = get("/console/api/approvals/oa_pending", &cookie)
        .send(&service)
        .await;
    assert_eq!(single.status_code, Some(StatusCode::OK));
    // No mutation route exists: POST to the list or single path is not a
    // success, so a read-only session cannot decide or consume through it.
    for path in [
        "/console/api/approvals",
        "/console/api/approvals/oa_pending",
    ] {
        let response = TestClient::post(format!("{BASE}{path}"))
            .add_header("host", "127.0.0.1:13300", true)
            .add_header("sec-fetch-site", "same-origin", true)
            .add_header("cookie", &cookie, true)
            .add_header("origin", BASE, true)
            .json(&json!({"choice": "always"}))
            .send(&service)
            .await;
        assert_ne!(
            response.status_code,
            Some(StatusCode::OK),
            "{path} exposed a mutation route"
        );
        assert_ne!(
            response.status_code,
            Some(StatusCode::CREATED),
            "{path} exposed a mutation route"
        );
    }
    f.close().await;
}
