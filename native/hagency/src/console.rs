//! Read-only browser facade. Native operator/runner authentication is unchanged.
mod assets;
mod authority;
pub mod client;
mod resource_configuration;
mod resources;
mod usage;
use crate::{App, refusal};
use authority::{Authority, COOKIE, Session};
use hyper::Method;
use salvo::prelude::*;
use serde::Deserialize;
use std::{path::Path, sync::Arc, time::Duration};
use tokio::sync::Semaphore;

#[derive(Debug, Clone, Copy, thiserror::Error)]
pub enum Error {
    #[error("native console assets are invalid or exceed their bounds")]
    Assets,
    #[error("native console request is invalid")]
    Invalid,
    #[error("native console access is required or expired")]
    Unauthorized,
    #[error("resource publication management scope is required")]
    Forbidden,
    #[error("resource configuration management scope is required")]
    ConfigurationForbidden,
    #[error("native console capacity is exhausted")]
    Busy,
    #[error("native console is unavailable")]
    Unavailable,
}
struct Inner {
    assets: assets::Assets,
    authority: Authority,
    requests: Arc<Semaphore>,
}
/// Clones retain the same finite authority and original immutable asset proofs.
#[derive(Clone)]
pub struct Console(Arc<Inner>);
impl Console {
    /// Synchronous startup only; no filesystem access occurs in HTTP handlers.
    pub fn load(path: &Path) -> Result<Self, Error> {
        Ok(Self(Arc::new(Inner {
            assets: assets::Assets::load(path)?,
            authority: Authority::new(),
            requests: Arc::new(Semaphore::new(8)),
        })))
    }
    pub fn retire(&self) {
        self.0.authority.retire();
    }
}
pub(crate) fn router() -> Router {
    Router::with_path("console")
        .hoop(browser_boundary)
        .push(Router::with_path("session").post(exchange).delete(logout))
        .push(
            Router::with_path("api")
                .hoop(authenticate)
                .push(usage::router())
                .push(resources::router())
                .push(resource_configuration::router()),
        )
        .push(Router::with_path("{**asset}").get(asset))
}
pub(crate) fn operator_router() -> Router {
    Router::new()
        .push(Router::with_path("console/access").post(issue))
        .push(Router::with_path("console/resource-publication-access").post(issue_publication))
        .push(Router::with_path("console/resource-configuration-access").post(issue_configuration))
}
fn console(depot: &Depot) -> Result<&Console, Error> {
    depot
        .get_typed::<App>()
        .ok()
        .and_then(|app| app.console.as_ref())
        .ok_or(Error::Unavailable)
}
fn failed(res: &mut Response, error: Error) {
    let (status, code) = match error {
        Error::Assets | Error::Unavailable => {
            (StatusCode::SERVICE_UNAVAILABLE, "console_unavailable")
        }
        Error::Invalid => (StatusCode::BAD_REQUEST, "invalid_console_request"),
        Error::Unauthorized => (StatusCode::UNAUTHORIZED, "console_access_required"),
        Error::Forbidden => (StatusCode::FORBIDDEN, "resource_publication_scope_required"),
        Error::ConfigurationForbidden => (
            StatusCode::FORBIDDEN,
            "resource_configuration_scope_required",
        ),
        Error::Busy => (StatusCode::TOO_MANY_REQUESTS, "console_busy"),
    };
    refusal(res, status, code);
}
fn common_authority(req: &Request, depot: &Depot) -> bool {
    let Ok(app) = depot.get_typed::<App>() else {
        return false;
    };
    let headers = req.headers();
    headers.get_all("host").iter().count() == 1
        && headers.get("host").and_then(|v| v.to_str().ok()) == Some(app.authority.as_str())
        && !headers
            .keys()
            .any(|k| k == "forwarded" || k.as_str().starts_with("x-forwarded-"))
        && !headers.contains_key("authorization")
}
fn same_origin(req: &Request, depot: &Depot, mutation: bool) -> bool {
    let Ok(app) = depot.get_typed::<App>() else {
        return false;
    };
    let origin = format!("http://{}", app.authority);
    let h = req.headers();
    h.get_all("sec-fetch-site").iter().count() == 1
        && h.get("sec-fetch-site").and_then(|v| v.to_str().ok()) == Some("same-origin")
        && h.get_all("origin").iter().count() <= 1
        && match h.get("origin") {
            Some(value) => value.to_str().ok() == Some(origin.as_str()),
            None => !mutation,
        }
}
#[handler]
async fn browser_boundary(
    req: &mut Request,
    depot: &mut Depot,
    res: &mut Response,
    ctrl: &mut FlowCtrl,
) {
    for (name, value) in [
        ("cache-control", "no-store"),
        ("referrer-policy", "no-referrer"),
        ("x-content-type-options", "nosniff"),
        ("x-frame-options", "DENY"),
    ] {
        res.headers_mut()
            .insert(name, value.parse().expect("static header"));
    }
    if !common_authority(req, depot) {
        refusal(res, StatusCode::FORBIDDEN, "console_origin_required");
        ctrl.skip_rest();
        return;
    }
    let result = console(depot).and_then(|c| {
        c.0.requests
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Busy)
    });
    match result {
        Ok(permit) => {
            depot.insert("console_permit", permit);
        }
        Err(error) => {
            failed(res, error);
            ctrl.skip_rest();
        }
    }
}
fn cookie(req: &Request) -> Result<&str, Error> {
    let headers = req.headers();
    if headers.get_all("cookie").iter().count() != 1 {
        return Err(Error::Unauthorized);
    }
    let value = headers
        .get("cookie")
        .and_then(|v| v.to_str().ok())
        .filter(|v| v.len() <= 4096)
        .ok_or(Error::Unauthorized)?;
    let mut selected = None;
    let mut count = 0;
    for field in value.split(';') {
        count += 1;
        if count > 16 {
            return Err(Error::Unauthorized);
        }
        let (name, value) = field.trim().split_once('=').ok_or(Error::Unauthorized)?;
        if name == COOKIE && selected.replace(value).is_some() {
            return Err(Error::Unauthorized);
        }
    }
    selected.ok_or(Error::Unauthorized)
}
fn current(req: &Request, depot: &Depot) -> Result<Session, Error> {
    if !same_origin(req, depot, req.method() != Method::GET) {
        return Err(Error::Unauthorized);
    }
    console(depot)?.0.authority.authenticate(cookie(req)?)
}
#[handler]
async fn authenticate(
    req: &mut Request,
    depot: &mut Depot,
    res: &mut Response,
    ctrl: &mut FlowCtrl,
) {
    match current(req, depot) {
        Ok(session) => {
            depot.insert_typed(session);
        }
        Err(error) => {
            failed(res, error);
            ctrl.skip_rest();
        }
    }
}
fn recheck(depot: &Depot) -> Result<(), Error> {
    console(depot)?.0.authority.check(
        depot
            .get_typed::<Session>()
            .map_err(|_| Error::Unauthorized)?,
    )
}
async fn body(req: &mut Request, maximum: usize) -> Result<Vec<u8>, Error> {
    tokio::time::timeout(Duration::from_secs(2), req.payload_with_max_size(maximum))
        .await
        .map_err(|_| Error::Invalid)?
        .map(|v| v.to_vec())
        .map_err(|_| Error::Invalid)
}
#[handler]
async fn issue(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    issue_scope(req, depot, res, false, false).await;
}
#[handler]
async fn issue_publication(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    issue_scope(req, depot, res, true, false).await;
}
#[handler]
async fn issue_configuration(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    issue_scope(req, depot, res, false, true).await;
}
async fn issue_scope(
    req: &mut Request,
    depot: &Depot,
    res: &mut Response,
    publication: bool,
    configuration: bool,
) {
    let result = async {
        let c = console(depot)?;
        let _permit =
            c.0.requests
                .clone()
                .try_acquire_owned()
                .map_err(|_| Error::Busy)?;
        if req.uri().query().is_some() || !body(req, 1).await?.is_empty() {
            return Err(Error::Invalid);
        }
        let value = if configuration {
            c.0.authority.issue_configuration()?
        } else if publication {
            c.0.authority.issue_publication()?
        } else {
            c.0.authority.issue()?
        };
        Ok(serde_json::json!({"ticket":value,"expires_in":120}))
    }
    .await;
    match result {
        Ok(value) => res.render(Json(value)),
        Err(error) => failed(res, error),
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Exchange {
    ticket: String,
}
#[handler]
async fn exchange(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    let result = async {
        if req.uri().query().is_some()
            || !same_origin(req, depot, true)
            || req.headers().get_all("content-type").iter().count() != 1
            || req
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.split(';').next())
                .map(str::trim)
                != Some("application/json")
        {
            return Err(Error::Invalid);
        }
        let input: Exchange =
            serde_json::from_slice(&body(req, 256).await?).map_err(|_| Error::Invalid)?;
        console(depot)?.0.authority.exchange(&input.ticket)
    }
    .await;
    match result {
        Ok(value) => {
            res.headers_mut().insert(
                "set-cookie",
                format!("{COOKIE}={value}; HttpOnly; SameSite=Strict; Path=/console; Max-Age=900")
                    .parse()
                    .expect("generated cookie"),
            );
            res.render(Json(serde_json::json!({"ok":true,"expires_in":900})));
        }
        Err(error) => failed(res, error),
    }
}
#[handler]
async fn logout(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    let result = async {
        if req.uri().query().is_some() || !body(req, 1).await?.is_empty() {
            return Err(Error::Invalid);
        }
        let session = current(req, depot)?;
        console(depot)?.0.authority.revoke(&session)
    }
    .await;
    match result {
        Ok(()) => {
            res.headers_mut().insert(
                "set-cookie",
                format!("{COOKIE}=; HttpOnly; SameSite=Strict; Path=/console; Max-Age=0")
                    .parse()
                    .expect("static cookie"),
            );
            res.render(Json(serde_json::json!({"ok":true})));
        }
        Err(error) => failed(res, error),
    }
}
#[handler]
async fn asset(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    let Ok(c) = console(depot) else {
        failed(res, Error::Unavailable);
        return;
    };
    let path = req.uri().path();
    // Only document navigation may start outside this origin. Assets never grant data authority.
    let resource_document = matches!(path, "/console/resources" | "/console/resources/");
    let editor_document = matches!(path, "/console/resources/new" | "/console/resources/new/");
    let document = editor_document
        || resource_document
        || matches!(path, "/console/usage" | "/console/usage/");
    if !document
        && req
            .headers()
            .get("sec-fetch-site")
            .is_some_and(|v| v != "same-origin" && v != "none")
    {
        refusal(res, StatusCode::FORBIDDEN, "console_origin_required");
        return;
    }
    if (!document && req.uri().query().is_some())
        || (document
            && if editor_document {
                resource_configuration::selection_query(req).is_err()
            } else if resource_document {
                resources::selection_query(req).is_err()
            } else {
                usage::selection_query(req).is_err()
            })
    {
        failed(res, Error::Invalid);
        return;
    }
    match c.0.assets.get(path) {
        Some(value) => {
            res.headers_mut()
                .insert("content-type", value.mime.parse().expect("validated MIME"));
            if res.write_body(value.bytes.clone()).is_err() {
                res.status_code(StatusCode::INTERNAL_SERVER_ERROR);
            }
        }
        None => {
            res.status_code(StatusCode::NOT_FOUND);
        }
    }
}
