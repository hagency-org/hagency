//! Retained-vs-native console-origin oracle vectors (brief 29, PC-C4 lane).
//!
//! `native/scripts/console-origin-vectors.mjs` EXECUTES the retained proxy
//! (`mockup/app/api/hagency/[...path]/route.js` + `backend-v2.js`) and writes
//! `tests/fixtures/console-origin-vectors.json`, pinned by sha256 (CI `--check`).
//! This selector replays every row through the ACTUAL Salvo console router
//! (`native/hagency/src/console.rs`): `common_authority` (:98) and
//! `same_origin` (:110). Where the native rule is stricter than the retained
//! one, the delta is ASSERTED against the fixture's named divergences
//! (host-header, sec-fetch-site-scope, origin-absent) instead of left
//! implicit. Row URLs are replayed against this fixture's bound authority;
//! an origin equal to the row's own host is rewritten to the bound host so
//! the verdict depends on the rule, not the port.
use super::*;
use serde_json::Value;

fn vectors() -> Value {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/console-origin-vectors.json"
    );
    serde_json::from_str(std::fs::read_to_string(path).unwrap().as_str()).unwrap()
}

fn approval_vectors() -> Value {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../hagency-matrix/tests/fixtures/approval-vectors.json"
    );
    serde_json::from_str(std::fs::read_to_string(path).unwrap().as_str()).unwrap()
}

fn row_parts(row: &Value) -> (Option<String>, Option<String>) {
    let url = row["url"].as_str().unwrap();
    let rest = url.split_once("://").unwrap().1;
    let (row_host, _path) = rest.split_once('/').unwrap();
    let bound = BASE.split_once("://").unwrap().1.to_owned();
    let origin = row["origin"].as_str().map(|origin| {
        if let Some((scheme, host)) = origin.split_once("://")
            && host == row_host
        {
            return format!("{scheme}://{bound}");
        }
        origin.to_owned()
    });
    let site = row["secFetchSite"].as_str().map(str::to_owned);
    (origin, site)
}

/// The native verdict for a one-origin write row: the retained verdict AND the
/// native strictness rules (exactly one `sec-fetch-site: same-origin` on every
/// request; a mutation must carry `origin` equal to the bound authority).
fn native_expects_allowed(row: &Value, origin: &Option<String>, site: &Option<String>) -> bool {
    let retained = row["expected"]["allowed"].as_bool().unwrap();
    let mutation = row["method"].as_str().is_none_or(|m| m != "GET");
    retained
        && site.as_deref() == Some("same-origin")
        && match origin.as_deref() {
            Some(value) => value == format!("http://{}", BASE.split_once("://").unwrap().1),
            None => !mutation,
        }
}

#[tokio::test]
async fn native_console_origin_matches_retained_vectors() {
    let vectors = vectors();
    assert_eq!(
        vectors["source"].as_str().unwrap(),
        "mockup/app/api/hagency/[...path]/route.js + backend-v2.js"
    );
    let fixture = Fixture::new(BASE.split_once("://").unwrap().1.parse().unwrap(), None);
    let service = fixture.service();
    let cookie = session(&service).await;

    // --- one-origin write gate: every retained sameOrigin row, replayed ---
    // The rows name the RETAINED path namespace (`/api/hagency/...`); the
    // native composition routes the same mutation class through its own
    // console API. Only the predicate is under test, so every row replays
    // against one native mutation route with the row's browser headers.
    for row in vectors["sameOrigin"].as_array().unwrap() {
        let (origin, site) = row_parts(row);
        let url = format!(
            "{BASE}/console/api/alerts/oracle_{}/transition",
            row["name"].as_str().unwrap().replace('-', "_")
        );
        let mut request = TestClient::post(url)
            .add_header("host", BASE.split_once("://").unwrap().1, true)
            .add_header("cookie", cookie.clone(), true)
            .json(&json!({"to": "acknowledged"}));
        if let Some(site) = site.as_deref() {
            request = request.add_header("sec-fetch-site", site, true);
        }
        if let Some(origin) = origin.as_deref() {
            request = request.add_header("origin", origin, true);
        }
        // The one-origin predicate refuses through EITHER hoop: the boundary
        // (403 console_origin_required) or the session (401
        // console_access_required — ADR-143's named shape for an absent
        // origin on a mutation). Any other status means the predicate passed
        // and the store answered (the synthetic alert key does not exist).
        let refused = matches!(
            request.send(&service).await.status_code,
            Some(StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED)
        );
        assert_eq!(
            refused,
            !native_expects_allowed(row, &origin, &site),
            "sameOrigin row {} diverged from the composed native verdict",
            row["name"].as_str().unwrap()
        );
    }

    // --- the caller rows: an authorization header on a console route is
    // refused outright (host-header divergence), with or without a session ---
    for row in vectors["caller"].as_array().unwrap() {
        if !row["consoleTokenConfigured"].as_bool().unwrap() {
            // The token-unset rows exercise the retained loopback-bind control,
            // which is structural in native (the bind), not a per-request rule.
            continue;
        }
        let res = TestClient::get(format!("{BASE}/console/api/engagements"))
            .add_header("host", BASE.split_once("://").unwrap().1, true)
            .add_header("sec-fetch-site", "same-origin", true)
            .add_header("cookie", cookie.clone(), true)
            .add_header(
                "authorization",
                row["authorization"].as_str().unwrap_or("Bearer wrong"),
                true,
            )
            .send(&service)
            .await;
        if row["authorization"].as_str().is_some() {
            assert_eq!(
                res.status_code,
                Some(StatusCode::FORBIDDEN),
                "caller row {} carried an authorization header",
                row["name"].as_str().unwrap()
            );
        }
    }

    // --- canonicalisation: traversal never canonicalises ---
    for row in vectors["canonicalisation"].as_array().unwrap() {
        let joined = row["segments"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s.as_str().unwrap())
            .collect::<Vec<_>>()
            .join("/");
        let res = TestClient::get(format!("{BASE}/console/{joined}"))
            .add_header("host", BASE.split_once("://").unwrap().1, true)
            .add_header("sec-fetch-site", "same-origin", true)
            .add_header("cookie", cookie.clone(), true)
            .send(&service)
            .await;
        if row["expected"]["canonical"].is_array() {
            assert_ne!(
                res.status_code,
                Some(StatusCode::BAD_REQUEST),
                "canonical path {joined} was refused as invalid"
            );
        } else {
            assert!(
                [
                    StatusCode::NOT_FOUND,
                    StatusCode::FORBIDDEN,
                    StatusCode::BAD_REQUEST
                ]
                .contains(&res.status_code.unwrap_or(StatusCode::NOT_FOUND)),
                "non-canonical path {joined} was admitted ({:?})",
                res.status_code
            );
        }
    }

    // --- one-origin CORS: the native surface never emits access-control-
    // allow-origin, and a foreign preflight is never answered 204 ---
    for row in vectors["cors"].as_array().unwrap() {
        let origin = row["origin"].as_str();
        let mut request = TestClient::post(format!("{BASE}/api/native/v1/console/access"))
            .add_header("host", BASE.split_once("://").unwrap().1, true)
            .json(&json!({}));
        if row["method"].as_str() == Some("OPTIONS") {
            request = request.add_header("access-control-request-method", "POST", true);
        }
        if let Some(origin) = origin {
            request = request.add_header("origin", origin, true);
        }
        let res = request.send(&service).await;
        assert!(
            res.headers().get("access-control-allow-origin").is_none(),
            "cors row {} received an access-control-allow-origin header",
            row["name"].as_str().unwrap()
        );
        if row["expected"]["answered204"].as_bool().unwrap() {
            assert_ne!(
                res.status_code,
                Some(StatusCode::NO_CONTENT),
                "foreign preflight was answered 204 by the native surface"
            );
        }
    }

    // --- the three named divergences, asserted explicitly ---
    let divergences = vectors["divergences"].as_array().unwrap();
    assert_eq!(
        divergences.len(),
        3,
        "the fixture's divergence list changed"
    );
    for rule in ["host-header", "sec-fetch-site-scope", "origin-absent"] {
        assert!(
            divergences.iter().any(|d| d["rule"] == rule),
            "divergence {rule} missing from the fixture"
        );
    }
    let host = BASE.split_once("://").unwrap().1;
    // host-header: forwarded headers are refused even from the loopback.
    let res = TestClient::get(format!("{BASE}/console/api/engagements"))
        .add_header("host", host, true)
        .add_header("sec-fetch-site", "same-origin", true)
        .add_header("cookie", cookie.clone(), true)
        .add_header("x-forwarded-for", "127.0.0.1", true)
        .send(&service)
        .await;
    assert_eq!(res.status_code, Some(StatusCode::FORBIDDEN));
    // sec-fetch-site-scope: required exactly once on reads too. The refusal
    // surfaces through EITHER hoop — the boundary's 403 or the session's 401
    // `console_access_required` (the shape :97 and :286 already accept); the
    // product rule is untouched.
    let res = TestClient::get(format!("{BASE}/console/api/engagements"))
        .add_header("host", host, true)
        .add_header("cookie", cookie.clone(), true)
        .send(&service)
        .await;
    assert!(
        matches!(
            res.status_code,
            Some(StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED)
        ),
        "sec-fetch-site-scope read refused by the boundary or the session hoop, got {:?}",
        res.status_code
    );
    // origin-absent on a mutation: refused by the console's own session
    // hoop with 401 console_access_required — never 403, which the console
    // reserves for a missing resource scope (the retained rule allowed the
    // request). A REAL mutation route is probed — the earlier revoke probe
    // held only because that route does not exist — so the status and the
    // reason code asserted here are the console's own.
    let res = TestClient::post(format!(
        "{BASE}/console/api/alerts/oracle_origin_absent/transition"
    ))
    .add_header("host", host, true)
    .add_header("sec-fetch-site", "same-origin", true)
    .add_header("cookie", cookie.clone(), true)
    .json(&json!({"to": "acknowledged"}))
    .send(&service)
    .await;
    assert_eq!(res.status_code, Some(StatusCode::UNAUTHORIZED));
    let mut res = res;
    let body = res.take_json::<Value>().await.unwrap();
    assert_eq!(body["ok"], false);
    assert_eq!(body["code"], "console_access_required");
    fixture.close().await;
}

/// The approval lane's client-origin rows, replayed against the same native
/// one-origin predicate the vector script names (`console.rs` `same_origin`):
/// the verdict must agree with the retained `sameOriginWrite` result for
/// every row. The wire-shape (encoder/verdict parser) rows are bound by the
/// store corpus selector `native_approval_wire_corpus`; this selector owns
/// the origin half of `tests/fixtures/approval-vectors.json`.
#[tokio::test]
async fn native_approval_origin_vectors_match_retained_proxy() {
    let vectors = approval_vectors();
    assert_eq!(
        vectors["source"].as_str().unwrap(),
        "bridge-matrix.js + mockup/app/api/hagency/[...path]/route.js"
    );
    let fixture = Fixture::new(BASE.split_once("://").unwrap().1.parse().unwrap(), None);
    let service = fixture.service();
    let cookie = session(&service).await;
    let host = BASE.split_once("://").unwrap().1;
    for row in vectors["origin"].as_array().unwrap() {
        // The row's own origin host (the vectors name 127.0.0.1:3100)
        // replays as the bound authority, exactly as `row_parts` rewrites
        // the sameOrigin rows, so the verdict depends on the rule, not the
        // port. Foreign origins pass through verbatim.
        let origin = row["origin"].as_str().map(|origin| {
            if origin == "http://127.0.0.1:3100" {
                format!("http://{}", BASE.split_once("://").unwrap().1)
            } else {
                origin.to_owned()
            }
        });
        let site = row["secFetchSite"].as_str().map(str::to_owned);
        // Mutations of the verdict endpoint replay against the native
        // mutation route; only the one-origin predicate is under test.
        let url = format!(
            "{BASE}/console/api/alerts/oracle_{}/transition",
            row["name"].as_str().unwrap().replace('-', "_")
        );
        let mut request = TestClient::post(url)
            .add_header("host", host, true)
            .add_header("cookie", cookie.clone(), true)
            .json(&json!({"to": "acknowledged"}));
        if let Some(site) = site.as_deref() {
            request = request.add_header("sec-fetch-site", site, true);
        }
        if let Some(origin) = origin.as_deref() {
            request = request.add_header("origin", origin, true);
        }
        // The composed verdict, not the retained one alone: the native rule
        // is a strict refinement of `sameOriginWrite` (ADR-107 CL-S4′ /
        // ADR-143), so a row the retained proxy allowed is still refused
        // natively when it lacks the mutation's `origin: http://<bound>`.
        // The refusal surfaces through EITHER hoop — the boundary's 403 or
        // the session's 401 `console_access_required` — so both count.
        let refused = matches!(
            request.send(&service).await.status_code,
            Some(StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED)
        );
        assert_eq!(
            refused,
            !native_expects_allowed(row, &origin, &site),
            "approval origin row {} diverged from the composed native verdict",
            row["name"].as_str().unwrap()
        );
    }
    fixture.close().await;
}
