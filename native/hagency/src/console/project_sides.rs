//! The read-only project-side observation (ADR-132): a bounded projection
//! over the fleet registrations and their projects, mounted under the API
//! sub-router's `authenticate` hoop with NO scope — the same read class as
//! `engagements` and the roster (ADR-126): scope facts are a payload on
//! reads, never a gate. The wire item carries EXACTLY six keys — `id`,
//! `representative`, `generation`, `reception_room_id`, `registered`,
//! `projects` (each exactly `{id, room_id}`) — serialized straight from the
//! store's `ProjectSide`, which extracts only the named config paths with
//! `json_extract`, never a parse-and-strip of the whole config: no
//! credential key exists, and a credential-shaped value seeded into a
//! registration config has no path into any byte of this response.
//! `owner_mxid` and the owner's DM room are withheld (ADR-112); the
//! server-owned `unavailable` list names every retained column native has
//! no source for, rendered as unknown by the page — never zero, never
//! invented.
use super::engagements::check_lifecycle;
use super::{Error, body, failed, recheck, usage::query};
use crate::{refusal, resources::domain};
use hagency_core::authority::Registration;
use salvo::prelude::*;

pub(super) fn router() -> Router {
    Router::with_path("project-sides").get(list).post(save)
}

/// Every retained `publicSide` column native has no source for in this
/// slice — the credential family (kind, presence, install state, the
/// appservice registration fields), the access verdicts, the per-side
/// allocation, the display label, the API base URL, the per-project name
/// and the owner. `registered` is served INSTEAD of `active` and is not an
/// access verdict; the ADR says so.
const UNAVAILABLE: [&str; 13] = [
    "label",
    "api_base_url",
    "credential_kind",
    "has_credential",
    "awaiting_install",
    "sender_localpart",
    "appservice_url",
    "namespace",
    "access_state",
    "access_detail",
    "allocated_tokens",
    "project_name",
    "owner",
];

#[handler]
async fn list(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    // A list observation takes no selection: any query parameter is
    // refused, the same hygiene the roster applies.
    if query(req, &[], 0).is_err() {
        failed(res, Error::Invalid);
        return;
    }
    let Some(store) = domain(depot, res) else {
        return;
    };
    let result = store.project_sides().await;
    if let Err(error) = recheck(depot) {
        failed(res, error);
        return;
    }
    match result {
        Ok(sides) => {
            // Statement time, as the roster and alerts reads do.
            let at_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .ok()
                .and_then(|d| u64::try_from(d.as_millis()).ok())
                .unwrap_or_default();
            res.render(Json(serde_json::json!({
                "at_ms": at_ms,
                "unavailable": UNAVAILABLE,
                "sides": sides,
            })));
        }
        Err(error) => store_error(res, error),
    }
}

fn store_error(res: &mut Response, error: hagency_store::Error) {
    let (status, code) = match error {
        hagency_store::Error::Invalid(_) => (StatusCode::BAD_REQUEST, "invalid_side_query"),
        hagency_store::Error::Generation => (StatusCode::CONFLICT, "stale_generation"),
        hagency_store::Error::Busy => (StatusCode::SERVICE_UNAVAILABLE, "busy"),
        hagency_store::Error::OutcomeUnknown => (StatusCode::GATEWAY_TIMEOUT, "outcome_unknown"),
        _ => (StatusCode::SERVICE_UNAVAILABLE, "sides_unavailable"),
    };
    refusal(res, status, code);
}

/// The operator's fleet registration (G11, spec
/// `task-rust-project-side-registration`): the retained `POST /api/project-sides`
/// shape on the console API, gated by the same `Scope::AgentLifecycle` the
/// lifecycle mutations use. The store owns every guarantee — shape validation,
/// the identical-content no-op, the stale-generation refusal, the rotate-and-
/// reconcile on advance; the route adds no guard of its own and renders the
/// saved record's own fields, never the operator token.
#[handler]
async fn save(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    if !check_lifecycle(depot, res) {
        return;
    }
    let raw = match body(req, 4 * 1024).await {
        Ok(raw) => raw,
        Err(_) => {
            failed(res, Error::Invalid);
            return;
        }
    };
    let registration: Registration = match serde_json::from_slice(&raw) {
        Ok(registration) => registration,
        Err(_) => {
            failed(res, Error::Invalid);
            return;
        }
    };
    let Some(store) = domain(depot, res) else {
        return;
    };
    let result = store.register(registration.clone()).await;
    if let Err(error) = recheck(depot) {
        failed(res, error);
        return;
    }
    match result {
        Ok(()) => res.render(Json(serde_json::json!({
            "ok": true,
            "side": {
                "id": registration.fleet_id,
                "generation": registration.generation,
                "server_name": registration.server_name,
                "reception_room_id": registration.reception_room_id,
                "representative": registration.representative_mxid,
            },
        }))),
        Err(error) => store_error(res, error),
    }
}
