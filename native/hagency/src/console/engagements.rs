//! The engagement retire surface (ADR-150, spec `task-rust-engagement-retire`):
//! the operator's retirement decision and its cleanup retry, both console
//! mutations under the console's existing `Scope::AgentLifecycle` — the same
//! finite scope the agents routes gate on (`agents.rs:139-160`). The store owns
//! every guarantee (ADR-150 §"keep the store's own guarantees"): the state
//! guard (`domain.rs:1294-1300`), the decision digest and replay
//! (`:1289-1292`, `:359-381`), the same-transaction provision-cancel and
//! retire-schedule (`:1308-1314`), the `engagement_ends` stamp (`:1324-1328`)
//! and the retry's failed-only reset (`:1274-1280`). The route carries the
//! operator's `commandId` verbatim — idempotency is the store's, never the
//! route's — and invents no outcome, no sweep and no timer: there is NO
//! automatic driver anywhere; a failed retirement waits for this operator act.
//!
//! Reads stay scope-free (the engagements read lives in `usage.rs`); only the
//! mutations consult the authority, and the missing-scope word
//! (`agent_lifecycle_scope_required`) is served before any store job, so a
//! read-only session changes nothing. The wire answer is the bounded decision
//! receipt — `id`, `state`, `cleanup` — never the full engagement
//! row and no project, room, resource or token field.
use super::{Error, Session, body, console, failed, recheck};
use crate::{refusal, resources::domain};
use hagency_core::project::{Engagement, identifier};
use salvo::prelude::*;
use serde::Deserialize;

pub(super) fn router() -> Router {
    Router::new()
        .push(Router::with_path("engagements/{id}/retire").post(retire))
        .push(Router::with_path("engagements/{id}/cleanup-retry").post(cleanup_retry))
}

fn store_error(res: &mut Response, error: hagency_store::Error) {
    let (status, code) = match error {
        hagency_store::Error::State => (StatusCode::CONFLICT, "engagement_not_live"),
        hagency_store::Error::Conflict => (StatusCode::CONFLICT, "command_conflict"),
        hagency_store::Error::NotFound => (StatusCode::NOT_FOUND, "not_found"),
        hagency_store::Error::Invalid(_) => (StatusCode::BAD_REQUEST, "invalid_engagement_id"),
        hagency_store::Error::Busy => (StatusCode::SERVICE_UNAVAILABLE, "busy"),
        hagency_store::Error::OutcomeUnknown => (StatusCode::GATEWAY_TIMEOUT, "outcome_unknown"),
        _ => (StatusCode::SERVICE_UNAVAILABLE, "engagement_unavailable"),
    };
    refusal(res, status, code);
}

/// The lifecycle gate shared with the agents mutations: any authenticated
/// session may read, only `Scope::AgentLifecycle` may decide a retirement.
pub(super) fn check_lifecycle(depot: &Depot, res: &mut Response) -> bool {
    let session = match depot.get_typed::<Session>() {
        Ok(session) => session,
        Err(_) => {
            failed(res, Error::Unauthorized);
            return false;
        }
    };
    match console(depot).and_then(|c| c.0.authority.can_lifecycle(session)) {
        Ok(true) => true,
        Ok(false) => {
            failed(res, Error::LifecycleForbidden);
            false
        }
        Err(error) => {
            failed(res, error);
            false
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Command {
    command_id: String,
}

/// Validate path and body, returning (engagement id, command id) or having
/// already served the refusal. The command id is the operator's idempotency
/// key: the store replays it by digest, so a re-POST with the same key and the
/// same act is the retained product's re-POST (backend-v2.js:15233-15234,
/// "The decision is already durable. Retry only detachment"), and a reused key
/// with different content is a conflict — both decided by the store, never by
/// this route.
async fn decision(
    req: &mut Request,
    depot: &Depot,
    res: &mut Response,
) -> Option<(String, String)> {
    if req.uri().query().is_some() {
        failed(res, Error::Invalid);
        return None;
    }
    let id = match req.param::<String>("id") {
        Some(id) => id,
        None => {
            failed(res, Error::Invalid);
            return None;
        }
    };
    if identifier(&id, 128).is_err() {
        failed(res, Error::Invalid);
        return None;
    }
    if !check_lifecycle(depot, res) {
        return None;
    }
    let raw = match body(req, 512).await {
        Ok(raw) => raw,
        Err(_) => {
            failed(res, Error::Invalid);
            return None;
        }
    };
    let input: Command = match serde_json::from_slice(&raw) {
        Ok(input) => input,
        Err(_) => {
            failed(res, Error::Invalid);
            return None;
        }
    };
    if identifier(&input.command_id, 512).is_err() {
        failed(res, Error::Invalid);
        return None;
    }
    Some((id, input.command_id))
}

fn receipt(res: &mut Response, result: Result<Engagement, hagency_store::Error>) {
    match result {
        Ok(engagement) => res.render(Json(serde_json::json!({
            "id": engagement.id,
            "state": engagement.state,
            "cleanup": engagement.cleanup,
        }))),
        Err(error) => store_error(res, error),
    }
}

/// The retirement decision: `DomainStore::revoke` → the store's `end(…, true)`
/// — the single writer of `revoked`. The store refuses an already-terminal
/// engagement (`Error::State`), replays an identical command id by digest, and
/// conflicts on a reused id with different content. The route adds no guard of
/// its own: a weaker route-side check could only diverge from the store's.
#[handler]
async fn retire(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    let Some((id, command)) = decision(req, depot, res).await else {
        return;
    };
    let Some(store) = domain(depot, res) else {
        return;
    };
    let result = store.revoke(command, id).await;
    if let Err(error) = recheck(depot) {
        failed(res, error);
        return;
    }
    receipt(res, result);
}

/// The operator's cleanup retry: `DomainStore::retry_cleanup` resets exactly
/// one failed retire effect to pending with its outcome digest cleared
/// (`domain.rs:1274-1280`). Nothing else may re-drive a failed retirement —
/// no timer, no sweep, no startup pass — so this act is the only path back.
#[handler]
async fn cleanup_retry(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    let Some((id, command)) = decision(req, depot, res).await else {
        return;
    };
    let Some(store) = domain(depot, res) else {
        return;
    };
    let result = store.retry_cleanup(command, id).await;
    if let Err(error) = recheck(depot) {
        failed(res, error);
        return;
    }
    receipt(res, result);
}
