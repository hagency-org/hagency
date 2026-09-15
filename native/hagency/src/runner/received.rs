//! Both received-file routes require current task authority, including cache reads.
use crate::{
    App,
    receive_service::{ReceiveError, ReceiveFile, ReceiveHandle},
    refusal,
    task_client::received::Page,
};
use salvo::prelude::*;
use std::time::Duration;

pub(super) fn router() -> Router {
    Router::with_path("received-files").get(list).post(receive)
}
fn failure(res: &mut Response, error: ReceiveError) {
    let status = match error {
        ReceiveError::Invalid => StatusCode::BAD_REQUEST,
        ReceiveError::Unauthorized => StatusCode::FORBIDDEN,
        ReceiveError::Conflict => StatusCode::CONFLICT,
        ReceiveError::Busy | ReceiveError::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        ReceiveError::Unknown => StatusCode::GATEWAY_TIMEOUT,
    };
    refusal(res, status, error.code());
}
fn service(depot: &Depot, res: &mut Response) -> Option<ReceiveHandle> {
    let handle = depot
        .get_typed::<App>()
        .ok()
        .and_then(|app| app.receives.clone());
    if handle.is_none() {
        failure(res, ReceiveError::Unavailable);
    }
    handle
}
fn page(req: &Request) -> Result<Page, ReceiveError> {
    let mut after = None;
    let mut limit = None;
    if let Some(query) = req.uri().query() {
        if query.len() > 96 {
            return Err(ReceiveError::Invalid);
        }
        for part in query.split('&') {
            let (name, value) = part.split_once('=').ok_or(ReceiveError::Invalid)?;
            if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
                return Err(ReceiveError::Invalid);
            }
            match name {
                "after" if after.is_none() => {
                    after = Some(value.parse().map_err(|_| ReceiveError::Invalid)?)
                }
                "limit" if limit.is_none() => {
                    limit = Some(value.parse().map_err(|_| ReceiveError::Invalid)?)
                }
                _ => return Err(ReceiveError::Invalid),
            }
        }
    }
    let page = Page {
        after: after.unwrap_or(0),
        limit: limit.unwrap_or(16),
    };
    page.validate().map_err(|_| ReceiveError::Invalid)?;
    Ok(page)
}
async fn body(req: &mut Request, depot: &Depot, res: &mut Response) -> Option<ReceiveFile> {
    let app = depot.get_typed::<App>().ok()?;
    let Ok(_permit) = app.requests.clone().try_acquire_owned() else {
        failure(res, ReceiveError::Busy);
        return None;
    };
    if req.uri().query().is_some() || req.headers().contains_key("content-encoding") {
        failure(res, ReceiveError::Invalid);
        return None;
    }
    if super::single_header(req, "content-type")
        .and_then(|v| v.split(';').next())
        .map(str::trim)
        != Some("application/json")
    {
        refusal(res, StatusCode::UNSUPPORTED_MEDIA_TYPE, "json_required");
        return None;
    }
    let bytes =
        match tokio::time::timeout(Duration::from_secs(2), req.payload_with_max_size(16 * 1024))
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
    let input = crate::mcp::json::json(bytes)
        .ok()
        .and_then(|v| serde_json::from_value::<ReceiveFile>(v).ok())
        .filter(|v| v.validate().is_ok());
    if input.is_none() {
        failure(res, ReceiveError::Invalid);
    }
    input
}
#[handler]
async fn list(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    let Some(context) = super::context(depot, res) else {
        return;
    };
    let page = match page(req) {
        Ok(page) => page,
        Err(error) => {
            failure(res, error);
            return;
        }
    };
    let Some(files) = service(depot, res) else {
        return;
    };
    let result = tokio::time::timeout(
        crate::task_client::DEFAULT_DEADLINE,
        files.list(context.cap, page.after, page.limit),
    )
    .await
    .unwrap_or(Err(ReceiveError::Unknown));
    match result {
        Ok(value) if page.validate_response(&value).is_ok() => res.render(Json(value)),
        Ok(_) => failure(res, ReceiveError::Unknown),
        Err(error) => failure(res, error),
    }
}
#[handler]
async fn receive(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    let Some(context) = super::context(depot, res) else {
        return;
    };
    let Some(input) = body(req, depot, res).await else {
        return;
    };
    let Some(files) = service(depot, res) else {
        return;
    };
    let event = input.event_id.clone();
    // Submit retains the original bounded job synchronously. Caller timeout or
    // disappearance drops only this waiter, never the admitted operation owner.
    let result = match files.submit(context.cap, input) {
        Ok(wait) => tokio::time::timeout(crate::task_client::DEFAULT_DEADLINE, wait.wait())
            .await
            .unwrap_or(Err(ReceiveError::Unknown)),
        Err(error) => Err(error),
    };
    match result {
        Ok(value) if value.validate().is_ok() && value.event_id == event => res.render(Json(value)),
        Ok(_) => failure(res, ReceiveError::Unknown),
        Err(error) => failure(res, error),
    }
}
