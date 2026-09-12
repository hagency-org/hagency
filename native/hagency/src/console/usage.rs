use super::{Error, failed, recheck};
use crate::{refusal, resources::domain};
use hagency_core::project::{CleanupState, EngagementState, identifier};
use salvo::prelude::*;
use serde::Serialize;

pub(super) fn router() -> Router {
    Router::new()
        .push(Router::with_path("engagements").get(engagements))
        .push(Router::with_path("engagements/{id}/usage").get(report))
}
pub(super) fn query(req: &Request, allowed: &[&str], limit: usize) -> Result<(), Error> {
    if req
        .uri()
        .query()
        .is_some_and(|q| q.len() > limit || q.contains('%') || q.contains('+'))
    {
        return Err(Error::Invalid);
    }
    let values = req.queries();
    for (key, _) in values.iter() {
        if !allowed.contains(&key.as_str())
            || values
                .get_vec(key)
                .is_none_or(|v| v.len() != 1 || v[0].is_empty())
        {
            return Err(Error::Invalid);
        }
    }
    Ok(())
}
pub(super) fn selection_query(req: &Request) -> Result<(), Error> {
    query(req, &["engagement_id"], 160)?;
    if let Some(value) = req.query::<String>("engagement_id") {
        identifier(&value, 128).map_err(|_| Error::Invalid)?;
    }
    Ok(())
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Label {
    id: String,
    agent_name: String,
    project_name: Option<String>,
    role: String,
    state: EngagementState,
    cleanup: CleanupState,
}
#[handler]
async fn engagements(req: &mut Request, depot: &mut Depot, res: &mut Response) {
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
    let result = store.engagements(after, limit + 1).await;
    if let Err(error) = recheck(depot) {
        failed(res, error);
        return;
    }
    match result {
        Ok(mut rows) => {
            let next_after = (rows.len() > limit).then(|| rows[limit - 1].id.clone());
            rows.truncate(limit);
            let labels: Vec<_> = rows
                .into_iter()
                .map(|e| Label {
                    id: e.id,
                    agent_name: e.agent_name.as_str().to_owned(),
                    project_name: e.project_name,
                    role: e.role,
                    state: e.state,
                    cleanup: e.cleanup,
                })
                .collect();
            res.render(Json(
                serde_json::json!({"engagements":labels,"next_after":next_after}),
            ));
        }
        Err(error) => store_error(res, error),
    }
}
#[handler]
async fn report(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    if query(req, &["at_ms"], 128).is_err() {
        failed(res, Error::Invalid);
        return;
    }
    let at = match req.query::<String>("at_ms") {
        None => None,
        Some(v) if v.bytes().all(|c| c.is_ascii_digit()) => match v.parse::<u64>() {
            Ok(v) => Some(v),
            Err(_) => {
                failed(res, Error::Invalid);
                return;
            }
        },
        _ => {
            failed(res, Error::Invalid);
            return;
        }
    };
    let Some(id) = req.param::<String>("id") else {
        failed(res, Error::Invalid);
        return;
    };
    let Some(store) = domain(depot, res) else {
        return;
    };
    let result = store.usage_report(id, at).await;
    if let Err(error) = recheck(depot) {
        failed(res, error);
        return;
    }
    match result {
        Ok(value) => res.render(Json(value)),
        Err(error) => store_error(res, error),
    }
}
fn store_error(res: &mut Response, error: hagency_store::Error) {
    let (status, code) = match error {
        hagency_store::Error::Invalid(_) => (StatusCode::BAD_REQUEST, "invalid_usage_query"),
        hagency_store::Error::NotFound => (StatusCode::NOT_FOUND, "not_found"),
        hagency_store::Error::Busy => (StatusCode::SERVICE_UNAVAILABLE, "busy"),
        hagency_store::Error::OutcomeUnknown => (StatusCode::GATEWAY_TIMEOUT, "outcome_unknown"),
        _ => (StatusCode::SERVICE_UNAVAILABLE, "usage_unavailable"),
    };
    refusal(res, status, code);
}
