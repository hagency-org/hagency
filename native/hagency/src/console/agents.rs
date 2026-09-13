//! The read-only agent roster (ADR-126): a bounded observation of the
//! engagement projections, mounted under the API sub-router's
//! `authenticate` hoop with NO scope — the same read class as
//! `engagements` and the resources list: scope facts are a payload on
//! reads, never a gate (only the mutations consult a scope). The wire
//! item carries EXACTLY seven scalar keys derived at the store
//! (`DomainRepository::agent_roster`): no credential home, no workdir,
//! no state dir, no workspace path, no tmux target, no pane buffer, no
//! token — and no nested object at all, so nothing can hide inside one.
//! The client validator refuses an eighth key, so a future widening
//! fails the whole read instead of leaking silently (fail-closed).
//!
//! `last_activity_ms` is "last dispatch activity" — the newest
//! `runner_attempts.created_at` among the engagement's sessions'
//! dispatches — NOT last seen: native has no heartbeat model. An
//! engagement with no attempt row reports `null`, never zero.
//! `unavailable` is server-owned, like the alert transition map: the
//! page renders whatever the server names, so a future source turns a
//! column on by removing its name here, not by a client edit.
use super::{Error, failed, recheck, usage::query};
use crate::{refusal, resources::domain};
use hagency_core::project::EngagementState;
use salvo::prelude::*;
use serde::Serialize;

pub(super) fn router() -> Router {
    Router::with_path("agents").get(list)
}

/// Exactly seven keys, in the ADR-126 order. Every key except `name` is
/// nullable at the source; `null` means "unknown", rendered as such.
#[derive(Serialize)]
struct RosterItem {
    name: String,
    framework: String,
    role: String,
    state: EngagementState,
    engagement_id: String,
    requested_tokens: u64,
    last_activity_ms: Option<u64>,
}

/// Every retained roster column native has no source for in this slice:
/// per-agent consumed usage (the ceiling report is keyed by resource),
/// last-seen/online, the tmux target and pane, the credential home and
/// workspace path (private by omission, named as unavailable), the seat.
const UNAVAILABLE: [&str; 8] = [
    "consumed",
    "last_seen",
    "online",
    "tmux",
    "pane",
    "credential_home",
    "workspace_path",
    "seat",
];

#[handler]
async fn list(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    // A roster takes no selection: any query parameter is refused, the
    // same hygiene the alerts read applies to its own allowlist.
    if query(req, &[], 0).is_err() {
        failed(res, Error::Invalid);
        return;
    }
    let Some(store) = domain(depot, res) else {
        return;
    };
    let result = store.agent_roster().await;
    if let Err(error) = recheck(depot) {
        failed(res, error);
        return;
    }
    match result {
        Ok(rows) => {
            // Statement time, as the alerts read does: the roster has no
            // clock parameter to honor.
            let at_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .ok()
                .and_then(|d| u64::try_from(d.as_millis()).ok())
                .unwrap_or_default();
            let agents: Vec<_> = rows
                .into_iter()
                .map(|row| RosterItem {
                    name: row.name,
                    framework: row.framework,
                    role: row.role,
                    state: row.state,
                    engagement_id: row.engagement_id,
                    requested_tokens: row.requested_tokens,
                    last_activity_ms: row.last_activity_ms,
                })
                .collect();
            res.render(Json(serde_json::json!({
                "at_ms": at_ms,
                "unavailable": UNAVAILABLE,
                "agents": agents,
            })));
        }
        Err(error) => store_error(res, error),
    }
}

fn store_error(res: &mut Response, error: hagency_store::Error) {
    let (status, code) = match error {
        hagency_store::Error::Invalid(_) => (StatusCode::BAD_REQUEST, "invalid_roster_query"),
        hagency_store::Error::Busy => (StatusCode::SERVICE_UNAVAILABLE, "busy"),
        hagency_store::Error::OutcomeUnknown => (StatusCode::GATEWAY_TIMEOUT, "outcome_unknown"),
        _ => (StatusCode::SERVICE_UNAVAILABLE, "roster_unavailable"),
    };
    refusal(res, status, code);
}
