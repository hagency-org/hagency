//! Explicit one-attempt development startup; no production scheduler or file tool.
pub mod accounts;
mod config;
mod driver;
pub(crate) mod palpo;
pub(crate) mod workspace;
use hagency_matrix::{CancellationToken, Collector};
use hagency_store::{DomainRepository, DomainStore, Repository, Store, private};
use salvo::prelude::*;
use serde::Serialize;
use std::{
    net::SocketAddr,
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Failure {
    #[error("development profile is invalid or unavailable")]
    Config,
    #[error("native startup owner is unavailable")]
    Startup,
    #[error("current Matrix refresh was refused")]
    Refresh,
    #[error("workspace registration is unavailable")]
    Registration,
    #[error("development attempt was cancelled")]
    Cancelled,
    #[error("development worker is unavailable")]
    Worker,
    #[error("original owned outcome is unknown; owner retained")]
    OutcomeUnknown,
    #[error("native server failed")]
    Server,
}

#[cfg(test)]
mod custody_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn native_bootstrap_runtime_observation_projection() {
        use hagency_execution::{RuntimeObservation, RuntimeStage, RuntimeWriteObservation};
        use hagency_runtime::codex::{self, session, transport};
        let observation = RuntimeObservation {
            stage: RuntimeStage::ThreadStart,
            session_error: Some(session::Error::UnsupportedRequest),
            transport_cause: Some(transport::Error::Protocol(codex::Error::UnexpectedEof)),
            pending_requests: Some(usize::MAX),
            pending_server_requests: Some(usize::MAX),
            write: Some(RuntimeWriteObservation {
                accepted_bytes: usize::MAX,
                total_bytes: usize::MAX,
            }),
        };
        // Longest labels and maximum-width counts bound the whole existing
        // operator status projection, not only a typical runtime observation.
        let handle = StatusHandle::new(true);
        {
            let mut status = handle.0.lock().unwrap();
            status.runtime = Some(RuntimeStatus::from(&observation));
            status.owned_failure = Some(owned_failure_label(
                hagency_execution::Failure::UnsupportedApproval,
            ));
            status.protocol = Some("not_started");
            status.cleanup = Some("whole_tree_stopped");
            status.settlement = Some("canonical_reply_ready");
        }
        handle.fail(Failure::OutcomeUnknown);
        let value = serde_json::to_value(handle.get()).unwrap();
        assert_eq!(value["state"], "outcome_unknown");
        assert_eq!(value["error"], "outcome_unknown");
        assert_eq!(
            value["runtime"]["transport_cause"],
            "protocol_unexpected_eof"
        );
        assert_eq!(value["runtime"]["write_accepted_bytes"], usize::MAX);
        assert_eq!(value["runtime"].as_object().unwrap().len(), 7);
        assert!(serde_json::to_vec(&value).unwrap().len() <= 768);
        assert_eq!(
            session_error_label(session::Error::Rejected(i64::MIN)),
            "rejected"
        );
        assert_eq!(
            session_error_label(session::Error::Rejected(i64::MAX)),
            "rejected"
        );
        let absent = RuntimeObservation {
            stage: RuntimeStage::Update,
            session_error: None,
            transport_cause: None,
            pending_requests: None,
            pending_server_requests: None,
            write: None,
        };
        let value = serde_json::to_value(RuntimeStatus::from(&absent)).unwrap();
        assert_eq!(value["stage"], "update");
        assert!(
            value
                .as_object()
                .unwrap()
                .iter()
                .filter(|(key, _)| *key != "stage")
                .all(|(_, value)| value.is_null())
        );
        let disabled = serde_json::to_value(StatusHandle::new(false).get()).unwrap();
        assert_eq!(disabled["state"], "disabled");
        assert!(disabled["runtime"].is_null());
        assert!(disabled["owned_failure"].is_null());
    }

    #[tokio::test]
    async fn native_bootstrap_custody_consumed_close_ack() {
        // Preserve the original consuming-API protocol regression at its new
        // Bootstrap owner. A modeled close result is not real SDK shutdown proof.
        for failed in [true, false] {
            let fixture = crate::file_service::test_common::Fixture::new();
            fixture.store.shutdown().await.unwrap();
            let state = fixture.root.path().join("domain");
            private::write_new(
                &state.join("operator.token"),
                b"fixture_operator_token_32_bytes_minimum",
            )
            .unwrap();
            let mut owner =
                Bootstrap::open(&state, "127.0.0.1:13300".parse().unwrap(), 16, false).unwrap();
            owner.shared = Some(
                Shared::new(fixture.config("https://127.0.0.1:1/"), owner.domain.clone()).unwrap(),
            );
            let original = Arc::new(Mutex::new(Some(())));
            let calls = Arc::new(AtomicUsize::new(0));
            let (retained, called) = (original.clone(), calls.clone());
            owner.collector_close = Some(tokio::spawn(async move {
                assert!(retained.lock().unwrap().take().is_some());
                called.fetch_add(1, Ordering::SeqCst);
                if failed {
                    Err(hagency_matrix::Error::OutcomeUnknown)
                } else {
                    Ok(())
                }
            }));
            let expected = if failed {
                Err(Failure::OutcomeUnknown)
            } else {
                Ok(())
            };
            assert_eq!(owner.close().await, expected);
            assert_eq!(owner.close().await, expected);
            assert_eq!(calls.load(Ordering::SeqCst), 1);
            assert!(original.lock().unwrap().is_none());
            assert!(owner.collector_close.is_none());
            assert_eq!(owner.collector_closed, Some(expected));
            assert_eq!(owner.domain_closed, !failed);
            assert_eq!(owner.store_closed, !failed);
            if failed {
                assert!(matches!(
                    DomainRepository::open(&state),
                    Err(hagency_store::Error::Locked)
                ));
                assert!(matches!(
                    Repository::open(&state),
                    Err(hagency_store::Error::Locked)
                ));
                // Explicit fixture teardown does not acknowledge the failed close.
                owner.domain.shutdown().await.unwrap();
                owner.store.shutdown().await.unwrap();
            }
        }
    }
}
// Existing authenticated operator diagnostics only. No raw report serialization.
#[derive(Clone, Serialize)]
struct RuntimeStatus {
    stage: &'static str,
    session_error: Option<&'static str>,
    transport_cause: Option<&'static str>,
    pending_requests: Option<usize>,
    pending_server_requests: Option<usize>,
    write_accepted_bytes: Option<usize>,
    write_total_bytes: Option<usize>,
}
fn owned_failure_label(error: hagency_execution::Failure) -> &'static str {
    use hagency_execution::Failure::*;
    match error {
        Admission => "admission",
        Cancelled => "cancelled",
        StartUnknown => "start_unknown",
        UsageBinding => "usage_binding",
        SpawnFailed => "spawn_failed",
        LostAuthority => "lost_authority",
        Protocol => "protocol",
        UnsupportedApproval => "unsupported_approval",
        ApprovalCapacity => "approval_capacity",
        ApprovalCancelled => "approval_cancelled",
        Deadline => "deadline",
        CleanupUnknown => "cleanup_unknown",
        SettlementUnknown => "settlement_unknown",
        Worker => "worker",
    }
}
/// Bounded projection of which store refusal produced a settlement failure.
/// Diagnostic only: a fixed label, never authority, retry, reply or lease
/// input, and never the store's own error text.
fn settlement_cause_label(cause: hagency_execution::SettlementCause) -> &'static str {
    use hagency_execution::SettlementCause::*;
    match cause {
        QueueBusy => "queue_busy",
        QueueUnavailable => "queue_unavailable",
        ReplyTimedOut => "reply_timed_out",
        RunnerAuthority => "runner_authority",
        State => "state",
        Quarantined => "quarantined",
        Storage => "storage",
        AcceptanceUnrecorded => "acceptance_unrecorded",
    }
}
fn session_error_label(error: hagency_runtime::codex::session::Error) -> &'static str {
    use hagency_runtime::codex::session::Error::*;
    match error {
        Settings => "settings",
        State => "state",
        Scope => "scope",
        Malformed => "malformed",
        Capacity => "capacity",
        Policy => "policy",
        Rejected(_) => "rejected",
        UnsupportedRequest => "unsupported_request",
        UnsupportedEvent => "unsupported_event",
        Cancelled => "cancelled",
        Transport(_) => "transport",
    }
}
fn transport_error_label(error: hagency_runtime::codex::transport::Error) -> &'static str {
    use hagency_runtime::codex::{Error as WireError, transport::Error::*};
    match error {
        Configuration => "configuration",
        Closed => "closed",
        CancelledOperation => "cancelled_operation",
        Timeout => "timeout",
        Io => "io",
        PeerEof => "peer_eof",
        Capacity => "capacity",
        HostClosed => "host_closed",
        Protocol(error) => match error {
            WireError::Closed => "protocol_closed",
            WireError::State => "protocol_state",
            WireError::Envelope => "protocol_envelope",
            WireError::Capacity => "protocol_capacity",
            WireError::Identity => "protocol_identity",
            WireError::Timeout => "protocol_timeout",
            WireError::Clock => "protocol_clock",
            WireError::UnexpectedEof => "protocol_unexpected_eof",
            WireError::Transport => "protocol_transport",
        },
    }
}
impl From<&hagency_execution::RuntimeObservation> for RuntimeStatus {
    fn from(observation: &hagency_execution::RuntimeObservation) -> Self {
        use hagency_execution::RuntimeStage;
        Self {
            stage: match observation.stage {
                RuntimeStage::Initialize => "initialize",
                RuntimeStage::ThreadStart => "thread_start",
                RuntimeStage::TurnStart => "turn_start",
                RuntimeStage::Update => "update",
            },
            session_error: observation.session_error.map(session_error_label),
            transport_cause: observation.transport_cause.map(transport_error_label),
            pending_requests: observation.pending_requests,
            pending_server_requests: observation.pending_server_requests,
            write_accepted_bytes: observation.write.map(|w| w.accepted_bytes),
            write_total_bytes: observation.write.map(|w| w.total_bytes),
        }
    }
}

#[derive(Clone, Serialize)]
pub struct Status {
    mode: &'static str,
    state: &'static str,
    workspace_registered: bool,
    protocol: Option<&'static str>,
    cleanup: Option<&'static str>,
    settlement: Option<&'static str>,
    error: Option<&'static str>,
    owned_failure: Option<&'static str>,
    settlement_cause: Option<&'static str>,
    runtime: Option<RuntimeStatus>,
}
#[derive(Clone)]
pub(crate) struct StatusHandle(Arc<Mutex<Status>>);
impl StatusHandle {
    fn new(enabled: bool) -> Self {
        Self(Arc::new(Mutex::new(Status {
            mode: if enabled { "one_attempt" } else { "disabled" },
            state: if enabled { "prepared" } else { "disabled" },
            workspace_registered: false,
            protocol: None,
            cleanup: None,
            settlement: None,
            error: None,
            owned_failure: None,
            settlement_cause: None,
            runtime: None,
        })))
    }
    pub(crate) fn get(&self) -> Status {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
    fn phase(&self, phase: &'static str) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).state = phase;
    }
    fn registered(&self) {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .workspace_registered = true;
    }
    fn fail(&self, failure: Failure) {
        let mut status = self.0.lock().unwrap_or_else(|e| e.into_inner());
        status.state = if matches!(failure, Failure::OutcomeUnknown | Failure::Worker) {
            "outcome_unknown"
        } else {
            "unavailable"
        };
        status.error = Some(match failure {
            Failure::Config => "config",
            Failure::Startup => "startup",
            Failure::Refresh => "refresh",
            Failure::Registration => "registration",
            Failure::Cancelled => "cancelled",
            Failure::Worker => "worker",
            Failure::OutcomeUnknown => "outcome_unknown",
            Failure::Server => "server",
        });
    }
    fn result(&self, report: &hagency_execution::Report) {
        use hagency_execution::{Protocol, Settlement};
        use hagency_runtime::owned::Cleanup;
        let mut status = self.0.lock().unwrap_or_else(|e| e.into_inner());
        status.owned_failure = report.failure.map(owned_failure_label);
        status.settlement_cause = report.settlement_cause.map(settlement_cause_label);
        status.runtime = report.runtime_observation().map(RuntimeStatus::from);
        status.protocol = Some(match report.protocol {
            Protocol::NotStarted => "not_started",
            Protocol::Completed => "completed",
            Protocol::Failed => "failed",
            Protocol::Interrupted => "interrupted",
            Protocol::Unsupported => "unsupported",
            Protocol::Unknown => "unknown",
        });
        status.cleanup = Some(match report.cleanup {
            Cleanup::Pending => "pending",
            Cleanup::Observed(v) if v.scope.whole_tree_stopped => "whole_tree_stopped",
            _ => "unknown",
        });
        status.settlement = Some(match report.settlement {
            Settlement::Pending => "pending",
            Settlement::Completed => "completed",
            Settlement::CanonicalReplyReady => "canonical_reply_ready",
            Settlement::Negative(_) => "negative",
            Settlement::Unknown => "unknown",
        });
        status.state = if report.failure.is_none() {
            "completed"
        } else {
            "outcome_unknown"
        };
        if report.failure.is_some() {
            status.error = Some("owned_attempt");
        }
    }
}
/// One original account/writer/root owner shared only inside the application.
#[derive(Clone)]
pub(crate) struct Shared {
    pub(crate) domain: DomainStore,
    pub(crate) collector: Arc<Collector>,
    pub(crate) workspace: workspace::WorkspaceAccess,
}
impl Shared {
    fn new(matrix: hagency_matrix::HostConfig, domain: DomainStore) -> Result<Self, Failure> {
        Ok(Self {
            collector: Arc::new(
                Collector::new(matrix, domain.clone()).map_err(|_| Failure::Config)?,
            ),
            domain,
            workspace: workspace::WorkspaceAccess::new(),
        })
    }
}
#[derive(Default)]
pub struct Options {
    pub development_driver: bool,
    pub palpo_transport: bool,
}

pub struct Bootstrap {
    store: Store,
    domain: DomainStore,
    app: crate::App,
    listen: SocketAddr,
    prepared: Option<config::Prepared>,
    palpo_prepared: Option<palpo::Prepared>,
    palpo: Option<palpo::Owner>,
    palpo_status: palpo::StatusHandle,
    driver: Option<driver::Driver>,
    shared: Option<Shared>,
    files: Option<crate::file_service::FileOwner>,
    receives: Option<crate::receive_service::ReceiveOwner>,
    collector_close: Option<tokio::task::JoinHandle<Result<(), hagency_matrix::Error>>>,
    collector_closed: Option<Result<(), Failure>>,
    status: StatusHandle,
    domain_closed: bool,
    store_closed: bool,
}
impl Bootstrap {
    /// Own fresh development state. No live repository, .env, arbitrary command
    /// or raw runner capability is accepted from configuration or HTTP.
    pub fn open(
        state: &Path,
        listen: SocketAddr,
        queue_capacity: usize,
        development: bool,
    ) -> Result<Self, Failure> {
        Self::open_with_options(
            state,
            listen,
            queue_capacity,
            Options {
                development_driver: development,
                palpo_transport: false,
            },
        )
    }

    /// Independent fixed private startup profiles. Existing registration is a
    /// Palpo prerequisite; startup does not register or rotate domain authority.
    pub fn open_with_options(
        state: &Path,
        listen: SocketAddr,
        queue_capacity: usize,
        options: Options,
    ) -> Result<Self, Failure> {
        let development = options.development_driver;
        tracing::trace!(target: "hagency_startup_observation", "native startup boundary: bootstrap_entered");
        if !listen.ip().is_loopback() || listen.port() == 0 {
            return Err(Failure::Config);
        }
        private::directory(state).map_err(|_| Failure::Startup)?;
        let state = state.canonicalize().map_err(|_| Failure::Startup)?;
        let token =
            private::read_secret(&state.join("operator.token")).map_err(|_| Failure::Startup)?;
        tracing::trace!(target: "hagency_startup_observation", "native startup boundary: configuration_entered");
        let mut prepared = if development {
            Some(config::Prepared::load(&state, listen)?)
        } else {
            None
        };
        let palpo_prepared = if options.palpo_transport {
            Some(palpo::Prepared::load(&state)?)
        } else {
            None
        };
        let palpo_status = palpo::StatusHandle::new(options.palpo_transport);
        tracing::trace!(target: "hagency_startup_observation", "native startup boundary: custody_entered");
        let store = Store::start(
            Repository::open(&state).map_err(|_| Failure::Startup)?,
            queue_capacity,
        )
        .map_err(|_| Failure::Startup)?;
        tracing::trace!(target: "hagency_startup_observation", "native startup boundary: domain_entered");
        let repository = DomainRepository::open(&state).map_err(|_| Failure::Startup)?;
        prepared = prepared
            .map(|mut prepared| {
                if let Some(id) = prepared.managed_account.take() {
                    let account = repository
                        .managed_account(&id)
                        .map_err(|_| Failure::Config)?;
                    prepared.claim = account
                        .bind_claim_profile(prepared.claim)
                        .map_err(|_| Failure::Config)?;
                    prepared.host = prepared
                        .host
                        .with_managed_account(account)
                        .map_err(|_| Failure::Config)?;
                }
                Ok::<_, Failure>(prepared)
            })
            .transpose()?;
        let domain =
            DomainStore::start(repository, queue_capacity).map_err(|_| Failure::Startup)?;
        tracing::trace!(target: "hagency_startup_observation", "native startup boundary: shared_entered");
        let shared = prepared
            .as_mut()
            .map(|p| Shared::new(p.matrix.take().ok_or(Failure::Config)?, domain.clone()))
            .transpose()?;
        tracing::trace!(target: "hagency_startup_observation", "native startup boundary: files_entered");
        let files = match (&shared, prepared.as_mut().and_then(|p| p.files.take())) {
            (Some(shared), Some(setup)) => Some(
                crate::file_service::FileOwner::start(shared.clone(), setup)
                    .map_err(|_| Failure::Startup)?,
            ),
            _ => None,
        };
        let status = StatusHandle::new(development);
        let receives = match (&shared, prepared.as_mut().and_then(|p| p.receives.take())) {
            (Some(shared), Some(setup)) => Some(
                crate::receive_service::ReceiveOwner::start(shared.clone(), setup)
                    .map_err(|_| Failure::Startup)?,
            ),
            _ => None,
        };
        tracing::trace!(target: "hagency_startup_observation", "native startup boundary: app_entered");
        let mut app = crate::App::new(store.clone(), &token, listen)
            .map_err(|_| Failure::Startup)?
            .with_domain(domain.clone())
            .with_development(status.clone())
            .with_palpo(palpo_status.clone());
        if let Some(files) = &files {
            app = app.with_files(files.handle());
        }
        if let Some(receives) = &receives {
            app = app.with_receive_service(receives.handle());
        }
        tracing::trace!(target: "hagency_startup_observation", "native startup boundary: bootstrap_ready");
        Ok(Self {
            store,
            domain,
            app,
            listen,
            prepared,
            palpo_prepared,
            palpo: None,
            palpo_status,
            driver: None,
            shared,
            files,
            receives,
            collector_close: None,
            collector_closed: None,
            status,
            domain_closed: false,
            store_closed: false,
        })
    }
    pub fn status(&self) -> Status {
        self.status.get()
    }
    /// Install an already validated startup asset owner before HTTP admission.
    pub fn with_console(mut self, console: crate::console::Console) -> Self {
        self.app = self.app.with_console(console);
        self
    }
    /// Borrowing close retains this original owner/writer wrapper on failure.
    /// Driver receipts stay inspectable; the existing writer shutdown API may
    /// return an unknown final outcome. Neither wrapper presence nor timeout
    /// proves its repository remains open or has closed. Retain this Bootstrap.
    pub async fn close(&mut self) -> Result<(), Failure> {
        if let Some(console) = &self.app.console {
            console.retire();
        }
        if let Some(receives) = &self.receives {
            receives.quiesce();
        }
        if let Some(files) = &self.files {
            files.quiesce();
        }
        if let Some(driver) = &self.driver {
            driver.cancel();
        }
        if let Some(palpo) = &self.palpo {
            palpo.cancel();
        }
        if let Some(shared) = &self.shared {
            shared.workspace.retire();
        }
        if let Some(files) = &mut self.files {
            files.close().await.map_err(|_| Failure::OutcomeUnknown)?;
        }
        if let Some(receives) = &mut self.receives {
            receives
                .close()
                .await
                .map_err(|_| Failure::OutcomeUnknown)?;
        }
        if let Some(driver) = &mut self.driver {
            driver.close().await?;
        }
        if let Some(palpo) = &mut self.palpo {
            palpo.close().await?;
        }
        if let Some(shared) = &self.shared {
            if let Some(result) = self.collector_closed {
                result?;
            } else {
                if self.collector_close.is_none() {
                    let collector = shared.collector.clone();
                    self.collector_close =
                        Some(tokio::spawn(async move { collector.close().await }));
                }
                let result = match tokio::time::timeout(
                    Duration::from_secs(2),
                    self.collector_close
                        .as_mut()
                        .ok_or(Failure::OutcomeUnknown)?,
                )
                .await
                {
                    Ok(Ok(Ok(()))) => Ok(()),
                    Ok(_) => Err(Failure::OutcomeUnknown),
                    Err(_) => return Err(Failure::OutcomeUnknown),
                };
                self.collector_close = None;
                self.collector_closed = Some(result);
                result?;
            }
        }
        if !self.domain_closed {
            self.domain
                .shutdown()
                .await
                .map_err(|_| Failure::OutcomeUnknown)?;
            self.domain_closed = true;
        }
        if !self.store_closed {
            self.store
                .shutdown()
                .await
                .map_err(|_| Failure::OutcomeUnknown)?;
            self.store_closed = true;
        }
        self.status.phase("closed");
        Ok(())
    }
    pub async fn serve(&mut self, shutdown: &CancellationToken) -> Result<(), Failure> {
        tracing::trace!(target: "hagency_startup_observation", "native startup boundary: bind_entered");
        let acceptor = TcpListener::new(self.listen)
            .try_bind()
            .await
            .map_err(|_| Failure::Server)?;
        tracing::trace!(target: "hagency_startup_observation", "native startup boundary: server_poll_entered");
        let server = Server::new(acceptor).max_connections(64);
        let handle = server.handle();
        let mut serving = Box::pin(server.try_serve(self.app.clone().router()));
        // Poll the real server first. Its listener/router exist before any child
        // or helper can try to connect; no fixture-only readiness setter.
        tokio::select! { biased; result=&mut serving=>{result.map_err(|_|Failure::Server)?;return Err(Failure::Server);}, _=tokio::task::yield_now()=>{} }
        tracing::trace!(target: "hagency_startup_observation", "native startup boundary: driver_entered");
        if let Some(prepared) = self.palpo_prepared.take() {
            self.palpo = Some(palpo::Owner::start(
                prepared,
                self.store.clone(),
                self.domain.clone(),
                self.palpo_status.clone(),
            ));
        }
        if let Some(prepared) = self.prepared.take() {
            self.driver = Some(driver::Driver::start(
                prepared,
                self.shared.clone().ok_or(Failure::Startup)?,
                self.files.as_ref().map(|files| files.handle()),
                self.status.clone(),
            )?);
        }
        tracing::trace!(target: "hagency_startup_observation", "native startup boundary: serving");
        tracing::info!("native service ready; production Agent execution remains unavailable");
        tokio::select! {
            result=&mut serving=>{ result.map_err(|_|Failure::Server)?; return Err(Failure::Server); },
            _=shutdown.cancelled()=>{}
        }
        if let Some(driver) = &self.driver {
            driver.cancel();
        }
        let outcome = tokio::select! {
            result=&mut serving=>{result.map_err(|_|Failure::Server)?;return Err(Failure::Server);},
            result=self.close()=>result,
        };
        if outcome.is_err() {
            self.status.fail(Failure::OutcomeUnknown);
            tracing::error!("native shutdown incomplete; original owner retained");
            // Keep the authenticated fixed status endpoint and the original
            // owner. No automatic close/claim retry or false successful exit.
            return serving
                .await
                .map_err(|_| Failure::Server)
                .and(Err(Failure::OutcomeUnknown));
        }
        handle.stop_graceful(Some(Duration::from_secs(5)));
        serving.await.map_err(|_| Failure::Server)
    }
}
