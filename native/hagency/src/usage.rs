//! Aggregate-only reads behind the existing operator authentication boundary.
use crate::{refusal, resources::domain};
use hagency_store::Error;
use salvo::prelude::*;

pub(crate) fn router() -> Router {
    Router::with_path("engagements/{id}/usage").get(report)
}

fn time_query(req: &Request) -> Result<Option<u64>, ()> {
    if req.uri().query().is_some_and(|q| q.len() > 128) {
        return Err(());
    }
    let fields = req.queries();
    if fields.is_empty() {
        return Ok(None);
    }
    if fields.len() != 1 {
        return Err(());
    }
    let values = fields.get_vec("at_ms").ok_or(())?;
    if values.len() != 1 || values[0].is_empty() || !values[0].bytes().all(|b| b.is_ascii_digit()) {
        return Err(());
    }
    Ok(Some(values[0].parse().map_err(|_| ())?))
}

#[handler]
async fn report(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    let Some(store) = domain(depot, res) else {
        return;
    };
    let (Some(id), Ok(at)) = (req.param::<String>("id"), time_query(req)) else {
        refusal(res, StatusCode::BAD_REQUEST, "invalid_usage_query");
        return;
    };
    match store.usage_report(id, at).await {
        Ok(value) => res.render(Json(value)),
        Err(error) => {
            let (status, code) = match error {
                Error::Invalid(_) => (StatusCode::BAD_REQUEST, "invalid_usage_query"),
                Error::NotFound => (StatusCode::NOT_FOUND, "not_found"),
                Error::Busy => (StatusCode::SERVICE_UNAVAILABLE, "busy"),
                Error::OutcomeUnknown => (StatusCode::GATEWAY_TIMEOUT, "outcome_unknown"),
                _ => (StatusCode::SERVICE_UNAVAILABLE, "usage_unavailable"),
            };
            refusal(res, status, code);
        }
    }
}
