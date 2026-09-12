use hagency_core::custody::{Delivery, MAX_DELIVERY_BYTES};
use hagency_store::{DomainStore, Error, Store};
mod alerts;
pub mod bootstrap;
pub mod console;
pub(crate) mod file_service;
pub mod mcp;
pub(crate) mod receive_service;
mod resources;
mod runner;
pub mod task_client;
mod usage;
use salvo::prelude::*;
use sha2::{Digest, Sha256};
use std::{
    net::SocketAddr,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use subtle::ConstantTimeEq;
use tokio::sync::Semaphore;

#[derive(Clone)]
pub struct App {
    store: Store,
    domain: Option<DomainStore>,
    token_hash: [u8; 32],
    authority: String,
    requests: Arc<Semaphore>,
    development: Option<bootstrap::StatusHandle>,
    palpo: Option<bootstrap::palpo::StatusHandle>,
    files: Option<file_service::FileHandle>,
    receives: Option<receive_service::ReceiveHandle>,
    console: Option<console::Console>,
    /// The ceiling-sweep loop's task handle (shared so readiness can observe
    /// liveness without owning the loop) and its tick channel. Both None
    /// when the loop was never started (unit wiring without `Bootstrap`).
    ceiling_sweep: Option<Arc<tokio::task::JoinHandle<()>>>,
    sweep_tick: Option<tokio::sync::watch::Receiver<bootstrap::CeilingSweepTick>>,
}

impl App {
    pub fn new(store: Store, token: &[u8], address: SocketAddr) -> Result<Self, Error> {
        if (!address.ip().is_loopback() || address.port() == 0)
            || token.len() < 32
            || token.len() > 256
            || !token.iter().all(u8::is_ascii_graphic)
        {
            return Err(hagency_core::InvalidInput(
                "use a loopback address and a 32..256 byte ASCII token",
            )
            .into());
        }
        Ok(Self {
            store,
            domain: None,
            token_hash: Sha256::digest(token).into(),
            authority: address.to_string(),
            requests: Arc::new(Semaphore::new(8)),
            development: None,
            palpo: None,
            files: None,
            receives: None,
            console: None,
            ceiling_sweep: None,
            sweep_tick: None,
        })
    }

    pub fn with_domain(mut self, domain: DomainStore) -> Self {
        self.domain = Some(domain);
        self
    }

    /// Attach the ceiling-sweep loop for readiness observation (brief 19).
    /// Called by `Bootstrap::serve` after `start_ceiling_sweep`; the handle
    /// is SHARED (bootstrap keeps it to abort at shutdown, `/health` reads
    /// liveness) so neither owns the loop. Public because integration tests
    /// wire the loop the same way bootstrap does. Readiness is diagnostic
    /// only: nothing may read it to retry, release or complete anything.
    pub fn with_ceiling_sweep(
        mut self,
        sweep: std::sync::Arc<tokio::task::JoinHandle<()>>,
        tick: tokio::sync::watch::Receiver<bootstrap::CeilingSweepTick>,
    ) -> Self {
        self.ceiling_sweep = Some(sweep);
        self.sweep_tick = Some(tick);
        self
    }

    pub fn with_console(mut self, console: console::Console) -> Self {
        self.console = Some(console);
        self
    }

    pub(crate) fn with_development(mut self, status: bootstrap::StatusHandle) -> Self {
        self.development = Some(status);
        self
    }

    pub(crate) fn with_palpo(mut self, status: bootstrap::palpo::StatusHandle) -> Self {
        self.palpo = Some(status);
        self
    }

    pub(crate) fn with_files(mut self, files: file_service::FileHandle) -> Self {
        self.files = Some(files);
        self
    }

    pub(crate) fn with_receive_service(mut self, receives: receive_service::ReceiveHandle) -> Self {
        self.receives = Some(receives);
        self
    }

    pub fn router(self) -> Router {
        Router::new()
            .hoop(self)
            .push(Router::with_path("health").get(health))
            .push(runner::router())
            .push(console::router())
            .push(
                Router::with_path("api/native/v1")
                    .hoop(authorize)
                    .push(Router::with_path("capabilities").get(capabilities))
                    .push(resources::router())
                    .push(usage::router())
                    .push(alerts::router())
                    .push(console::operator_router())
                    .push(Router::with_path("custody").post(receive)),
            )
    }
}

#[handler]
async fn health(depot: &mut Depot, res: &mut Response) {
    // Readiness rollup (brief 19, ADR-096 amendment). Every probe is
    // SYNCHRONOUS: no writer job, no lock, no allocation on the request path
    // beyond the reply — the `bounded_work_keeps_health_responsive`
    // invariant. States are words, never private counts, mirroring the
    // retained rule that `/health` is unauthenticated and must not leak
    // fleet detail (`backend-v2.js:7585` publishes counts to a local
    // operator; the native boundary is stricter: names and states only).
    // Readiness is DIAGNOSTIC, never authority: nothing may read it to
    // retry, release or complete anything.
    let app = depot.get_typed::<App>().ok();
    let mut components: Vec<(&str, &str)> = Vec::new();
    let domain = app.as_ref().and_then(|a| a.domain.as_ref());
    components.push((
        "domain_writer",
        match domain {
            None => "disabled",
            Some(store) if store.writer_open() => "open",
            Some(_) => "closed",
        },
    ));
    let custody = app.as_ref().map(|a| &a.store);
    components.push((
        "custody_store",
        if custody.is_some_and(|s| s.writer_open()) {
            "open"
        } else {
            "closed"
        },
    ));
    // The sweep: liveness is the task; the last tick's outcome is a separate
    // named component so a dead loop can never hide behind a good outcome —
    // and an outcome (swept, or a back-pressure refusal waiting for the next
    // tick) can never fail readiness on its own.
    let sweep = app.as_ref().and_then(|a| a.ceiling_sweep.as_ref());
    let tick = app.as_ref().and_then(|a| a.sweep_tick.as_ref());
    let (sweep_state, last_tick) = match (sweep, tick) {
        (None, _) => ("disabled", None),
        (Some(handle), Some(receiver)) => {
            let last = match &*receiver.borrow() {
                bootstrap::CeilingSweepTick::Swept(_) => "swept",
                bootstrap::CeilingSweepTick::Refused("unstarted") => "unstarted",
                bootstrap::CeilingSweepTick::Refused("busy") => "refused_busy",
                bootstrap::CeilingSweepTick::Refused("outcome_unknown") => {
                    "refused_outcome_unknown"
                }
                bootstrap::CeilingSweepTick::Refused(_) => "refused",
            };
            (
                if handle.is_finished() {
                    "stopped"
                } else if last == "unstarted" {
                    "unstarted"
                } else {
                    "alive"
                },
                Some(last),
            )
        }
        (Some(_), None) => ("unknown", None),
    };
    components.push(("ceiling_sweep", sweep_state));
    components.push(("ceiling_sweep_last_tick", last_tick.unwrap_or("disabled")));
    // Optional owners report their own state words; only the settled
    // failure words fail readiness. `stopped` fails too: a finished loop or
    // owner is not serving, and 503 during shutdown is the honest answer.
    let owner_state = |configured: bool, state: &'static str| -> &'static str {
        if !configured {
            "disabled"
        } else if matches!(state, "unavailable" | "outcome_unknown" | "stopped") {
            state
        } else {
            "ready"
        }
    };
    if let Some(status) = app.as_ref().and_then(|a| a.development.as_ref()) {
        components.push(("development_driver", owner_state(true, status.state())));
    }
    if let Some(status) = app.as_ref().and_then(|a| a.palpo.as_ref()) {
        components.push(("palpo_transport", owner_state(true, status.state())));
    }
    let ready = components.iter().all(|(_, state)| {
        matches!(
            *state,
            "open" | "alive" | "disabled" | "ready" | "unstarted" | "swept" | "running"
        )
    });
    let value = serde_json::json!({
        "status": if ready { "ok" } else { "unavailable" },
        "implementation": "rust",
        "components": components
            .into_iter()
            .map(|(name, state)| serde_json::json!({"name": name, "state": state}))
            .collect::<Vec<_>>(),
    });
    if !ready {
        res.status_code(StatusCode::SERVICE_UNAVAILABLE);
    }
    res.render(Json(value));
}

#[handler]
async fn capabilities(depot: &mut Depot, res: &mut Response) {
    let management = depot
        .get_typed::<App>()
        .is_ok_and(|app| app.domain.is_some());
    let mut value = serde_json::json!({"custody":true, "agent_execution":false, "palpo_transport":false,
        "matrix_crypto":false, "resource_management":management, "runner_task_api":management, "usage_observations_read":management, "project_request_transport":false, "production_api_parity":false});
    if let Ok(app) = depot.get_typed::<App>()
        && let Some(status) = &app.development
    {
        value["development_execution"] =
            serde_json::to_value(status.get()).expect("fixed status serializes");
    }
    if let Ok(app) = depot.get_typed::<App>()
        && let Some(status) = &app.palpo
    {
        value["palpo_publication"] =
            serde_json::to_value(status.get()).expect("fixed status serializes");
    }
    res.render(Json(value));
}

fn refusal(res: &mut Response, status: StatusCode, code: &str) {
    res.status_code(status);
    res.render(Json(serde_json::json!({"ok":false,"code":code})));
}

fn local_authority(req: &Request, depot: &Depot, res: &mut Response) -> bool {
    res.headers_mut()
        .insert("cache-control", "no-store".parse().expect("static header"));
    let Ok(app) = depot.get_typed::<App>() else {
        refusal(res, StatusCode::SERVICE_UNAVAILABLE, "unavailable");
        return false;
    };
    let headers = req.headers();
    let forbidden = headers.get_all("host").iter().count() != 1
        || headers.contains_key("origin")
        || headers.contains_key("sec-fetch-site")
        || headers.contains_key("forwarded")
        || headers.contains_key("x-forwarded-for")
        || headers.get("host").and_then(|v| v.to_str().ok()) != Some(app.authority.as_str());
    if forbidden {
        refusal(res, StatusCode::FORBIDDEN, "local_authority_required");
        return false;
    }
    true
}

#[handler]
async fn authorize(req: &mut Request, depot: &mut Depot, res: &mut Response, ctrl: &mut FlowCtrl) {
    if !local_authority(req, depot, res) {
        ctrl.skip_rest();
        return;
    }
    let Ok(app) = depot.get_typed::<App>() else {
        ctrl.skip_rest();
        return;
    };
    let headers = req.headers();
    let bearer = if headers.get_all("authorization").iter().count() == 1 {
        headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.strip_prefix("Bearer "))
    } else {
        None
    };
    let valid = bearer.filter(|s| s.len() <= 256).is_some_and(|s| {
        let hash: [u8; 32] = Sha256::digest(s.as_bytes()).into();
        bool::from(hash.ct_eq(&app.token_hash))
    });
    if !valid {
        refusal(res, StatusCode::UNAUTHORIZED, "operator_auth_required");
        ctrl.skip_rest();
    }
}

#[handler]
async fn receive(req: &mut Request, depot: &mut Depot, res: &mut Response) {
    let Ok(app) = depot.get_typed::<App>() else {
        refusal(res, StatusCode::SERVICE_UNAVAILABLE, "unavailable");
        return;
    };
    let Ok(_permit) = app.requests.clone().try_acquire_owned() else {
        refusal(res, StatusCode::SERVICE_UNAVAILABLE, "busy");
        return;
    };
    if req
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(';').next().unwrap_or_default().trim())
        != Some("application/json")
    {
        refusal(res, StatusCode::UNSUPPORTED_MEDIA_TYPE, "json_required");
        return;
    }
    if req
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<usize>().ok())
        .is_some_and(|n| n > MAX_DELIVERY_BYTES)
    {
        refusal(res, StatusCode::PAYLOAD_TOO_LARGE, "body_too_large");
        return;
    }
    let bytes = match tokio::time::timeout(
        Duration::from_secs(2),
        req.payload_with_max_size(MAX_DELIVERY_BYTES),
    )
    .await
    {
        Ok(Ok(bytes)) => bytes,
        Ok(Err(_)) => {
            refusal(res, StatusCode::PAYLOAD_TOO_LARGE, "body_rejected");
            return;
        }
        Err(_) => {
            refusal(res, StatusCode::REQUEST_TIMEOUT, "body_timeout");
            return;
        }
    };
    let delivery: Delivery = match serde_json::from_slice(bytes) {
        Ok(delivery) => delivery,
        Err(_) => {
            refusal(res, StatusCode::BAD_REQUEST, "invalid_delivery");
            return;
        }
    };
    let now = match SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| u64::try_from(d.as_millis()).ok())
    {
        Some(now) => now,
        None => {
            refusal(res, StatusCode::SERVICE_UNAVAILABLE, "clock_unavailable");
            return;
        }
    };
    match app.store.receive(delivery, now).await {
        Ok(receipt) => {
            res.status_code(StatusCode::ACCEPTED);
            res.render(Json(receipt));
        }
        Err(error) => {
            let (status, code) = match error {
                Error::Invalid(_) => (StatusCode::BAD_REQUEST, "invalid_delivery"),
                Error::Conflict => (StatusCode::CONFLICT, "idempotency_conflict"),
                Error::Generation => (StatusCode::CONFLICT, "generation_mismatch"),
                Error::Capacity => (
                    StatusCode::SERVICE_UNAVAILABLE,
                    "custody_capacity_exhausted",
                ),
                Error::Busy => (StatusCode::SERVICE_UNAVAILABLE, "busy"),
                Error::OutcomeUnknown => (StatusCode::GATEWAY_TIMEOUT, "outcome_unknown"),
                _ => (StatusCode::SERVICE_UNAVAILABLE, "storage_unavailable"),
            };
            refusal(res, status, code);
        }
    }
}

#[handler]
impl App {
    async fn handle(&self, depot: &mut Depot) {
        depot.insert_typed(self.clone());
    }
}
