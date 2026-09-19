//! Explicit one-attempt development startup; no production scheduler or file tool.
pub mod accounts;
mod approval;
mod config;
mod driver;
pub mod fleet;
pub mod intake_refusal;
pub(crate) mod palpo;
pub mod provision;
pub mod registration;
pub(crate) mod workspace;
use hagency_matrix::{CancellationToken, Collector};
use hagency_store::{
    DomainRepository, DomainStore, PEER_RETENTION_CEILING, Repository, Store, private,
};
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
    fn native_bootstrap_matrix_failure_projection() {
        assert_eq!(
            matrix_error_label(&hagency_matrix::Error::UnsafeSnapshot(
                "private fixture content".into()
            )),
            "unsafe_snapshot"
        );
        assert_eq!(
            matrix_error_label(&hagency_matrix::Error::Remote(429)),
            "remote"
        );
        let handle = StatusHandle::new(true);
        handle.matrix_refusal(&hagency_matrix::Error::UnsafeSnapshot(
            "private fixture content".into(),
        ));
        let value = serde_json::to_value(handle.get()).unwrap();
        assert_eq!(value["matrix_error"], "unsafe_snapshot");
        assert!(!value.to_string().contains("private fixture content"));
    }

    #[test]
    fn native_bootstrap_runtime_observation_projection() {
        use hagency_execution::{RuntimeObservation, RuntimeStage, RuntimeWriteObservation};
        use hagency_runtime::codex::{self, session, transport};
        let observation = RuntimeObservation {
            stage: RuntimeStage::ThreadStart,
            server_request: Some("permissions_approval"),
            refused_notification: Some("thread_status"),
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
                &hagency_execution::Failure::UnsupportedApproval,
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
        assert_eq!(value["runtime"]["refused_notification"], "thread_status");
        assert_eq!(value["runtime"].as_object().unwrap().len(), 9);
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
            server_request: None,
            refused_notification: None,
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

    #[tokio::test]
    async fn native_private_approval_close_retains_original() {
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
        let config = hagency_matrix::HostConfig::new(
            hagency_matrix::HostIdentity {
                server_name: "example.test".into(),
                registration_fingerprint: "a".repeat(64),
                transport: hagency_core::replies::MatrixTransportObservation {
                    engagement_id: "absent_approval_engagement".into(),
                    registration_generation: 1,
                    generation: 1,
                    sender_mxid: "@approval:example.test".into(),
                    device_id: "APPROVAL_DEVICE".into(),
                },
            },
            "https://127.0.0.1:1/",
            "synthetic-approval-close-token",
            state.join("approval-sdk"),
            [42; 32],
            vec![hagency_matrix::HostRoom {
                room_id: "!private:example.test".into(),
                generation: 1,
                privacy: hagency_core::replies::RoomPrivacy::Direct {
                    human_mxid: "@owner:example.test".into(),
                },
            }],
            hagency_matrix::Limits::default(),
        )
        .unwrap();
        let anchor = matrix_sdk_crypto::vodozemac::Ed25519SecretKey::new()
            .public_key()
            .to_base64();
        let collector = approval::collector(
            config,
            "absent_approval_engagement".into(),
            vec![("@owner:example.test".into(), anchor)],
            owner.domain.clone(),
        )
        .unwrap();
        let original = Arc::new(approval::Pump::new(collector, owner.domain.clone()));
        owner.approval = Some(original.clone());
        // This is a real, network-free close failure: the original domain
        // cannot resolve its missing engagement for negative fencing. It is
        // not an injected SDK shutdown result or positive shutdown proof.
        for _ in 0..2 {
            assert_eq!(owner.close().await, Err(Failure::OutcomeUnknown));
            assert!(Arc::ptr_eq(owner.approval.as_ref().unwrap(), &original));
            assert!(!owner.domain_closed && !owner.store_closed);
            assert!(matches!(
                DomainRepository::open(&state),
                Err(hagency_store::Error::Locked)
            ));
            assert!(matches!(
                Repository::open(&state),
                Err(hagency_store::Error::Locked)
            ));
        }
        assert!(!state.join("approval-sdk").exists());
        // Explicit fixture teardown does not turn the retained close into Ok.
        owner.domain.shutdown().await.unwrap();
        owner.store.shutdown().await.unwrap();
    }
}
// Existing authenticated operator diagnostics only. No raw report serialization.
#[derive(Clone, Serialize)]
struct RuntimeStatus {
    stage: &'static str,
    server_request: Option<&'static str>,
    refused_notification: Option<&'static str>,
    session_error: Option<&'static str>,
    transport_cause: Option<&'static str>,
    pending_requests: Option<usize>,
    pending_server_requests: Option<usize>,
    write_accepted_bytes: Option<usize>,
    write_total_bytes: Option<usize>,
}
fn owned_failure_label(error: &hagency_execution::Failure) -> &'static str {
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
        PeerUnavailable => "peer_unavailable",
        Worker => "worker",
        // ADR-142: the named refusal for a dispatch whose framework has no
        // native runner. The framework itself stays out of this fixed label
        // vocabulary; the operator-facing word is the failure variant's own.
        UnsupportedRunner { .. } => "unsupported_runner",
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
        TaskWriterStartup => "task_writer_startup",
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
        // The arm tag the runtime named — the operator surface can tell
        // "stdin write" from "stdout read" instead of the old four-way
        // "io" collapse. Fixed four-tag set, width-safe.
        Io(tag) => tag,
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
            server_request: observation.server_request,
            refused_notification: observation.refused_notification,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    matrix_error: Option<&'static str>,
    owned_failure: Option<&'static str>,
    settlement_cause: Option<&'static str>,
    runtime: Option<RuntimeStatus>,
}
#[derive(Clone)]
pub(crate) struct StatusHandle(Arc<Mutex<Status>>);
impl StatusHandle {
    #[cfg(test)]
    fn new(enabled: bool) -> Self {
        Self::for_mode(if enabled {
            DriverMode::OneAttempt
        } else {
            DriverMode::Disabled
        })
    }
    fn for_mode(mode: DriverMode) -> Self {
        Self(Arc::new(Mutex::new(Status {
            mode: mode.label(),
            state: if mode.enabled() {
                "prepared"
            } else {
                "disabled"
            },
            workspace_registered: false,
            protocol: None,
            cleanup: None,
            settlement: None,
            error: None,
            matrix_error: None,
            owned_failure: None,
            settlement_cause: None,
            runtime: None,
        })))
    }
    pub(crate) fn get(&self) -> Status {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
    /// The state word alone, for the readiness rollup (brief 19): the full
    /// `Status` stays console-only; `/health` names components by state
    /// words, never private detail.
    pub(crate) fn state(&self) -> &'static str {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).state
    }
    fn phase(&self, phase: &'static str) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).state = phase;
    }
    fn begin_attempt(&self) {
        let mut status = self.0.lock().unwrap_or_else(|e| e.into_inner());
        status.state = "refreshing";
        status.workspace_registered = false;
        status.protocol = None;
        status.cleanup = None;
        status.settlement = None;
        status.error = None;
        status.matrix_error = None;
        status.owned_failure = None;
        status.settlement_cause = None;
        status.runtime = None;
    }
    fn registered(&self) {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .workspace_registered = true;
    }
    fn matrix_refusal(&self, error: &hagency_matrix::Error) {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .matrix_error = Some(matrix_error_label(error));
    }
    fn handoff_refusal(&self, error: &hagency_execution::Failure) {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .owned_failure = Some(owned_failure_label(error));
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
        status.owned_failure = report.failure.as_ref().map(owned_failure_label);
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
pub(crate) fn matrix_error_label(error: &hagency_matrix::Error) -> &'static str {
    use hagency_matrix::Error::*;
    match error {
        Config => "config",
        Busy => "busy",
        Cancelled => "cancelled",
        Timeout => "timeout",
        Transport => "transport",
        Redirect => "redirect",
        Headers => "headers",
        BodyTooLarge => "body_too_large",
        InvalidJson => "invalid_json",
        Wire => "wire",
        Identity => "identity",
        Recipients => "recipients",
        Generation => "generation",
        Unauthorized => "unauthorized",
        Remote(_) => "remote",
        Storage => "storage",
        OutcomeUnknown => "outcome_unknown",
        Capacity => "capacity",
        Conflict => "conflict",
        Unsupported => "unsupported",
        UnsafeSnapshot(_) => "unsafe_snapshot",
        Domain => "domain",
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DriverMode {
    Disabled,
    OneAttempt,
    Continuous,
}
impl DriverMode {
    fn enabled(self) -> bool {
        self != Self::Disabled
    }
    fn label(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::OneAttempt => "one_attempt",
            Self::Continuous => "continuous",
        }
    }
}

#[derive(Default)]
pub struct Options {
    pub development_driver: bool,
    pub agent_driver: bool,
    pub palpo_transport: bool,
}

/// What one sweep tick observed: the outcome, or the refusal code when the
/// writer could not take the job. Diagnostic only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CeilingSweepTick {
    Swept(hagency_store::SweepOutcome),
    Refused(&'static str),
}

/// Production ceiling-overrun sweep cadence (ADR-124 slice b): the condition
/// is standing, so an hour is the retained default
/// (`backend-v2.js:17499-17504`); tests inject a short one via
/// [`Bootstrap::with_ceiling_sweep_period`].
pub const CEILING_SWEEP_PERIOD: Duration = Duration::from_secs(3600);

/// Installed-corpus ceiling for admitted messages (ADR-125). Retained default
/// 5000 (`backend-v2.js:236`, `MESSAGE_RETENTION_LIMIT`); the store clamps any
/// configured value up to `MESSAGE_RETENTION_FLOOR = 100` — the same
/// `Math.max(100, …)` guard. Reconciled by the `[retention]` sweep; never a
/// refusal of new admission.
pub const MESSAGE_RETENTION_CEILING: u64 = 5000;
/// One period for the whole retention task (tick contract §2.2): every
/// phase's condition is standing, so a missed tick is harmless and a shorter
/// period only re-does the same deferral.
pub const RETENTION_SWEEP_PERIOD: Duration = Duration::from_secs(60);
/// Per-phase writer budget (tick contract §2.3): each phase must complete
/// inside 600 ms — under a third of the callers' 2 s reply bound. A phase
/// that exceeds it halves its batch next tick (floor 1).
pub const RETENTION_PHASE_BUDGET_MS: u64 = 600;
/// The `messages` phase's initial batch (a hypothesis, not the bound — the
/// deadline is what actually stops it, tick contract §2.3).
const RETENTION_MESSAGES_BATCH: u64 = 512;
/// The `peer` phase's initial batch (ADR-125 peer phase, tick contract §2.3): the
/// same 512 hypothesis, its own deadline is the budget.
const RETENTION_PEER_BATCH: u64 = 512;
/// The `execution` phase's initial batch (tick contract §2.3's hypothesis for
/// this phase): 64 dispatches per tick, each dispatch's rows removed in the
/// phase's one transaction; the deadline is the budget.
const RETENTION_EXECUTION_BATCH: u64 = 64;
/// The `engagements` phase's initial batch hypothesis (tick contract §2.3):
/// each cascade prunes a whole reachable set, so the hypothesis starts an
/// order of magnitude below the messages one.
const RETENTION_ENGAGEMENTS_BATCH: u64 = 32;

/// The ceiling-overrun sweep loop (ADR-124 slice b): hourly in production
/// because the condition is standing — an agent past its ceiling at 09:00 is
/// still past it at 09:05, and a tighter loop would only re-file the same
/// alert (`backend-v2.js:17499-17504`). On `Busy` or `OutcomeUnknown` the
/// tick logs the refusal with the `[ceiling]` prefix and waits for the next
/// one: never an in-line retry, never blocking admission traffic — the sweep
/// is idempotent by dedupe key, so a missed tick is harmless. The returned
/// watch channel is the observation hook a test awaits; no sleep-based
/// polling. The loop exits when `shutdown` cancels: a tick aborted at its
/// await point is skipped or committed, never torn — a job the writer has
/// not dequeued is dropped whole, a dequeued one runs its single `Immediate`
/// transaction to commit — so cancellation means no NEW effect beyond the
/// current tick's atomic unit (ADR-124, "Abort-vs-commit on shutdown").
pub fn start_ceiling_sweep(
    domain: DomainStore,
    shutdown: CancellationToken,
    period: Duration,
) -> (
    tokio::task::JoinHandle<()>,
    tokio::sync::watch::Receiver<CeilingSweepTick>,
) {
    let (sender, observed) = tokio::sync::watch::channel(CeilingSweepTick::Refused("unstarted"));
    let handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(period);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = shutdown.cancelled() => break,
                _ = interval.tick() => {}
            }
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .ok()
                .and_then(|d| u64::try_from(d.as_millis()).ok())
                .unwrap_or_default();
            let tick = match domain.sweep_ceiling_overruns(now).await {
                Ok(outcome) => CeilingSweepTick::Swept(outcome),
                Err(hagency_store::Error::Busy) => {
                    tracing::warn!("[ceiling] sweep tick refused: busy; waiting for the next tick");
                    CeilingSweepTick::Refused("busy")
                }
                Err(hagency_store::Error::OutcomeUnknown) => {
                    tracing::warn!(
                        "[ceiling] sweep tick outcome unknown; waiting for the next tick"
                    );
                    CeilingSweepTick::Refused("outcome_unknown")
                }
                Err(error) => {
                    tracing::warn!(
                        "[ceiling] sweep tick failed: {error}; waiting for the next tick"
                    );
                    CeilingSweepTick::Refused("failed")
                }
            };
            let _ = sender.send(tick);
        }
    });
    (handle, observed)
}

/// What one retention tick observed for its `messages` phase: the outcome, or
/// the refusal code when the writer could not take the job. Diagnostic only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RetentionSweepTick {
    Swept(hagency_store::CorpusSweepOutcome),
    PeerSwept(hagency_store::PeerSweepOutcome),
    ExecutionSwept(hagency_store::ExecutionPruneOutcome),
    Refused(&'static str),
}

/// The per-phase batch hypotheses (tick contract §2.3, wiring review c):
/// one scalar capped by the `messages` const cannot carry the contract's
/// per-phase state — each phase owns its own, so one phase's over-budget
/// halving never disturbs another's hypothesis. Slice 1 lands `messages`;
/// later phases append their field here.
#[derive(Debug, Clone, Copy)]
struct RetentionBatches {
    messages: u64,
    peer: u64,
    execution: u64,
    engagements: u64,
}

impl Default for RetentionBatches {
    fn default() -> Self {
        Self {
            messages: RETENTION_MESSAGES_BATCH,
            peer: RETENTION_PEER_BATCH,
            execution: RETENTION_EXECUTION_BATCH,
            engagements: RETENTION_ENGAGEMENTS_BATCH,
        }
    }
}

/// The ONE retention sweep task (tick contract §1.5/§2.2): one period for
/// every phase, one sequential `Job::Run` submission per phase per tick.
/// Slice 1 lands the `messages` phase only (ADR-125); later slices append
/// their phase to this same loop, sharing the period and the reply bound.
/// On `Busy` or `OutcomeUnknown` a phase logs the refusal with the
/// `[retention]` prefix and waits for the next tick — never an in-line
/// retry. Each phase owns one `Immediate` transaction, so a tick is skipped
/// or committed, never torn. The measured `elapsed_ms` is logged every tick
/// (tick contract §2.5) and drives the batch-reduction rule: over
/// [`RETENTION_PHASE_BUDGET_MS`] halves the batch for the next tick, floor 1.
pub fn start_retention_sweep(
    domain: DomainStore,
    shutdown: CancellationToken,
    period: Duration,
    ceiling: u64,
    peer_ceiling: u64,
) -> (
    tokio::task::JoinHandle<()>,
    tokio::sync::watch::Receiver<RetentionSweepTick>,
) {
    let (sender, observed) = tokio::sync::watch::channel(RetentionSweepTick::Refused("unstarted"));
    let mut batches = RetentionBatches::default();
    let handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(period);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = shutdown.cancelled() => break,
                _ = interval.tick() => {}
            }
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .ok()
                .and_then(|d| u64::try_from(d.as_millis()).ok())
                .unwrap_or_default();
            // Phase 1 of the tick: `messages` (ADR-125). Phases land in the
            // contract's fixed order; between submissions the writer's FIFO
            // channel drains any foreground caller that arrived in between.
            let tick = match domain
                .sweep_admitted_corpus(now, ceiling, batches.messages)
                .await
            {
                Ok(outcome) => {
                    // The contract's measured budget rule (§2.3/§2.4): over
                    // budget halves the batch next tick; floor 1. Under
                    // budget restores the hypothesis upward one step. The
                    // hypothesis is PER PHASE (wiring review c): one scalar
                    // capped by the `messages` const cannot carry the
                    // contract's later phases, so each phase's halving
                    // never disturbs another's.
                    if outcome.elapsed_ms > RETENTION_PHASE_BUDGET_MS {
                        batches.messages = (batches.messages / 2).max(1);
                    } else if batches.messages < RETENTION_MESSAGES_BATCH {
                        batches.messages = (batches.messages * 2).min(RETENTION_MESSAGES_BATCH);
                    }
                    tracing::info!(
                        "[retention] messages phase: pruned {} remaining {} elapsed_ms {} batch {}",
                        outcome.pruned,
                        outcome.remaining,
                        outcome.elapsed_ms,
                        batches.messages
                    );
                    RetentionSweepTick::Swept(outcome)
                }
                Err(hagency_store::Error::Busy) => {
                    tracing::warn!(
                        "[retention] messages phase refused: busy; waiting for the next tick"
                    );
                    RetentionSweepTick::Refused("busy")
                }
                Err(hagency_store::Error::OutcomeUnknown) => {
                    tracing::warn!(
                        "[retention] messages phase outcome unknown; waiting for the next tick"
                    );
                    RetentionSweepTick::Refused("outcome_unknown")
                }
                Err(error) => {
                    tracing::warn!(
                        "[retention] messages phase failed: {error}; waiting for the next tick"
                    );
                    RetentionSweepTick::Refused("failed")
                }
            };
            let _ = sender.send(tick);
            // Phase 2 of the tick: `peer` (ADR-125), the same submission
            // shape as phase 1 — one sequential `Job::Run`, its own
            // `Immediate` transaction in the store, its own per-phase
            // batch hypothesis (wiring review c). The channel carries the
            // LAST phase's outcome per tick; the readiness mirror maps
            // both Swept variants to the same word.
            let tick = match domain
                .sweep_peer_corpus(now, peer_ceiling, batches.peer)
                .await
            {
                Ok(outcome) => {
                    if outcome.elapsed_ms > RETENTION_PHASE_BUDGET_MS {
                        batches.peer = (batches.peer / 2).max(1);
                    } else if batches.peer < RETENTION_PEER_BATCH {
                        batches.peer = (batches.peer * 2).min(RETENTION_PEER_BATCH);
                    }
                    tracing::info!(
                        "[retention] peer phase: pruned {} moved {} remaining {} elapsed_ms {} batch {}",
                        outcome.pruned,
                        outcome.moved,
                        outcome.remaining,
                        outcome.elapsed_ms,
                        batches.peer
                    );
                    RetentionSweepTick::PeerSwept(outcome)
                }
                Err(hagency_store::Error::Busy) => {
                    tracing::warn!(
                        "[retention] peer phase refused: busy; waiting for the next tick"
                    );
                    RetentionSweepTick::Refused("busy")
                }
                Err(hagency_store::Error::OutcomeUnknown) => {
                    tracing::warn!(
                        "[retention] peer phase outcome unknown; waiting for the next tick"
                    );
                    RetentionSweepTick::Refused("outcome_unknown")
                }
                Err(error) => {
                    tracing::warn!(
                        "[retention] peer phase failed: {error}; waiting for the next tick"
                    );
                    RetentionSweepTick::Refused("failed")
                }
            };
            let _ = sender.send(tick);
            // Phase 3 of the tick: `execution` (ADR-125, the ADR-053/031
            // amendments) — the per-dispatch execution corpus bound. One
            // sequential `Job::Run` after `peer`, its own `Immediate`
            // transaction in the store, its own per-phase batch hypothesis.
            let tick = match domain.prune_execution_corpus(now, batches.execution).await {
                Ok(outcome) => {
                    if outcome.elapsed_ms > RETENTION_PHASE_BUDGET_MS {
                        batches.execution = (batches.execution / 2).max(1);
                    } else if batches.execution < RETENTION_EXECUTION_BATCH {
                        batches.execution = (batches.execution * 2).min(RETENTION_EXECUTION_BATCH);
                    }
                    tracing::info!(
                        "[retention] execution phase: pruned {} remaining {} elapsed_ms {} batch {}",
                        outcome.pruned,
                        outcome.remaining,
                        outcome.elapsed_ms,
                        batches.execution
                    );
                    RetentionSweepTick::ExecutionSwept(outcome)
                }
                Err(hagency_store::Error::Busy) => {
                    tracing::warn!(
                        "[retention] execution phase refused: busy; waiting for the next tick"
                    );
                    RetentionSweepTick::Refused("busy")
                }
                Err(hagency_store::Error::OutcomeUnknown) => {
                    tracing::warn!(
                        "[retention] execution phase outcome unknown; waiting for the next tick"
                    );
                    RetentionSweepTick::Refused("outcome_unknown")
                }
                Err(error) => {
                    tracing::warn!(
                        "[retention] execution phase failed: {error}; waiting for the next tick"
                    );
                    RetentionSweepTick::Refused("failed")
                }
            };
            let _ = sender.send(tick);
            // Phase 4, `engagements` (ADR-095 Slice 6), last in the
            // contract's fixed order: its cascade is the longest, and the
            // earlier phases' deletes only shorten it. The watch channel's
            // shape still carries the last reported phase's outcome; this
            // phase reports through its own logs with the `[engagement]`
            // prefix the contract requires.
            match domain
                .sweep_engagements(now, hagency_store::ENDED_LIMIT, batches.engagements)
                .await
            {
                Ok(outcome) => {
                    if outcome.elapsed_ms > RETENTION_PHASE_BUDGET_MS {
                        batches.engagements = (batches.engagements / 2).max(1);
                    } else if batches.engagements < RETENTION_ENGAGEMENTS_BATCH {
                        batches.engagements =
                            (batches.engagements * 2).min(RETENTION_ENGAGEMENTS_BATCH);
                    }
                    tracing::info!(
                        "[engagement] engagements phase: pruned {} remaining {} elapsed_ms {} batch {}",
                        outcome.pruned,
                        outcome.remaining,
                        outcome.elapsed_ms,
                        batches.engagements
                    );
                }
                Err(hagency_store::Error::Busy) => {
                    tracing::warn!(
                        "[engagement] engagements phase refused: busy; waiting for the next tick"
                    );
                }
                Err(hagency_store::Error::OutcomeUnknown) => {
                    tracing::warn!(
                        "[engagement] engagements phase outcome unknown; waiting for the next tick"
                    );
                }
                Err(error) => {
                    tracing::warn!(
                        "[engagement] engagements phase failed: {error}; waiting for the next tick"
                    );
                }
            }
        }
    });
    (handle, observed)
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
    fleet: Option<fleet::Service>,
    shared: Option<Shared>,
    /// PC-C0: the approval bot's own collector (never the pooled ordinary
    /// one) and the pump's handoff channel. The pump is built at open; the
    /// forwarder is spawned on THIS service runtime in `serve`.
    approval: Option<Arc<approval::Pump>>,
    approval_sender: Option<tokio::sync::mpsc::Sender<hagency_execution::ApprovalRequests>>,
    approval_pump: Option<Arc<tokio::task::JoinHandle<()>>>,
    files: Option<crate::file_service::FileOwner>,
    receives: Option<crate::receive_service::ReceiveOwner>,
    collector_close: Option<tokio::task::JoinHandle<Result<(), hagency_matrix::Error>>>,
    collector_closed: Option<Result<(), Failure>>,
    status: StatusHandle,
    driver_mode: DriverMode,
    domain_closed: bool,
    store_closed: bool,
    ceiling_sweep_period: Duration,
    message_retention_ceiling: u64,
    peer_retention_ceiling: u64,
    retention_sweep_period: Duration,
    /// Shared with the readiness read in `App` (brief 19): bootstrap keeps
    /// this to abort at shutdown, `/health` observes liveness. Neither owns
    /// the loop alone.
    ceiling_sweep: Option<std::sync::Arc<tokio::task::JoinHandle<()>>>,
    /// The one retention sweep task's handle (tick contract §1.5): kept to
    /// abort at shutdown, exactly the ceiling sweep's shape above.
    retention_sweep: Option<std::sync::Arc<tokio::task::JoinHandle<()>>>,
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
                agent_driver: false,
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
        if options.development_driver && options.agent_driver {
            return Err(Failure::Config);
        }
        let driver_mode = if options.agent_driver {
            DriverMode::Continuous
        } else if options.development_driver {
            DriverMode::OneAttempt
        } else {
            DriverMode::Disabled
        };
        tracing::trace!(target: "hagency_startup_observation", "native startup boundary: bootstrap_entered");
        if !listen.ip().is_loopback() || listen.port() == 0 {
            return Err(Failure::Config);
        }
        private::directory(state).map_err(|_| Failure::Startup)?;
        let state = state.canonicalize().map_err(|_| Failure::Startup)?;
        let token =
            private::read_secret(&state.join("operator.token")).map_err(|_| Failure::Startup)?;
        tracing::trace!(target: "hagency_startup_observation", "native startup boundary: configuration_entered");
        let mut prepared = if driver_mode.enabled() {
            Some(config::Prepared::load(&state, listen, driver_mode)?)
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
        let mut repository = DomainRepository::open(&state).map_err(|_| Failure::Startup)?;
        // G6 (wiring audit): the receive-inbox plan names a workspace no
        // production path creates — `register_workspace` is the only
        // `workspace_resources` INSERT outside `recover_dispatch`, and the
        // plan's dispatch (enqueued by `select_receive_inbox`) references
        // that row. Register it before the first claim; a plan whose
        // workspace is missing from the map is already refused at config
        // load (config.rs), so this is the plan's named workspace, never a
        // widened set.
        if let Some(plan) = prepared.as_ref().and_then(|p| p.receive_inbox.as_ref()) {
            repository
                .register_workspace(&plan.workspace_id)
                .map_err(|_| Failure::Registration)?;
        }
        if let Some(prepared) = &prepared {
            for plan in &prepared.agent_inboxes {
                repository
                    .register_workspace(&plan.workspace_id)
                    .map_err(|_| Failure::Registration)?;
            }
        }
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
        // PC-C0 (plan v4 Q3): build the approval bot's OWN collector from the
        // second credential set — never the pooled ordinary `HostConfig` in
        // `Shared` (`Collector::new` refuses `approval == true`). Refuses at
        // startup with the named failure when the fresh-account enrollment
        // anchors are absent; no card can then be sent through any owner.
        let approval_collector = match prepared.as_mut().and_then(|p| p.approval.take()) {
            Some(approval) => Some(approval::collector(
                approval.config,
                approval.engagement_id,
                approval.anchors,
                domain.clone(),
            )?),
            None => None,
        };
        let root_engagement = prepared
            .as_ref()
            .and_then(|p| p.matrix.as_ref())
            .map(|matrix| matrix.engagement_id().to_owned());
        if let Some(prepared) = &mut prepared {
            prepared.attach_factory(approval_collector.clone())?;
        }
        let shared = prepared
            .as_mut()
            .map(|p| Shared::new(p.matrix.take().ok_or(Failure::Config)?, domain.clone()))
            .transpose()?;
        let approval = approval_collector
            .map(|collector| Arc::new(approval::Pump::new(collector, domain.clone())));
        tracing::trace!(target: "hagency_startup_observation", "native startup boundary: files_entered");
        let files = match (&shared, prepared.as_mut().and_then(|p| p.files.take())) {
            (Some(shared), Some(setup)) => Some(
                crate::file_service::FileOwner::start(shared.clone(), setup)
                    .map_err(|_| Failure::Startup)?,
            ),
            _ => None,
        };
        let status = StatusHandle::for_mode(driver_mode);
        let receives = match (&shared, prepared.as_mut().and_then(|p| p.receives.take())) {
            (Some(shared), Some(setup)) => Some(
                crate::receive_service::ReceiveOwner::start(shared.clone(), setup)
                    .map_err(|_| Failure::Startup)?,
            ),
            _ => None,
        };
        let fleet = match (&shared, prepared.as_mut().and_then(|p| p.fleet.take())) {
            (Some(shared), Some(setup)) => {
                let fleet = fleet::Service::new(domain.clone(), shared.collector.clone(), setup)?;
                fleet.register_root(
                    root_engagement.ok_or(Failure::Config)?,
                    files.as_ref().map(|owner| owner.handle()),
                    receives.as_ref().map(|owner| owner.handle()),
                    status.clone(),
                )?;
                Some(fleet)
            }
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
        if let Some(fleet) = &fleet {
            app = app.with_fleet(fleet);
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
            fleet,
            shared,
            approval,
            approval_sender: None,
            approval_pump: None,
            files,
            receives,
            collector_close: None,
            collector_closed: None,
            status,
            driver_mode,
            domain_closed: false,
            store_closed: false,
            ceiling_sweep_period: CEILING_SWEEP_PERIOD,
            message_retention_ceiling: MESSAGE_RETENTION_CEILING,
            peer_retention_ceiling: PEER_RETENTION_CEILING,
            retention_sweep_period: RETENTION_SWEEP_PERIOD,
            ceiling_sweep: None,
            retention_sweep: None,
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
    /// Override the ceiling-overrun sweep cadence (ADR-124 slice b). The
    /// production default is [`CEILING_SWEEP_PERIOD`] (hourly, the retained
    /// cadence); tests inject a short one here rather than sleeping.
    pub fn with_ceiling_sweep_period(mut self, period: Duration) -> Self {
        self.ceiling_sweep_period = period;
        self
    }
    /// Override the installed-corpus retention policy (ADR-125): the ceiling
    /// the `messages` phase reconciles to, and the one retention-tick period
    /// every phase shares (tick contract §2.2). The store clamps the ceiling
    /// up to the floor; a smaller ceiling only prunes sooner.
    pub fn with_message_retention(mut self, ceiling: u64, period: Duration) -> Self {
        self.message_retention_ceiling = ceiling;
        self.retention_sweep_period = period;
        self
    }
    /// Borrowing close retains this original owner/writer wrapper on failure.
    /// Driver receipts stay inspectable; the existing writer shutdown API may
    /// return an unknown final outcome. Neither wrapper presence nor timeout
    /// proves its repository remains open or has closed. Retain this Bootstrap.
    pub async fn close(&mut self) -> Result<(), Failure> {
        if let Some(fleet) = &self.fleet {
            fleet.quiesce();
        }
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
        if let Some(sweep) = &mut self.ceiling_sweep {
            sweep.abort();
        }
        // Wiring review a: the retention task's handle is read here — the
        // abort-on-shutdown its doc comment promises, the ceiling sweep's
        // shape.
        if let Some(sweep) = &mut self.retention_sweep {
            sweep.abort();
        }
        if let Some(palpo) = &self.palpo {
            palpo.cancel();
        }
        if let Some(shared) = &self.shared {
            shared.workspace.retire();
        }
        let mut children_failed = false;
        if let Some(files) = &mut self.files {
            children_failed |= files.close().await.is_err();
        }
        if let Some(receives) = &mut self.receives {
            children_failed |= receives.close().await.is_err();
        }
        if let Some(driver) = &mut self.driver {
            children_failed |= driver.close().await.is_err();
        }
        if let Some(palpo) = &mut self.palpo {
            children_failed |= palpo.close().await.is_err();
        }
        if let Some(fleet) = &mut self.fleet {
            children_failed |= fleet.drain_agents().await.is_err();
        }
        if children_failed {
            return Err(Failure::OutcomeUnknown);
        }
        if let Some(fleet) = &mut self.fleet {
            fleet.close().await?;
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
        // PC-C0: stop the forwarder and close the approval bot's own
        // collector beside the ordinary one — same bounded shape, the
        // original owner retained on an unknown outcome.
        if let Some(pump) = self.approval_pump.take() {
            pump.abort();
        }
        self.approval_sender = None;
        if let Some(pump) = self.approval.as_ref() {
            pump.close().await?;
        }
        self.approval = None;
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
        // Hourly ceiling-overrun sweep (ADR-124 slice b): started beside the
        // other background owners, after the writer exists and BEFORE the
        // router snapshot below — the served `App` must carry the handle and
        // tick channel from the first request, or readiness would report the
        // loop `disabled` forever (brief 19). The handle is shared: bootstrap
        // keeps it to abort at shutdown, `/health` observes liveness.
        let (ceiling_sweep, sweep_tick) = start_ceiling_sweep(
            self.domain.clone(),
            shutdown.clone(),
            self.ceiling_sweep_period,
        );
        let sweep = std::sync::Arc::new(ceiling_sweep);
        self.ceiling_sweep = Some(sweep.clone());
        self.app = self.app.clone().with_ceiling_sweep(sweep, sweep_tick);
        // The ONE retention sweep task (tick contract §1.5), beside the
        // ceiling task — same `start_*_sweep` shape, its own period and its
        // own watch channel; slice 1 lands the `messages` phase only.
        let (retention_sweep, retention_tick) = start_retention_sweep(
            self.domain.clone(),
            shutdown.clone(),
            self.retention_sweep_period,
            self.message_retention_ceiling,
            self.peer_retention_ceiling,
        );
        let retention = std::sync::Arc::new(retention_sweep);
        self.retention_sweep = Some(retention.clone());
        // Wiring review a/b: the handle IS read — aborted at shutdown — and
        // the tick channel is kept, not dropped, so the loop has the same
        // liveness/observation hook the ceiling sweep has.
        self.app = self
            .app
            .clone()
            .with_retention_sweep(retention, retention_tick);
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
        if let Some(pump) = self.approval.as_ref() {
            self.status.phase("enrolling");
            // Keep the actual listener/router polled while the original bot
            // establishes its private Matrix binding and encryption custody.
            // A failed startup cannot admit a runner or be retried here.
            tokio::select! {
                result = &mut serving => {result.map_err(|_| Failure::Server)?; return Err(Failure::Server);},
                result = pump.initialize(shutdown) => result?,
            }
        }
        if !shutdown.is_cancelled()
            && let Some(prepared) = self.prepared.take()
        {
            // PC-C0 (plan v4 Q1): the handoff channel exists ONLY when the
            // approval pump is configured. The forwarder is spawned HERE, on
            // the service's multi-threaded runtime — host/service scope —
            // and the driver receives only the sender half. The pump ends by
            // itself after all original senders and handoffs close. An idle
            // source cannot hold later agents behind its receiver lifetime.
            if self.approval.is_some() && self.approval_pump.is_none() {
                let (sender, receiver) = tokio::sync::mpsc::channel(1);
                let pump = self.approval.clone().expect("approval pump");
                let cancel = shutdown.clone();
                let status = self.status.clone();
                self.approval_pump = Some(std::sync::Arc::new(tokio::spawn(async move {
                    if let Err(error) = pump.drain(receiver, &cancel).await {
                        status.fail(error);
                    }
                })));
                self.approval_sender = Some(sender);
            }
            self.driver = Some(driver::Driver::start(
                prepared,
                self.shared.clone().ok_or(Failure::Startup)?,
                self.files.as_ref().map(|files| files.handle()),
                self.status.clone(),
                self.approval_sender.clone(),
                self.driver_mode,
            )?);
        }
        tracing::trace!(target: "hagency_startup_observation", "native startup boundary: serving");
        tracing::info!("native service ready; production Agent execution remains unavailable");
        let service_error = tokio::select! {
            result=&mut serving=>{ result.map_err(|_|Failure::Server)?; return Err(Failure::Server); },
            _=shutdown.cancelled()=>None,
            result=async {
                match &mut self.fleet {
                    Some(fleet)=>fleet.run(self.approval_sender.clone().ok_or(Failure::Config)?,shutdown).await,
                    None=>std::future::pending::<Result<(),Failure>>().await,
                }
            }=>result.err(),
        };
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
        serving.await.map_err(|_| Failure::Server)?;
        service_error.map_or(Ok(()), Err)
    }
}
