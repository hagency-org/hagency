//! Operator-only management. Authenticated Matrix admission will have its own
//! adapter; this API does not accept room observation or approval booleans.
use crate::{App, refusal};
use hagency_core::project::{Resource, Seat};
use hagency_store::{DomainStore, Error};
use salvo::prelude::*;
use serde::de::DeserializeOwned;
use std::time::Duration;

pub(crate) fn router() -> Router {
    Router::new()
        .push(
            Router::with_path("resources")
                .get(catalog)
                .post(put_resource),
        )
        .push(Router::with_path("resources/{id}/budget").get(budget))
        .push(Router::with_path("resource-configurations").get(configurations))
        .push(Router::with_path("seats").get(seats).post(put_seat))
        .push(Router::with_path("engagements").get(engagements))
        .push(Router::with_path("roles").get(role_publications))
        .push(Router::with_path("roles/{role}/publication").post(publish_role))
}
pub(super) fn domain(depot: &Depot, res: &mut Response) -> Option<DomainStore> {
    let store = depot.get_typed::<App>().ok().and_then(|a| a.domain.clone());
    if store.is_none() {
        refusal(res, StatusCode::SERVICE_UNAVAILABLE, "domain_unavailable");
    }
    store
}
fn failure(res: &mut Response, error: Error) {
    let (status, code) = match error {
        Error::Invalid(_) => (StatusCode::BAD_REQUEST, "invalid_domain_command"),
        Error::NotFound => (StatusCode::NOT_FOUND, "not_found"),
        Error::Conflict => (StatusCode::CONFLICT, "idempotency_conflict"),
        Error::State => (StatusCode::CONFLICT, "state_conflict"),
        Error::Busy => (StatusCode::SERVICE_UNAVAILABLE, "busy"),
        Error::OutcomeUnknown => (StatusCode::GATEWAY_TIMEOUT, "outcome_unknown"),
        _ => (StatusCode::SERVICE_UNAVAILABLE, "domain_unavailable"),
    };
    refusal(res, status, code);
}
pub(super) async fn body<T: DeserializeOwned>(
    req: &mut Request,
    depot: &Depot,
    res: &mut Response,
) -> Option<T> {
    let app = depot.get_typed::<App>().ok()?;
    let Ok(_permit) = app.requests.clone().try_acquire_owned() else {
        refusal(res, StatusCode::SERVICE_UNAVAILABLE, "busy");
        return None;
    };
    if req
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(';').next())
        .map(str::trim)
        != Some("application/json")
    {
        refusal(res, StatusCode::UNSUPPORTED_MEDIA_TYPE, "json_required");
        return None;
    }
    let bytes =
        match tokio::time::timeout(Duration::from_secs(2), req.payload_with_max_size(64 * 1024))
            .await
        {
            Ok(Ok(bytes)) => bytes,
            Ok(Err(_)) => {
                refusal(res, StatusCode::PAYLOAD_TOO_LARGE, "body_rejected");
                return None;
            }
            Err(_) => {
                refusal(res, StatusCode::REQUEST_TIMEOUT, "body_timeout");
                return None;
            }
        };
    match serde_json::from_slice(bytes) {
        Ok(value) => Some(value),
        Err(_) => {
            refusal(res, StatusCode::BAD_REQUEST, "invalid_domain_command");
            None
        }
    }
}
#[handler]
async fn put_resource(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    let Some(store) = domain(depot, res) else {
        return;
    };
    let Some(input) = body::<serde_json::Value>(req, depot, res).await else {
        return;
    };
    if input.get("roles").is_some() {
        refusal(res, StatusCode::BAD_REQUEST, "roles_are_model_derived");
        return;
    }
    let publication = input.get("published").and_then(|v| v.as_bool());
    let value = match serde_json::from_value::<Resource>(input) {
        Ok(value) => value,
        Err(_) => {
            refusal(res, StatusCode::BAD_REQUEST, "invalid_domain_command");
            return;
        }
    };
    match store.edit_resource(value, publication).await {
        Ok(value) => res.render(Json(value)),
        Err(error) => failure(res, error),
    }
}
#[handler]
async fn put_seat(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    let Some(store) = domain(depot, res) else {
        return;
    };
    let Some(value) = body::<Seat>(req, depot, res).await else {
        return;
    };
    match store.put_seat(value).await {
        Ok(()) => res.render(Json(serde_json::json!({"saved":true}))),
        Err(error) => failure(res, error),
    }
}
fn page(req: &Request) -> Result<(String, usize), Error> {
    let after = req.query::<String>("after").unwrap_or_default();
    if after.len() > 256 {
        return Err(hagency_core::InvalidInput("invalid page cursor").into());
    }
    let limit = match req.query::<String>("limit") {
        Some(n) => n
            .parse()
            .map_err(|_| hagency_core::InvalidInput("invalid page limit"))?,
        None => 100,
    };
    Ok((after, limit))
}
#[handler]
async fn catalog(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    let Some(store) = domain(depot, res) else {
        return;
    };
    let (after, limit) = match page(req) {
        Ok(page) => page,
        Err(error) => {
            failure(res, error);
            return;
        }
    };
    match store.catalog(after, limit).await {
        Ok(value) => res.render(Json(value)),
        Err(error) => failure(res, error),
    }
}
#[handler]
async fn configurations(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    let Some(store) = domain(depot, res) else {
        return;
    };
    let (after, limit) = match page(req) {
        Ok(page) => page,
        Err(error) => {
            failure(res, error);
            return;
        }
    };
    match store.resource_configurations(after, limit).await {
        Ok(value) => res.render(Json(value)),
        Err(error) => failure(res, error),
    }
}
#[handler]
async fn role_publications(depot: &mut Depot, res: &mut Response) {
    let Some(store) = domain(depot, res) else {
        return;
    };
    match store.role_publications().await {
        Ok(value) => res.render(Json(value)),
        Err(error) => failure(res, error),
    }
}
#[handler]
async fn publish_role(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Publication {
        published: bool,
    }
    let Some(store) = domain(depot, res) else {
        return;
    };
    let Some(role) = req.param::<String>("role") else {
        refusal(res, StatusCode::BAD_REQUEST, "role_required");
        return;
    };
    let Some(input) = body::<Publication>(req, depot, res).await else {
        return;
    };
    match store.set_role_publication(role, input.published).await {
        Ok(()) => res.render(Json(serde_json::json!({"saved":true}))),
        Err(error) => failure(res, error),
    }
}
#[handler]
async fn seats(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    let Some(store) = domain(depot, res) else {
        return;
    };
    let (after, limit) = match page(req) {
        Ok(page) => page,
        Err(error) => {
            failure(res, error);
            return;
        }
    };
    match store.seats(after, limit).await {
        Ok(value) => res.render(Json(value)),
        Err(error) => failure(res, error),
    }
}
#[handler]
async fn engagements(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    let Some(store) = domain(depot, res) else {
        return;
    };
    let (after, limit) = match page(req) {
        Ok(page) => page,
        Err(error) => {
            failure(res, error);
            return;
        }
    };
    match store.engagements(after, limit).await {
        Ok(value) => res.render(Json(value)),
        Err(error) => failure(res, error),
    }
}
#[handler]
async fn budget(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    let Some(store) = domain(depot, res) else {
        return;
    };
    let Some(id) = req.param::<String>("id") else {
        refusal(res, StatusCode::BAD_REQUEST, "resource_required");
        return;
    };
    match store.resource_budget(id).await {
        Ok(value) => res.render(Json(value)),
        Err(error) => failure(res, error),
    }
}
