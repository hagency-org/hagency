//! Open ceiling overrun alerts behind the existing operator authentication
//! boundary (ADR-124 slice b). Publication only: an alert is diagnostic and
//! never confers authority; the sweep that writes the rows is in the store.
//! The transition route (ADR-124 amendment) mutates DISPLAY STATE only.
use crate::{
    refusal,
    resources::{body, domain},
};
use hagency_store::{ALERT_STATUSES, AlertTransition, Error, MAX_OPEN_CEILING_ALERTS};
use salvo::prelude::*;
use serde::Deserialize;

pub(crate) fn router() -> Router {
    Router::with_path("alerts")
        .get(list)
        .push(Router::with_path("{key}/transition").post(transition))
}

/// The operator transition body: `to` plus optional display provenance.
/// Notes carry operator text only, bounded at the store.
#[derive(Deserialize)]
struct TransitionBody {
    to: String,
    actor: Option<String>,
    note: Option<String>,
}

/// One operator display-state transition (ADR-124 amendment): the SAME
/// boundary and authority as the read (this router mounts under the
/// `authorize` hoop). A transition never enforces anything — display state
/// only; the refusal words match the list route's vocabulary.
#[handler]
async fn transition(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    let Some(key) = req.param::<String>("key") else {
        refusal(res, StatusCode::BAD_REQUEST, "invalid_alert_transition");
        return;
    };
    if key.is_empty() || key.len() > 256 {
        refusal(res, StatusCode::BAD_REQUEST, "invalid_alert_transition");
        return;
    }
    let Some(store) = domain(depot, res) else {
        return;
    };
    let Some(value) = body::<serde_json::Value>(req, depot, res).await else {
        return;
    };
    let input = match serde_json::from_value::<TransitionBody>(value) {
        Ok(input) => input,
        Err(_) => {
            refusal(res, StatusCode::BAD_REQUEST, "invalid_alert_transition");
            return;
        }
    };
    // `to` must be one of the four states; legality of the PAIR is the
    // store's to refuse (bad_transition) — one map, one owner.
    let Some(to) = ALERT_STATUSES.iter().find(|state| **state == input.to) else {
        refusal(res, StatusCode::BAD_REQUEST, "invalid_alert_transition");
        return;
    };
    // Statement time; a clock fault is the service's own unavailability,
    // never `busy` (the brief-20 E2 rule).
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| u64::try_from(d.as_millis()).ok());
    let Some(now) = now else {
        refusal(res, StatusCode::SERVICE_UNAVAILABLE, "alerts_unavailable");
        return;
    };
    let command = AlertTransition {
        key,
        to,
        actor: input.actor.unwrap_or_default(),
        note: input.note,
        now,
    };
    match store.transition_ceiling_alert(command).await {
        Ok(alert) => res.render(Json(alert)),
        Err(error) => match error {
            Error::Invalid(_) => refusal(res, StatusCode::BAD_REQUEST, "bad_transition"),
            Error::NotFound => refusal(res, StatusCode::NOT_FOUND, "not_found"),
            Error::Busy => refusal(res, StatusCode::SERVICE_UNAVAILABLE, "busy"),
            Error::OutcomeUnknown => refusal(res, StatusCode::GATEWAY_TIMEOUT, "outcome_unknown"),
            _ => refusal(res, StatusCode::SERVICE_UNAVAILABLE, "alerts_unavailable"),
        },
    }
}

fn limit_query(req: &Request) -> Result<u32, ()> {
    if req.uri().query().is_some_and(|q| q.len() > 64) {
        return Err(());
    }
    let fields = req.queries();
    if fields.is_empty() {
        return Ok(100);
    }
    if fields.len() != 1 {
        return Err(());
    }
    let values = fields.get_vec("limit").ok_or(())?;
    if values.len() != 1 || values[0].is_empty() || !values[0].bytes().all(|b| b.is_ascii_digit()) {
        return Err(());
    }
    let limit: u32 = values[0].parse().map_err(|_| ())?;
    // Deliberate divergence from the retained route's clamp
    // (`Math.min(parseInt(limit) || 100, 500)`, alert-store.js:410): an
    // out-of-range limit is refused, not silently clamped, matching every
    // other bounded read behind this boundary.
    if limit == 0 || limit > MAX_OPEN_CEILING_ALERTS as u32 {
        return Err(());
    }
    Ok(limit)
}

#[handler]
async fn list(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    let Some(store) = domain(depot, res) else {
        return;
    };
    let Ok(limit) = limit_query(req) else {
        refusal(res, StatusCode::BAD_REQUEST, "invalid_alerts_query");
        return;
    };
    // The read clock is statement time, mirroring the retained GET /api/alerts
    // (backend-v2.js:16069-16079): no at_ms parameter exists to honor.
    let at_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| u64::try_from(d.as_millis()).ok())
        .unwrap_or_default();
    match store.open_ceiling_alerts(limit).await {
        Ok(alerts) => res.render(Json(serde_json::json!({"at_ms": at_ms, "alerts": alerts}))),
        Err(error) => {
            let (status, code) = match error {
                Error::Invalid(_) => (StatusCode::BAD_REQUEST, "invalid_alerts_query"),
                Error::Schema => (StatusCode::SERVICE_UNAVAILABLE, "alerts_corrupt"),
                Error::Busy => (StatusCode::SERVICE_UNAVAILABLE, "busy"),
                Error::OutcomeUnknown => (StatusCode::GATEWAY_TIMEOUT, "outcome_unknown"),
                _ => (StatusCode::SERVICE_UNAVAILABLE, "alerts_unavailable"),
            };
            refusal(res, status, code);
        }
    }
}
