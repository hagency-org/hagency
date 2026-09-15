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
use super::resources::failure;
use super::{Error, Session, body, console, failed, recheck, usage::query};
use crate::{refusal, resources::domain};
use hagency_core::project::{EngagementState, identifier};
use salvo::prelude::*;
use serde::{Deserialize, Serialize};

pub(super) fn router() -> Router {
    Router::with_path("agents")
        .get(list)
        .push(Router::with_path("{id}/start").post(start))
        .push(Router::with_path("{id}/stop").post(stop))
        .push(Router::with_path("{id}/preset").post(preset))
        .push(Router::with_path("{id}/recover-dispatch").post(recover_dispatch))
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
            // CL-S2 (ADR-130): the lifecycle controls render ONLY from the
            // served boolean — a read-only session renders none enabled.
            let manage_lifecycle = match depot.get_typed::<Session>() {
                Ok(session) => {
                    match console(depot).and_then(|c| c.0.authority.can_lifecycle(session)) {
                        Ok(value) => value,
                        Err(error) => {
                            failed(res, error);
                            return;
                        }
                    }
                }
                Err(_) => {
                    failed(res, Error::Unauthorized);
                    return;
                }
            };
            res.render(Json(serde_json::json!({
                "at_ms": at_ms,
                "unavailable": UNAVAILABLE,
                "agents": agents,
                "permissions": {"manageLifecycle": manage_lifecycle},
            })));
        }
        Err(error) => store_error(res, error),
    }
}

/// The engagement id path parameter — an opaque identifier, never a path
/// component that can address a session, authority or filesystem.
fn engagement_id(req: &Request) -> Result<String, Error> {
    let id = req.param::<String>("id").ok_or(Error::Invalid)?;
    identifier(&id, 128).map_err(|_| Error::Invalid)?;
    Ok(id)
}

/// The three lifecycle routes share one scope gate: a valid session whose
/// grant is `Scope::AgentLifecycle`. A read-only or other-scoped session is
/// refused with `agent_lifecycle_scope_required` before any store work.
fn check_lifecycle(depot: &Depot, res: &mut Response) -> bool {
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

/// Start — an at-most-once ensure over the engagement's own state word,
/// never a process birth (ADR-053's fixed launcher). An engagement that is
/// already `active` is live and refuses with a named word; anything else
/// reports the ensure held and spawns nothing.
#[handler]
async fn start(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    if query(req, &[], 0).is_err() {
        failed(res, Error::Invalid);
        return;
    }
    let id = match engagement_id(req) {
        Ok(id) => id,
        Err(error) => {
            failed(res, error);
            return;
        }
    };
    if !check_lifecycle(depot, res) {
        return;
    }
    let Some(store) = domain(depot, res) else {
        return;
    };
    let result = async {
        let rows = store.agent_roster().await?;
        let row = rows
            .into_iter()
            .find(|r| r.engagement_id == id)
            .ok_or(hagency_store::Error::NotFound)?;
        if row.state == EngagementState::Active {
            return Err(hagency_store::Error::State);
        }
        Ok(())
    }
    .await;
    if let Err(error) = recheck(depot) {
        failed(res, error);
        return;
    }
    match result {
        Ok(()) => res.render(Json(serde_json::json!({"ok": true}))),
        Err(hagency_store::Error::State) => {
            refusal(res, StatusCode::CONFLICT, "agent_already_live")
        }
        Err(hagency_store::Error::NotFound) => refusal(res, StatusCode::NOT_FOUND, "not_found"),
        Err(error) => store_error(res, error),
    }
}

/// Stop — fence, never settle: the store resolves the named engagement's
/// dispatch (live set or unsettled stop row) and serves the five-key wire
/// object verbatim. Idempotent by construction (see `stop_dispatch_for_agent`).
#[handler]
async fn stop(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    if query(req, &[], 0).is_err() {
        failed(res, Error::Invalid);
        return;
    }
    let id = match engagement_id(req) {
        Ok(id) => id,
        Err(error) => {
            failed(res, error);
            return;
        }
    };
    if !check_lifecycle(depot, res) {
        return;
    }
    let Some(store) = domain(depot, res) else {
        return;
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| u64::try_from(d.as_millis()).ok())
        .unwrap_or_default();
    let result = store.stop_dispatch_for_agent(id, now).await;
    if result.is_ok() && recheck(depot).is_err() {
        failure(res, hagency_store::Error::OutcomeUnknown);
        return;
    }
    match result {
        Ok(value) => res.render(Json(value)),
        Err(error) => failure(res, error),
    }
}

/// Recover-dispatch — operator recovery and resume of an orphaned dispatch
/// (ADR-148). The operator names the crashed dispatch's replacement and the
/// evidence of what was inspected; the store enforces the orphan state, the
/// stop-row refusal and the evidence record. Triggered ONLY by this route,
/// under the existing `Scope::AgentLifecycle` — never automatic, never a sweep,
/// and no second clearer over the stop-fenced sibling's state.
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct RecoverDispatch {
    original: String,
    replacement: hagency_core::tasks::DispatchInput,
    evidence: String,
}

#[handler]
async fn recover_dispatch(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    if query(req, &[], 0).is_err() {
        failed(res, Error::Invalid);
        return;
    }
    // Shape-validation only: the path names the agent whose dispatch is
    // recovered, keeping the agents surface's path hygiene; the store call
    // itself is keyed by the body's `original` dispatch id.
    if let Err(error) = engagement_id(req) {
        failed(res, error);
        return;
    }
    if !check_lifecycle(depot, res) {
        return;
    }
    let raw = match body(req, 8192).await {
        Ok(raw) => raw,
        Err(_) => {
            failed(res, Error::Invalid);
            return;
        }
    };
    let input: RecoverDispatch = match serde_json::from_slice(&raw) {
        Ok(input) => input,
        Err(_) => {
            failed(res, Error::Invalid);
            return;
        }
    };
    if input.replacement.validate().is_err()
        || identifier(&input.original, 128).is_err()
        || input.evidence.is_empty()
        || input.evidence.chars().count() > 4096
    {
        failed(res, Error::Invalid);
        return;
    }
    let Some(store) = domain(depot, res) else {
        return;
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| u64::try_from(d.as_millis()).ok())
        .unwrap_or_default();
    let result = store
        .recover_dispatch(
            input.original.clone(),
            input.replacement,
            input.evidence,
            now,
        )
        .await;
    if result.is_ok() && recheck(depot).is_err() {
        failure(res, hagency_store::Error::OutcomeUnknown);
        return;
    }
    match result {
        Ok(()) => res.render(Json(serde_json::json!({"ok": true}))),
        Err(hagency_store::Error::State) => {
            // The orphan-state guard or the stop-row refusal (execution.rs:1044-1050).
            refusal(res, StatusCode::CONFLICT, "dispatch_not_recoverable")
        }
        Err(hagency_store::Error::NotFound) => refusal(res, StatusCode::NOT_FOUND, "not_found"),
        Err(error) => failure(res, error),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct PresetApply {
    preset_id: String,
}

/// Preset-apply — a pointer, not a second editor: the named preset must be
/// already-published, and exactly one apply may be pending per session. The
/// preset's own fields stay the configure scope's act; nothing is widened
/// and no row is written by this route.
#[handler]
async fn preset(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    if query(req, &[], 0).is_err() {
        failed(res, Error::Invalid);
        return;
    }
    let engagement = match engagement_id(req) {
        Ok(id) => id,
        Err(error) => {
            failed(res, error);
            return;
        }
    };
    if !check_lifecycle(depot, res) {
        return;
    }
    let raw = match body(req, 256).await {
        Ok(raw) => raw,
        Err(_) => {
            failed(res, Error::Invalid);
            return;
        }
    };
    let input: PresetApply = match serde_json::from_slice(&raw) {
        Ok(input) => input,
        Err(_) => {
            failed(res, Error::Invalid);
            return;
        }
    };
    if identifier(&input.preset_id, 128).is_err() {
        failed(res, Error::Invalid);
        return;
    }
    let Some(store) = domain(depot, res) else {
        return;
    };
    let resource = match store.resource_configuration(input.preset_id.clone()).await {
        Ok(resource) => resource,
        Err(hagency_store::Error::NotFound) => {
            refusal(res, StatusCode::NOT_FOUND, "preset_not_published");
            return;
        }
        Err(error) => {
            failure(res, error);
            return;
        }
    };
    if !resource.published {
        refusal(res, StatusCode::NOT_FOUND, "preset_not_published");
        return;
    }
    // One pending apply per session, tracked in-memory on the grant (the
    // spec licenses no new store column; the apply writes no row).
    let session = match depot.get_typed::<Session>() {
        Ok(session) => session,
        Err(_) => {
            failed(res, Error::Unauthorized);
            return;
        }
    };
    if let Err(error) = console(depot).and_then(|c| {
        c.0.authority
            .begin_lifecycle_apply(session, &input.preset_id)
    }) {
        failed(res, error);
        return;
    }
    let _ = engagement;
    res.render(Json(
        serde_json::json!({"ok": true, "presetId": input.preset_id}),
    ));
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
