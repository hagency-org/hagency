//! New sends use current authentication; historical reads have only exact-row
//! authority. Neither route performs source IO or owns a cancellable pipeline.
use crate::{
    App,
    file_service::{FileError, FileHandle, FileView, SendFile},
    local_authority, refusal,
};
use hagency_core::project::identifier;
use salvo::prelude::*;
use std::time::Duration;

pub(super) fn current() -> Router {
    Router::with_path("file-deliveries").post(submit)
}
pub(super) fn historical() -> Router {
    Router::with_path("file-deliveries/{id}").get(inspect)
}
fn service(depot: &Depot, res: &mut Response) -> Option<FileHandle> {
    let files = depot.get_typed::<App>().ok().and_then(|a| a.files.clone());
    if files.is_none() {
        failure(res, FileError::Unavailable);
    }
    files
}
fn failure(res: &mut Response, error: FileError) {
    let status = match error {
        FileError::Invalid => StatusCode::BAD_REQUEST,
        FileError::Unauthorized => StatusCode::FORBIDDEN,
        FileError::Conflict => StatusCode::CONFLICT,
        FileError::Busy | FileError::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        FileError::Unknown => StatusCode::GATEWAY_TIMEOUT,
    };
    refusal(res, status, error.code());
}
fn render(res: &mut Response, result: Result<FileView, FileError>) {
    match result {
        Ok(value) if value.validate().is_ok() => res.render(Json(value)),
        // A malformed receipt cannot certify admission or delivery.
        Ok(_) => failure(res, FileError::Unknown),
        Err(error) => failure(res, error),
    }
}
async fn body(req: &mut Request, depot: &Depot, res: &mut Response) -> Option<SendFile> {
    let app = depot.get_typed::<App>().ok()?;
    let Ok(_permit) = app.requests.clone().try_acquire_owned() else {
        failure(res, FileError::Busy);
        return None;
    };
    if req.uri().query().is_some() || req.headers().contains_key("content-encoding") {
        failure(res, FileError::Invalid);
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
        .and_then(|value| serde_json::from_value::<SendFile>(value).ok())
        .filter(|input| input.validate().is_ok());
    if input.is_none() {
        failure(res, FileError::Invalid);
    }
    input
}
#[handler]
async fn submit(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    let Some(context) = super::context(depot, res) else {
        return;
    };
    let Some(files) = service(depot, res) else {
        return;
    };
    let Some(input) = body(req, depot, res).await else {
        return;
    };
    // The synchronous handoff retains the original bounded job before the
    // caller can disappear at an await. Dropping the waiter cannot drop the job.
    let result = match files.submit(context.cap, input) {
        Ok(wait) => tokio::time::timeout(crate::task_client::DEFAULT_DEADLINE, wait.wait())
            .await
            .unwrap_or(Err(FileError::Unknown)),
        Err(error) => Err(error),
    };
    render(res, result);
}
#[handler]
async fn inspect(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    if !local_authority(req, depot, res) {
        return;
    }
    let Some(capability) = super::credential(req) else {
        refusal(res, StatusCode::UNAUTHORIZED, "runner_auth_required");
        return;
    };
    let Some(files) = service(depot, res) else {
        return;
    };
    let Some(id) = req
        .param::<String>("id")
        .filter(|id| identifier(id, 128).is_ok())
    else {
        failure(res, FileError::Invalid);
        return;
    };
    if req.uri().query().is_some() {
        failure(res, FileError::Invalid);
        return;
    }
    // No current Check, capture, reconciliation or replacement execution.
    // The original domain row must independently match this exact credential.
    render(res, files.inspect(capability, id).await);
}
