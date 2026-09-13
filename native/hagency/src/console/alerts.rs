//! Open ceiling alerts for the native console (ADR-124 amendment: the console
//! consumer) and the operator TRANSITION route the same amendment adds:
//! display state only, never enforcement. The legal-transition map is
//! SERVED — every row carries `next`, and the page renders buttons only
//! from it, so a route the server does not serve can never appear as a
//! control. Mirrors `usage.rs`: recheck after the store answers, the same
//! store-error mapping shape, no-store (from the boundary), statement-time
//! `at_ms`.
//!
//! `severity` and `status` are DERIVED, never stored: every
//! `agent_ceiling_overrun` ingest is a `warning` (`backend-v2.js:9422-9447`
//! passes `severity: 'warning'`), and this read is open-rows-only by
//! construction (`resolved_at_ms IS NULL`), so `status` is always `"open"`.
use super::{Error, body, failed, recheck, usage::query};
use crate::{refusal, resources::domain};
use hagency_store::{
    ALERT_STATUSES, AlertTransition, CeilingAlert, MAX_OPEN_CEILING_ALERTS, allowed_transitions,
};
use salvo::prelude::*;
use serde::Serialize;

pub(super) fn router() -> Router {
    Router::with_path("alerts")
        .get(list)
        .push(Router::with_path("{key}/transition").post(transition))
}

/// The wire item: the store's `CeilingAlert` plus the two derived fields.
/// Exactly these thirteen keys — the client validator's exact-key list must
/// match this set or the page never reaches ready.
#[derive(Serialize)]
struct ConsoleAlert {
    dedupe_key: String,
    resource_id: String,
    summary: String,
    detail: serde_json::Value,
    runbook: String,
    impact: String,
    recovery_condition: String,
    occurrences: u64,
    first_seen_ms: u64,
    last_seen_ms: u64,
    resolved: bool,
    severity: &'static str,
    status: String,
    next: Vec<&'static str>,
    note: Option<String>,
}

/// The row builder BOTH handlers render through — one wire shape, `next`
/// derived from the one server-owned map.
fn console_alert(alert: CeilingAlert) -> ConsoleAlert {
    ConsoleAlert {
        severity: "warning",
        status: alert.status.clone(),
        next: allowed_transitions(&alert.status).to_vec(),
        note: alert.note,
        resolved: alert.resolved,
        dedupe_key: alert.dedupe_key,
        resource_id: alert.resource_id,
        summary: alert.summary,
        detail: alert.detail,
        runbook: alert.runbook,
        impact: alert.impact,
        recovery_condition: alert.recovery_condition,
        occurrences: alert.occurrences,
        first_seen_ms: alert.first_seen_ms,
        last_seen_ms: alert.last_seen_ms,
    }
}

/// The operator transition body: `to` plus optional display provenance.
#[derive(serde::Deserialize)]
struct TransitionBody {
    to: String,
    actor: Option<String>,
    note: Option<String>,
}

/// One display-state transition through the console session. Refusals are
/// named with the alerts vocabulary; `bad_transition` stays the store's own
/// word for an illegal pair.
#[handler]
async fn transition(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    let Some(key) = req.param::<String>("key") else {
        failed(res, Error::Invalid);
        return;
    };
    if key.is_empty() || key.len() > 256 || query(req, &[], 0).is_err() {
        failed(res, Error::Invalid);
        return;
    }
    let headers_ok = req.headers().get_all("content-type").iter().count() == 1
        && req
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(';').next())
            .map(str::trim)
            == Some("application/json");
    let parsed = if headers_ok {
        serde_json::from_slice::<TransitionBody>(&body(req, 256).await.unwrap_or_default()).ok()
    } else {
        None
    };
    let Some(input) = parsed else {
        failed(res, Error::Invalid);
        return;
    };
    let Some(to) = ALERT_STATUSES.iter().find(|state| **state == input.to) else {
        failed(res, Error::Invalid);
        return;
    };
    let Some(store) = domain(depot, res) else {
        return;
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| u64::try_from(d.as_millis()).ok())
        .unwrap_or_default();
    let result = store
        .transition_ceiling_alert(AlertTransition {
            key: key.clone(),
            to,
            actor: input.actor.unwrap_or_default(),
            note: input.note,
            now,
        })
        .await;
    if let Err(error) = recheck(depot) {
        failed(res, error);
        return;
    }
    match result {
        Ok(alert) => res.render(Json(serde_json::json!({
            "at_ms": now,
            "alerts": [console_alert(alert)],
        }))),
        Err(hagency_store::Error::Invalid(_)) => {
            refusal(res, StatusCode::BAD_REQUEST, "bad_transition")
        }
        Err(hagency_store::Error::NotFound) => refusal(res, StatusCode::NOT_FOUND, "not_found"),
        Err(error) => store_error(res, error),
    }
}

fn store_error(res: &mut Response, error: hagency_store::Error) {
    let (status, code) = match error {
        hagency_store::Error::Invalid(_) => (StatusCode::BAD_REQUEST, "invalid_alerts_query"),
        hagency_store::Error::Busy => (StatusCode::SERVICE_UNAVAILABLE, "busy"),
        hagency_store::Error::OutcomeUnknown => (StatusCode::GATEWAY_TIMEOUT, "outcome_unknown"),
        hagency_store::Error::Schema => (StatusCode::SERVICE_UNAVAILABLE, "alerts_corrupt"),
        _ => (StatusCode::SERVICE_UNAVAILABLE, "alerts_unavailable"),
    };
    refusal(res, status, code);
}

#[handler]
async fn list(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    // Same query hygiene as the usage reads: one optional `limit`, no
    // encoded payloads, bounded query length.
    if query(req, &["limit"], 64).is_err() {
        failed(res, Error::Invalid);
        return;
    }
    // Default 100 (the retained listAlerts default), refused outside
    // 1..=200 — the documented clamp-vs-refuse divergence (ADR-124).
    let limit = match req.query::<String>("limit") {
        None => 100usize,
        Some(v) if v.bytes().all(|c| c.is_ascii_digit()) => v.parse::<usize>().unwrap_or(0),
        _ => 0,
    };
    if limit == 0 || limit > MAX_OPEN_CEILING_ALERTS {
        failed(res, Error::Invalid);
        return;
    }
    let Some(store) = domain(depot, res) else {
        return;
    };
    let result = store.open_ceiling_alerts(limit as u32).await;
    if let Err(error) = recheck(depot) {
        failed(res, error);
        return;
    }
    match result {
        Ok(rows) => {
            // Statement time, as the operator route does: the retained
            // GET /api/alerts has no clock parameter to honor.
            let at_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .ok()
                .and_then(|d| u64::try_from(d.as_millis()).ok())
                .unwrap_or_default();
            let alerts: Vec<_> = rows.into_iter().map(console_alert).collect();
            res.render(Json(serde_json::json!({"at_ms": at_ms, "alerts": alerts})));
        }
        Err(error) => store_error(res, error),
    }
}
