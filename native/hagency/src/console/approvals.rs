//! Approval observation (ADR-138, PC-C2b) plus the bounded grant-revocation
//! mutation (ADR-043). The two GET routes are read-only: they serve under the
//! `authenticate` hoop with no scope, so a read-only ticket (`mutation: None`,
//! `authority.rs:151-159`) can observe but reaches no mutation. The DELETE
//! grant-revocation route is a mutation and is gated on `Scope::AgentLifecycle`
//! — the console's scope for direct store mutations (start/stop/preset) — via
//! the same `can_lifecycle` check the agents routes use; a read-only ticket is
//! refused with `agent_lifecycle_scope_required` before any store work.
//!
//! The row is exactly seven camelCase keys — `id`, `state`, `choice`,
//! `reusableScope`, `expiresAt`, `engagementId`, `projectRoomId` — drawn from
//! the store's named `SELECT` (`APPROVAL_SELECT`). No nested object, no
//! `description`/`config`/`application`/`observation`, no owner or room
//! column, no card byte, so an undelivered approval can show its `state` and
//! `choice` words and never a card, a preview or a delivery stage.
use super::{Error, Session, console, failed, recheck, usage::query};
use crate::{refusal, resources::domain};
use hagency_core::project::identifier;
use salvo::prelude::*;

pub(super) fn router() -> Router {
    Router::new()
        .push(Router::with_path("approvals").get(list))
        .push(Router::with_path("approvals/{id}").get(single))
        .push(Router::with_path("approvals/grants/{id}").delete(revoke_grant))
}

fn store_error(res: &mut Response, error: hagency_store::Error) {
    let (status, code) = match error {
        hagency_store::Error::Invalid(_) => (StatusCode::BAD_REQUEST, "invalid_approvals_query"),
        hagency_store::Error::NotFound => (StatusCode::NOT_FOUND, "not_found"),
        hagency_store::Error::Busy => (StatusCode::SERVICE_UNAVAILABLE, "busy"),
        hagency_store::Error::OutcomeUnknown => (StatusCode::GATEWAY_TIMEOUT, "outcome_unknown"),
        _ => (StatusCode::SERVICE_UNAVAILABLE, "approvals_unavailable"),
    };
    refusal(res, status, code);
}

#[handler]
async fn list(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    if query(req, &["after", "limit"], 192).is_err() {
        failed(res, Error::Invalid);
        return;
    }
    let after = req.query::<String>("after").unwrap_or_default();
    let limit = match req.query::<String>("limit") {
        None => 16,
        Some(v) if v.bytes().all(|c| c.is_ascii_digit()) => v.parse::<usize>().unwrap_or(0),
        _ => 0,
    };
    if !(1..=16).contains(&limit) || (!after.is_empty() && identifier(&after, 128).is_err()) {
        failed(res, Error::Invalid);
        return;
    }
    let Some(store) = domain(depot, res) else {
        return;
    };
    let result = store.approvals(after, limit + 1).await;
    if let Err(error) = recheck(depot) {
        failed(res, error);
        return;
    }
    match result {
        Ok(mut rows) => {
            let next_after =
                (rows.len() > limit).then(|| rows[limit - 1]["id"].as_str().unwrap().to_owned());
            rows.truncate(limit);
            res.render(Json(
                serde_json::json!({"approvals": rows, "next_after": next_after}),
            ));
        }
        Err(error) => store_error(res, error),
    }
}

#[handler]
async fn single(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    if req.uri().query().is_some() {
        failed(res, Error::Invalid);
        return;
    }
    let Some(id) = req.param::<String>("id") else {
        failed(res, Error::Invalid);
        return;
    };
    let Some(store) = domain(depot, res) else {
        return;
    };
    let result = store.approval(id).await;
    if let Err(error) = recheck(depot) {
        failed(res, error);
        return;
    }
    match result {
        Ok(row) => res.render(Json(row)),
        Err(error) => store_error(res, error),
    }
}

/// G7 (spec `native_owner_approval_grants`, ADR-043): the console revocation
/// route. Revocation only REMOVES authority — it can never grant, decide or
/// consume — so it serves any authenticated operator session with no new
/// `Scope`, the same class as the observation routes above; an anonymous
/// caller is refused at the `authenticate` hoop. The response is the bounded
/// ADR-043 grant projection: the grant id and the revoked word, nothing else.
#[handler]
async fn revoke_grant(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    if req.uri().query().is_some() {
        failed(res, Error::Invalid);
        return;
    }
    let Some(id) = req.param::<String>("id") else {
        failed(res, Error::Invalid);
        return;
    };
    // The mutation gate: a valid session whose grant is `Scope::AgentLifecycle`.
    // A read-only or other-scoped session is refused before any store work.
    let session = match depot.get_typed::<Session>() {
        Ok(session) => session,
        Err(_) => {
            failed(res, Error::Unauthorized);
            return;
        }
    };
    match console(depot).and_then(|c| c.0.authority.can_lifecycle(session)) {
        Ok(true) => {}
        Ok(false) => {
            failed(res, Error::LifecycleForbidden);
            return;
        }
        Err(error) => {
            failed(res, error);
            return;
        }
    }
    let Some(store) = domain(depot, res) else {
        return;
    };
    let result = store.revoke_approval_grant(id.clone()).await;
    if let Err(error) = recheck(depot) {
        failed(res, error);
        return;
    }
    match result {
        Ok(()) => res.render(Json(serde_json::json!({"id": id, "revoked": true}))),
        Err(error) => store_error(res, error),
    }
}
