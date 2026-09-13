#[path = "alerts/fixture.rs"]
mod fixture;
use fixture::*;
use hagency::bootstrap::{CeilingSweepTick, start_ceiling_sweep};
use hagency_matrix::CancellationToken;
use salvo::{
    prelude::*,
    test::{ResponseExt, TestClient},
};
use serde_json::{Value, json};
use std::time::Duration;

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

/// The authority matrix of the usage test, applied to the alerts route: the
/// operator boundary is the product's, not this route's invention.
#[tokio::test]
async fn native_alerts_read_requires_operator_authority() {
    let f = Fixture::new(false);
    for token in [None, Some("incorrect")] {
        let request = TestClient::get(f.url()).add_header("host", "127.0.0.1:13300", true);
        let request = match token {
            Some(token) => request.bearer_auth(token),
            None => request,
        };
        let mut response = request.send(&f.service).await;
        assert_eq!(response.status_code, Some(StatusCode::UNAUTHORIZED));
        assert_eq!(
            response.take_json::<Value>().await.unwrap(),
            json!({"ok":false,"code":"operator_auth_required"})
        );
    }
    for (header, value) in [
        ("origin", "https://untrusted.test"),
        ("x-forwarded-for", "127.0.0.1"),
        ("host", "untrusted.test"),
    ] {
        let mut response = TestClient::get(f.url())
            .add_header("host", "127.0.0.1:13300", true)
            .add_header(header, value, true)
            .bearer_auth(TOKEN)
            .send(&f.service)
            .await;
        assert_eq!(response.status_code, Some(StatusCode::FORBIDDEN));
        assert_eq!(
            response.take_json::<Value>().await.unwrap(),
            json!({"ok":false,"code":"local_authority_required"})
        );
    }
    let response = TestClient::post(f.url())
        .add_header("host", "127.0.0.1:13300", true)
        .bearer_auth(TOKEN)
        .json(&json!({"input":999}))
        .send(&f.service)
        .await;
    assert!(matches!(
        response.status_code,
        Some(StatusCode::METHOD_NOT_ALLOWED | StatusCode::NOT_FOUND)
    ));
    // Bounded read: an empty, repeated, non-numeric, zero or above-cap limit
    // is refused exactly the way every other bounded read here refuses.
    for query in [
        "?limit=bad",
        "?limit=0",
        "?limit=201",
        "?limit=1&limit=2",
        "?unknown=1",
        "?at_ms=2000",
        "?limit=99999999999999999999",
    ] {
        let mut response = TestClient::get(format!("{}{}", f.url(), query))
            .add_header("host", "127.0.0.1:13300", true)
            .bearer_auth(TOKEN)
            .send(&f.service)
            .await;
        assert_eq!(
            response.status_code,
            Some(StatusCode::BAD_REQUEST),
            "{query}"
        );
        assert_eq!(
            response.take_json::<Value>().await.unwrap(),
            json!({"ok":false,"code":"invalid_alerts_query"}),
            "{query}"
        );
    }
    f.close().await;
}

/// Publication: every retained field on the row, resolved rows absent, limit
/// respected. The route publishes what the sweep wrote — it never re-derives
/// the draw (which is how the console/headroom divergence bug was born).
#[tokio::test]
async fn native_alerts_read_publishes_open_ceiling_alerts() {
    let f = Fixture::new(true);
    let t0 = now_ms();
    let swept = f.domain.sweep_ceiling_overruns(t0).await.unwrap();
    assert_eq!(swept.raised, 2);
    // Newest-first must be pinned by distinct timestamps, not by a tie. Every
    // sweep refreshes every open-and-over row, so two open rows can only
    // differ when one is skipped: remove pool_a's declared ceiling (unknown is
    // not zero, so its alert stays open, untouched at t0) and sweep again at
    // t1, which refreshes pool_b alone.
    let ceilingless: hagency_core::project::Resource = serde_json::from_value(json!({
        "presetId":"alerts_pool_a","seatId":"alerts_pool_a_seat","framework":"codex",
        "model":"gpt-5.6-sol","reasoning":"medium"
    }))
    .unwrap();
    f.domain.put_resource(ceilingless).await.unwrap();
    let again = f.domain.sweep_ceiling_overruns(t0 + 1_000).await.unwrap();
    assert_eq!((again.raised, again.updated, again.resolved), (0, 1, 0));

    let mut response = TestClient::get(f.url())
        .add_header("host", "127.0.0.1:13300", true)
        .bearer_auth(TOKEN)
        .send(&f.service)
        .await;
    assert_eq!(response.status_code, Some(StatusCode::OK));
    assert_eq!(response.headers().get("cache-control").unwrap(), "no-store");
    let value = response.take_json::<Value>().await.unwrap();
    let text = value.to_string();
    for private in [
        TOKEN,
        "!project:example.test",
        "@owner:example.test",
        "ownerDmRoomId",
    ] {
        assert!(
            !text.contains(private),
            "unexpected private field {private} in alerts read"
        );
    }
    assert!(value["at_ms"].as_u64().unwrap() > 0);
    let alerts = value["alerts"].as_array().unwrap();
    assert_eq!(alerts.len(), 2);
    let alert = &alerts[0];
    let expected_id = resource("alerts_pool_b", "alerts_pool_b_seat", 1).id();
    assert_eq!(alert["resource_id"], expected_id);
    assert_eq!(
        alert["dedupe_key"],
        format!("agent_ceiling_overrun:{expected_id}")
    );
    assert_eq!(alert["resolved"], false);
    // Opened at t0 and refreshed at t1: the repeat rides the same row.
    assert_eq!(alert["occurrences"], 2);
    assert_eq!(alert["last_seen_ms"], t0 + 1_000);
    assert_eq!(alerts[1]["last_seen_ms"], t0);
    assert!(alert["first_seen_ms"].as_u64().unwrap() > 0);
    assert!(alert["last_seen_ms"].as_u64().unwrap() > 0);
    assert!(
        alert["summary"]
            .as_str()
            .unwrap()
            .contains("has drawn 2500000 against a ceiling of 2000000")
    );
    assert!(
        alert["runbook"]
            .as_str()
            .unwrap()
            .contains("raise the ceiling on preset alerts_pool_b")
    );
    assert!(alert["impact"].as_str().unwrap().contains("cannot retract"));
    assert!(
        alert["recovery_condition"]
            .as_str()
            .unwrap()
            .contains("falls back under the ceiling")
    );
    // detail is the PARSED object with the raw numbers, not a string.
    let detail = &alert["detail"];
    assert!(detail.is_object());
    assert_eq!(detail["committedTokens"], 2_500_000);
    assert_eq!(detail["drawnTokens"], 2_500_000);
    assert_eq!(detail["overByTokens"], 500_000);
    assert_eq!(detail["ceilingTokens"], 2_000_000);
    assert_eq!(detail["measuredTokens"], Value::Null);

    // Limit respected: one row for ?limit=1, newest activity first.
    // E3 sequence: t1 raise pool_b → resolve sweep (pool_b gone, pool_a
    // untouched at t1); t2 lower pool_b → reopen sweep (pool_b's
    // last_seen=t2, pool_a's still t0 because it lost its ceiling above).
    // Then the full read must list pool_b FIRST and ?limit=1 exactly pool_b.
    let t1 = t0 + 10_000;
    let t2 = t0 + 20_000;
    f.domain
        .put_resource(resource("alerts_pool_b", "alerts_pool_b_seat", GENEROUS))
        .await
        .unwrap();
    let outcome = f.domain.sweep_ceiling_overruns(t1).await.unwrap();
    assert_eq!(outcome.resolved, 1);
    assert_eq!(outcome.updated, 0, "pool_a is untouched at t1");
    f.domain
        .put_resource(resource("alerts_pool_b", "alerts_pool_b_seat", 2_000_000))
        .await
        .unwrap();
    let outcome = f.domain.sweep_ceiling_overruns(t2).await.unwrap();
    assert_eq!(outcome.raised, 1, "pool_b reopens at t2");
    assert_eq!(outcome.updated, 0, "pool_a is untouched at t2");
    let mut response = TestClient::get(f.url())
        .add_header("host", "127.0.0.1:13300", true)
        .bearer_auth(TOKEN)
        .send(&f.service)
        .await;
    let value = response.take_json::<Value>().await.unwrap();
    let alerts = value["alerts"].as_array().unwrap();
    assert_eq!(alerts.len(), 2);
    assert_eq!(alerts[0]["resource_id"], expected_id, "t2 row is newest");
    assert_eq!(
        alerts[0]["last_seen_ms"].as_u64().unwrap(),
        t2,
        "distinct last_seen pins the order"
    );
    assert_eq!(
        alerts[1]["resource_id"],
        resource("alerts_pool_a", "alerts_pool_a_seat", 1).id()
    );
    assert_eq!(alerts[1]["last_seen_ms"].as_u64().unwrap(), t0);
    let mut response = TestClient::get(format!("{}/?limit=1", f.url()))
        .add_header("host", "127.0.0.1:13300", true)
        .bearer_auth(TOKEN)
        .send(&f.service)
        .await;
    let value = response.take_json::<Value>().await.unwrap();
    assert_eq!(value["alerts"].as_array().unwrap().len(), 1);
    assert_eq!(value["alerts"][0]["resource_id"], expected_id);

    // Resolved alerts are absent: restore the ceiling, sweep, read again.
    f.domain
        .put_resource(resource("alerts_pool_a", "alerts_pool_a_seat", GENEROUS))
        .await
        .unwrap();
    f.domain
        .put_resource(resource("alerts_pool_b", "alerts_pool_b_seat", GENEROUS))
        .await
        .unwrap();
    let outcome = f.domain.sweep_ceiling_overruns(now_ms()).await.unwrap();
    assert_eq!(outcome.resolved, 2);
    let mut response = TestClient::get(f.url())
        .add_header("host", "127.0.0.1:13300", true)
        .bearer_auth(TOKEN)
        .send(&f.service)
        .await;
    let value = response.take_json::<Value>().await.unwrap();
    assert_eq!(
        value["alerts"].as_array().unwrap().len(),
        0,
        "resolved alerts must not be published"
    );
    f.close().await;
}

/// B5: the 7-day prune, all three sides — a resolved row older than the TTL
/// is pruned, a younger one is kept, an open row is never pruned — with the
/// `pruned` count asserted. All sweeps go through the fixture's own store
/// wrapper (one writer); the only second connection is read/update SQL.
#[tokio::test]
async fn native_ceiling_alert_prunes_resolved_rows_after_seven_days() {
    let f = Fixture::new(false);
    // The fixture seeded pool_a over (committed 1.5M, ceiling lowered 1M).
    let outcome = f.domain.sweep_ceiling_overruns(1_000_000).await.unwrap();
    assert_eq!(outcome.raised, 1);
    // Restore: the next sweep resolves it.
    f.domain
        .put_resource(resource("alerts_pool_a", "alerts_pool_a_seat", GENEROUS))
        .await
        .unwrap();
    let outcome = f.domain.sweep_ceiling_overruns(2_000_000).await.unwrap();
    assert_eq!(outcome.resolved, 1);
    // Backdate the resolution 8+ days before the final sweep's clock.
    let connection = rusqlite::Connection::open(&f.state).unwrap();
    connection
        .execute("UPDATE ceiling_alerts SET resolved_at_ms=?1", [1000u64])
        .unwrap();
    let count: i64 = connection
        .query_row("SELECT COUNT(*) FROM ceiling_alerts", [], |r| r.get(0))
        .unwrap();
    drop(connection);
    assert_eq!(count, 1);
    let cutoff_sweep = 1000 + 7 * 24 * 60 * 60 * 1000 + 1;
    let outcome = f.domain.sweep_ceiling_overruns(cutoff_sweep).await.unwrap();
    assert_eq!(outcome.pruned, 1);
    let connection = rusqlite::Connection::open(&f.state).unwrap();
    let count: i64 = connection
        .query_row("SELECT COUNT(*) FROM ceiling_alerts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0, "the old resolved row is gone");
    // A young resolved row is kept: over again, then resolved just now.
    f.domain
        .put_resource(resource("alerts_pool_a", "alerts_pool_a_seat", 1_000_000))
        .await
        .unwrap();
    let outcome = f
        .domain
        .sweep_ceiling_overruns(cutoff_sweep + 1)
        .await
        .unwrap();
    assert_eq!(outcome.raised, 1);
    f.domain
        .put_resource(resource("alerts_pool_a", "alerts_pool_a_seat", GENEROUS))
        .await
        .unwrap();
    let outcome = f
        .domain
        .sweep_ceiling_overruns(cutoff_sweep + 2)
        .await
        .unwrap();
    assert_eq!(outcome.resolved, 1);
    let outcome = f
        .domain
        .sweep_ceiling_overruns(cutoff_sweep + 3)
        .await
        .unwrap();
    assert_eq!(outcome.pruned, 0, "a young resolved row is kept");
    // An open row is never pruned, however old: over again and leave it open.
    f.domain
        .put_resource(resource("alerts_pool_a", "alerts_pool_a_seat", 1_000_000))
        .await
        .unwrap();
    let outcome = f
        .domain
        .sweep_ceiling_overruns(cutoff_sweep + 4)
        .await
        .unwrap();
    assert_eq!(outcome.raised, 1);
    let outcome = f
        .domain
        .sweep_ceiling_overruns(cutoff_sweep + 40 * 24 * 60 * 60 * 1000)
        .await
        .unwrap();
    assert_eq!(outcome.pruned, 0, "an open row is never pruned");
    assert_eq!(outcome.updated, 1);
    let connection = rusqlite::Connection::open(&f.state).unwrap();
    let (count, resolved): (i64, i64) = connection
        .query_row(
            "SELECT COUNT(*), resolved_at_ms IS NULL FROM ceiling_alerts",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((count, resolved), (1, 1));
    drop(connection);
    f.close().await;
}

/// B7/§3.4: a resource that LOSES its declared ceiling keeps its alert open —
/// Node behaves the same way (`backend-v2.js:9397-9400` `continue`s on a
/// missing ceiling, so auto-resolve is never reached and the alert persists
/// until an operator closes it through the alert surface). Native slices
/// (a)/(b) have no operator close path either, so the parity — no crash, no
/// resolve, no prune, row stays open — is the pinned behaviour, not a defect
/// to paper over with an invented auto-resolve the retained code lacks.
#[tokio::test]
async fn native_ceiling_alert_lost_ceiling_keeps_alert_open() {
    let f = Fixture::new(false);
    let outcome = f.domain.sweep_ceiling_overruns(1_000_000).await.unwrap();
    assert_eq!(outcome.raised, 1);
    // Remove the declared ceiling entirely (resource stays otherwise valid).
    let ceilingless: hagency_core::project::Resource = serde_json::from_value(serde_json::json!({
        "presetId": "alerts_pool_a",
        "seatId": "alerts_pool_a_seat",
        "framework": "codex",
        "model": "gpt-5.6-sol",
        "reasoning": "medium"
    }))
    .unwrap();
    f.domain.put_resource(ceilingless).await.unwrap();
    let outcome = f.domain.sweep_ceiling_overruns(2_000_000).await.unwrap();
    assert_eq!(
        outcome,
        hagency_store::SweepOutcome::default(),
        "no ceiling is unknown, not zero: skipped for raise AND resolve"
    );
    let connection = rusqlite::Connection::open(&f.state).unwrap();
    let (count, resolved): (i64, i64) = connection
        .query_row(
            "SELECT COUNT(*), resolved_at_ms IS NULL FROM ceiling_alerts",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        (count, resolved),
        (1, 1),
        "the alert stays open, as in Node"
    );
    drop(connection);
    f.close().await;
}
/// while another connection holds the SQLite write lock, and the loop alive
/// and sweeping again after release — awaited on the watch hook, never by
/// sleeping. Refusal handling is the loop's contract: log with the `[ceiling]`
/// prefix and wait for the next tick; never an in-line retry, never blocking
/// admission traffic.
#[tokio::test]
async fn native_alert_sweep_runs_hourly_and_survives_busy() {
    let f = Fixture::with_capacity(false, 1);
    let cancel = CancellationToken::new();
    let (handle, mut observed) =
        start_ceiling_sweep(f.domain.clone(), cancel.clone(), Duration::from_millis(50));
    async fn until<F>(observed: &mut tokio::sync::watch::Receiver<CeilingSweepTick>, ok: F)
    where
        F: Fn(&CeilingSweepTick) -> bool,
    {
        loop {
            if ok(&observed.borrow()) {
                return;
            }
            tokio::time::timeout(Duration::from_secs(10), observed.changed())
                .await
                .expect("tick observation timed out")
                .expect("sweep loop dropped its watch");
        }
    }
    // One tick observed sweeping the seeded overrun.
    until(&mut observed, |tick| {
        matches!(tick, CeilingSweepTick::Swept(_))
    })
    .await;
    let first = observed.borrow().clone();
    let CeilingSweepTick::Swept(outcome) = first else {
        unreachable!()
    };
    assert_eq!(outcome.raised, 1);

    // E4: refuse with the STORE's own `Error::Busy`, not the SQLite busy
    // timeout's catch-all. The seam (review §3): `Error::Busy` is the mpsc
    // `try_send` refusal when the queue is full — reachable here by holding
    // the SQLite write lock (the writer grinds inside a job for its 100 ms
    // busy timeout) while a second job occupies the capacity-1 queue, so the
    // loop's tick finds the queue full. The window is ~100 ms per attempt,
    // so the maneuver retries within a bounded budget rather than trusting
    // one shot; the assertion is exact — `Refused("busy")` — so a loop that
    // only ever produced the Sqlite catch-all `failed` would time out and
    // fail. (`OutcomeUnknown` needs a writer parked past the 2 s reply
    // bound; the only such seam — `Probe::paused` — is `#[cfg(test)]`-
    // private to hagency-store, so that arm stays for the store-side suite.)
    // One queued job per attempt gave the tick a ~100 ms window and missed
    // it on a hosted macOS runner; instead a few submitters keep the single
    // queue slot refilled for the whole attempt while the external lock
    // holds the writer, so every tick in the window finds the queue full.
    let mut saw_busy = false;
    for _ in 0..5 {
        let lock = rusqlite::Connection::open(&f.state).unwrap();
        lock.execute_batch("BEGIN IMMEDIATE").unwrap();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let hammers: Vec<_> = (0..4)
            .map(|_| {
                let store = f.domain.clone();
                let stop = stop.clone();
                tokio::spawn(async move {
                    while !stop.load(std::sync::atomic::Ordering::Acquire) {
                        let filler = resource("alerts_pool_a", "alerts_pool_a_seat", 1_000_000);
                        let _ = store.put_resource(filler).await;
                        tokio::task::yield_now().await;
                    }
                })
            })
            .collect();
        let seen = tokio::time::timeout(Duration::from_secs(3), async {
            until(&mut observed, |tick| {
                matches!(tick, CeilingSweepTick::Refused("busy"))
            })
            .await;
        })
        .await;
        stop.store(true, std::sync::atomic::Ordering::Release);
        lock.execute_batch("COMMIT").unwrap();
        drop(lock);
        for hammer in hammers {
            let _ = hammer.await;
        }
        if seen.is_ok() {
            saw_busy = true;
            break;
        }
    }
    assert!(
        saw_busy,
        "the store's own Error::Busy arm was never observed"
    );
    let refusal = observed.borrow().clone();
    assert_eq!(refusal, CeilingSweepTick::Refused("busy"));
    assert!(
        !handle.is_finished(),
        "the sweep loop must survive a refusal"
    );

    // Release: the next tick sweeps again — recovery, not a restart.
    until(&mut observed, |tick| {
        matches!(tick, CeilingSweepTick::Swept(_))
    })
    .await;
    let recovered = observed.borrow().clone();
    let CeilingSweepTick::Swept(outcome) = recovered else {
        unreachable!()
    };
    assert_eq!(outcome.updated, 1, "the open row rides the repeat counter");

    cancel.cancel();
    handle.await.unwrap();
    f.close().await;
}

/// The operator transition route (ADR-124 amendment): the read's own
/// authority matrix, its refusal words, and the success shape — the bare
/// row the store returns, one legal walk to terminal, and every refusal
/// named. Bearer plus local authority exactly as the read.
#[tokio::test]
async fn native_ceiling_alert_transition_route_authority() {
    let f = Fixture::new(false);
    let swept = f.domain.sweep_ceiling_overruns(now_ms()).await.unwrap();
    assert_eq!(swept.raised, 1);
    let transition = format!("{}/{{}}/transition", f.url());
    let post = |path: &str, token: Option<&str>, body: Value| {
        let request = TestClient::post(path)
            .add_header("host", "127.0.0.1:13300", true)
            .add_header("content-type", "application/json", true);
        let request = match token {
            Some(token) => request.bearer_auth(token),
            None => request,
        };
        request.json(&body)
    };
    // The authority matrix: anonymous and forged are 401 before any store
    // read; a foreign header is 403 — the boundary is the read's, not the
    // route's invention.
    for token in [None, Some("incorrect")] {
        let response = post(&transition, token, json!({"to": "acknowledged"}))
            .send(&f.service)
            .await;
        assert_eq!(response.status_code, Some(StatusCode::UNAUTHORIZED));
    }
    let response = TestClient::post(&transition)
        .add_header("host", "127.0.0.1:13300", true)
        .add_header("x-forwarded-for", "127.0.0.1", true)
        .bearer_auth(TOKEN)
        .json(&json!({"to": "acknowledged"}))
        .send(&f.service)
        .await;
    assert_eq!(response.status_code, Some(StatusCode::FORBIDDEN));
    // An unknown key is a named 404.
    let response = post(
        &format!("{}/agent_ceiling_overrun:missing/transition", f.url()),
        Some(TOKEN),
        json!({"to": "acknowledged"}),
    )
    .send(&f.service)
    .await;
    assert_eq!(response.status_code, Some(StatusCode::NOT_FOUND));
    // The seeded key, from the read the operator uses.
    let mut response = TestClient::get(f.url())
        .add_header("host", "127.0.0.1:13300", true)
        .bearer_auth(TOKEN)
        .send(&f.service)
        .await;
    let value = response.take_json::<Value>().await.unwrap();
    let key = value["alerts"][0]["dedupe_key"]
        .as_str()
        .unwrap()
        .to_owned();
    let seeded = format!("{}/{key}/transition", f.url());
    // A malformed body and an unknown state word are invalid.
    for body in [json!({}), json!({"to": "assigned"}), json!({"to": ""})] {
        let response = post(&seeded, Some(TOKEN), body).send(&f.service).await;
        assert_eq!(response.status_code, Some(StatusCode::BAD_REQUEST));
    }
    // One legal walk to terminal: acknowledge, suppress, resolve — the row
    // itself comes back, display state and provenance asserted per hop.
    for (to, note) in [
        ("acknowledged", Some("seen")),
        ("suppressed", None),
        ("resolved", Some("closed by operator")),
    ] {
        let body = match note {
            Some(note) => json!({"to": to, "note": note, "actor": "operator"}),
            None => json!({"to": to}),
        };
        let mut response = post(&seeded, Some(TOKEN), body).send(&f.service).await;
        assert_eq!(response.status_code, Some(StatusCode::OK));
        let alert = response.take_json::<Value>().await.unwrap();
        assert_eq!(alert["status"], to);
        assert_eq!(alert["note"].as_str(), note);
        assert_eq!(alert["transitioned_by"], "operator");
        assert_eq!(alert["dedupe_key"], key);
    }
    // Terminal: every further pair is the store's own bad_transition word.
    let response = post(&seeded, Some(TOKEN), json!({"to": "open"}))
        .send(&f.service)
        .await;
    assert_eq!(response.status_code, Some(StatusCode::BAD_REQUEST));
    let mut response = response;
    let body = response.take_json::<Value>().await.unwrap();
    assert_eq!(body["code"], "bad_transition");
    // A note over the bound never reaches the store's UPDATE.
    let response = post(
        &seeded,
        Some(TOKEN),
        json!({"to": "acknowledged", "note": "x".repeat(2049)}),
    )
    .send(&f.service)
    .await;
    assert_eq!(response.status_code, Some(StatusCode::BAD_REQUEST));
}
