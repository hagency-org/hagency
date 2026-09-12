use crate::registration::{Gate, RegistrationSlot, WorkspaceRegistration};
use crate::usage::{UsageFailure, UsageRun, UsageStatus};
use crate::workspace::{Binding, Handoff};
use crate::{Host, Limits, StartedWorkspace};
use hagency_core::tasks::{RunnerCapability, RunnerCommand, Task, TaskState};
use hagency_runtime::{
    codex::session::{self, Outcome, Update},
    owned::{Cleanup, OwnedSession},
};
use hagency_store::{DomainStore, OwnedFailure, OwnedObservation};
use std::{
    future::Future,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::Duration,
};
use tokio::{
    sync::oneshot,
    time::{Instant, MissedTickBehavior, interval},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Failure {
    #[error("host dispatch admission refused")]
    Admission,
    #[error("host operation cancelled")]
    Cancelled,
    #[error("durable start response unknown; no child launched")]
    StartUnknown,
    #[error("usage source binding failed or its response is unknown; no child launched")]
    UsageBinding,
    #[error("owned native child startup failed")]
    SpawnFailed,
    #[error("dispatch authority expired, changed or was revoked")]
    LostAuthority,
    #[error("native runner protocol failed")]
    Protocol,
    #[error("owned approval application is unavailable")]
    UnsupportedApproval,
    #[error("owned approval capacity exhausted")]
    ApprovalCapacity,
    #[error("original approval callback was cancelled before response admission")]
    ApprovalCancelled,
    #[error("host operation deadline expired")]
    Deadline,
    #[error("whole-tree cleanup remains unproven")]
    CleanupUnknown,
    #[error("domain settlement outcome unknown")]
    SettlementUnknown,
    #[error("host worker failed")]
    Worker,
}
impl Failure {
    fn observation(self) -> OwnedFailure {
        match self {
            Self::Admission | Self::UsageBinding => OwnedFailure::Admission,
            Self::Cancelled => OwnedFailure::Cancelled,
            Self::StartUnknown => OwnedFailure::StartUnknown,
            Self::SpawnFailed => OwnedFailure::SpawnFailed,
            Self::LostAuthority => OwnedFailure::LostAuthority,
            Self::Protocol | Self::Worker => OwnedFailure::Protocol,
            Self::UnsupportedApproval | Self::ApprovalCapacity | Self::ApprovalCancelled => {
                OwnedFailure::UnsupportedApproval
            }
            Self::Deadline => OwnedFailure::Deadline,
            Self::CleanupUnknown => OwnedFailure::CleanupUnknown,
            Self::SettlementUnknown => OwnedFailure::SettlementUnknown,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    NotStarted,
    Completed,
    Failed,
    Interrupted,
    Unsupported,
    Unknown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Settlement {
    Pending,
    Completed,
    CanonicalReplyReady,
    Negative(OwnedObservation),
    Unknown,
}

/// Which store refusal produced `Failure::SettlementUnknown`. A fixed
/// discriminant, never the store's own error text: this carries no path,
/// payload or capability material, and cannot become execution authority,
/// retry, reply or lease input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettlementCause {
    /// The command never entered the writer queue (`Busy`).
    QueueBusy,
    /// The writer is gone (`Unavailable`).
    QueueUnavailable,
    /// The writer reply did not arrive inside its bound (`OutcomeUnknown`).
    ReplyTimedOut,
    /// The writer refused this scope, fence, route or deadline.
    RunnerAuthority,
    /// The writer refused its own current state.
    State,
    /// An unrelated dispatch or lease still holds the scope.
    Quarantined,
    /// Durable store capacity, schema or IO refusal.
    Storage,
    /// The acceptance write was refused or abandoned and the ordered reconcile
    /// read found no accepted row. Set directly by the approval acceptance pump,
    /// never derived from a store error: it names a *missing record*, not a
    /// refused call. Carries no path, payload or capability material, and cannot
    /// become execution authority, retry, reply or lease input.
    AcceptanceUnrecorded,
}
impl SettlementCause {
    pub(crate) fn of(error: &hagency_store::Error) -> Self {
        match error {
            hagency_store::Error::Busy => Self::QueueBusy,
            hagency_store::Error::Unavailable => Self::QueueUnavailable,
            hagency_store::Error::OutcomeUnknown => Self::ReplyTimedOut,
            hagency_store::Error::RunnerAuthority => Self::RunnerAuthority,
            hagency_store::Error::State => Self::State,
            hagency_store::Error::Quarantined => Self::Quarantined,
            _ => Self::Storage,
        }
    }
    /// Fixed trace label. Never store text: the discriminant is the whole datum.
    #[cfg(any(test, feature = "test-diagnostics"))]
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::QueueBusy => "queue-busy",
            Self::QueueUnavailable => "queue-unavailable",
            Self::ReplyTimedOut => "reply-timed-out",
            Self::RunnerAuthority => "runner-authority",
            Self::State => "state",
            Self::Quarantined => "quarantined",
            Self::Storage => "storage",
            Self::AcceptanceUnrecorded => "acceptance-unrecorded",
        }
    }
}
/// Record the cause and preserve the existing terminal verdict exactly.
///
/// The **first** cause on a path is the root cause and is never replaced. The
/// reconcile sets `AcceptanceUnrecorded` when its ordered read conclusively
/// found no accepted row; a later unrelated refusal (`observe_owned_completion`,
/// `publish_owned_completion`, `complete_owned_dispatch`) would otherwise
/// overwrite that verdict with a refusal-derived marker and hide the record
/// that actually went missing. So this only fills an unset marker.
fn settlement_failure(report: &mut Report, error: &hagency_store::Error) -> Failure {
    if report.settlement_cause.is_none() {
        report.settlement_cause = Some(SettlementCause::of(error));
    }
    Failure::SettlementUnknown
}

/// Fixed diagnostics from the original owned runtime, never execution authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeStage {
    Initialize,
    ThreadStart,
    TurnStart,
    Update,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeWriteObservation {
    pub accepted_bytes: usize,
    pub total_bytes: usize,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeObservation {
    pub stage: RuntimeStage,
    pub session_error: Option<session::Error>,
    pub transport_cause: Option<hagency_runtime::codex::transport::Error>,
    pub pending_requests: Option<usize>,
    pub pending_server_requests: Option<usize>,
    pub write: Option<RuntimeWriteObservation>,
}
impl RuntimeObservation {
    pub(crate) fn capture(stage: RuntimeStage, runner: &OwnedSession) -> Self {
        let termination = runner.transport_termination();
        Self {
            stage,
            session_error: match runner.protocol_outcome() {
                Some(Outcome::Unknown { reason }) => Some(*reason),
                _ => None,
            },
            transport_cause: termination.map(|t| t.cause),
            pending_requests: termination.map(|t| t.pending_requests),
            pending_server_requests: termination.map(|t| t.pending_server_requests),
            write: termination
                .and_then(|t| t.unconfirmed_write.as_ref())
                .map(|w| RuntimeWriteObservation {
                    accepted_bytes: w.accepted_bytes,
                    total_bytes: w.total_bytes,
                }),
        }
    }
}

/// Private host result: no Serialize/Debug or automatic console/Matrix projection.
/// An unresolved owner remains retained here; retrying stop never clears a lease.
pub struct Report {
    pub protocol: Protocol,
    pub cleanup: Cleanup,
    pub canonical_status: Option<TaskState>,
    pub settlement: Settlement,
    /// Diagnostic only: which store refusal was observed first. Set once,
    /// never used for authority, retry, reply or lease decisions.
    pub settlement_cause: Option<SettlementCause>,
    pub failure: Option<Failure>,
    runtime_observation: Option<RuntimeObservation>,
    runtime_stage: RuntimeStage,
    pub text: Option<String>,
    owner: Option<OwnedSession>,
    approvals: Option<crate::approval::ApprovalRun>,
    live: Option<crate::approval::Reservation>,
    reconciliation: Option<(DomainStore, RunnerCapability, OwnedFailure)>,
    usage: Option<UsageRun>,
    // After owner in field order: actual cleanup drops before retained roots.
    workspace: Option<Arc<Binding>>,
    account: Option<hagency_store::ManagedLaunch>,
    handoff: Handoff,
    registration: Option<Gate>,
}
impl Report {
    fn new(handoff: Handoff) -> Self {
        Self {
            protocol: Protocol::NotStarted,
            cleanup: Cleanup::Pending,
            canonical_status: None,
            settlement: Settlement::Pending,
            settlement_cause: None,
            failure: None,
            runtime_observation: None,
            runtime_stage: RuntimeStage::Initialize,
            text: None,
            owner: None,
            approvals: None,
            live: None,
            reconciliation: None,
            usage: None,
            workspace: None,
            account: None,
            handoff,
            registration: None,
        }
    }
    /// Immutable original observation survives retry_stop discarding a stopped
    /// owner. No descriptor, process ID, payload or private stderr is exposed.
    pub fn runtime_observation(&self) -> Option<&RuntimeObservation> {
        self.runtime_observation.as_ref()
    }
    #[cfg(test)]
    pub(crate) fn approval_custody(&self) -> (usize, usize, usize, usize) {
        self.approvals
            .as_ref()
            .map_or((0, 0, 0, 0), crate::approval::ApprovalRun::custody)
    }
    pub fn usage_status(&self) -> UsageStatus {
        self.usage
            .as_ref()
            .map_or_else(UsageStatus::default, UsageRun::status)
    }
    /// Explicit retry of one retained historical observation. Never restarts
    /// capture, execution, cleanup, canonical completion or message delivery.
    pub async fn retry_usage(&mut self) -> Result<UsageStatus, UsageFailure> {
        match &mut self.usage {
            Some(usage) => usage.record_pending().await,
            None => Ok(UsageStatus::default()),
        }
    }
    /// A bounded negative-only retry after a lost database response. Cancellation
    /// leaves the same retained receipt/capability for a subsequent explicit retry.
    /// This never retries successful settlement or clears a dirty lease.
    pub async fn retry_reconcile(&mut self) -> Settlement {
        self.retry_stop();
        if let Some((domain, cap, failure)) = &self.reconciliation
            && let Ok(value) = domain.observe_owned_failure(cap.clone(), *failure).await
        {
            self.settlement = Settlement::Negative(value);
            self.reconciliation = None;
        }
        self.settlement
    }

    /// Local physical custody only, never task/grant/lease settlement authority.
    /// Pending without an owner is the existing pre-child result. Unknown spawn
    /// or incomplete stop observations remain retained even without a returned owner.
    pub fn retains_process_custody(&self) -> bool {
        self.owner.is_some()
            || match self.cleanup {
                Cleanup::Pending => false,
                Cleanup::Observed(_) => !stopped(self.cleanup),
                Cleanup::Unknown { .. } => true,
            }
    }

    pub fn retry_stop(&mut self) -> Cleanup {
        if let Some(owner) = &mut self.owner {
            self.cleanup = owner.stop();
        } else if self.cleanup == Cleanup::Pending
            && let Some(live) = &mut self.live
        {
            live.release();
        }
        if stopped(self.cleanup) {
            if let Some(live) = &mut self.live {
                live.release();
            }
            if let Some(approvals) = &mut self.approvals {
                approvals.stopped();
            }
            self.owner.take();
        }
        self.cleanup
    }
}
fn stopped(cleanup: Cleanup) -> bool {
    matches!(cleanup, Cleanup::Observed(report) if report.scope.whole_tree_stopped && report.scope.leader_exited && report.scope.signals_accepted)
}

/// One host operation, one retained OS worker, one result slot. No scheduler or
/// detached cleanup. Dropping a waiting future requests cancellation; the worker
/// still stops and reconciles. Dropping this handle joins it synchronously, with
/// the conservative 60 s combined wait allowance documented by `Limits`. It must
/// not be dropped on a latency-sensitive HTTP/UI worker. There is no forced
/// thread termination or claim that an unknown child/guardian outcome is clean.
pub struct Operation {
    cancel: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    result: oneshot::Receiver<Box<Report>>,
    workspace: Handoff,
    registration: RegistrationSlot,
    approvals: Option<crate::ApprovalRequests>,
}
impl Operation {
    pub fn start(
        domain: DomainStore,
        capability: RunnerCapability,
        host: Host,
        limits: Limits,
    ) -> Result<Self, Failure> {
        Self::start_mode(domain, capability, host, limits, false)
    }
    /// Require the host to register the exact Started workspace before any child.
    pub fn start_requiring_workspace(
        domain: DomainStore,
        capability: RunnerCapability,
        host: Host,
        limits: Limits,
    ) -> Result<Self, Failure> {
        Self::start_mode(domain, capability, host, limits, true)
    }
    fn start_mode(
        domain: DomainStore,
        capability: RunnerCapability,
        host: Host,
        limits: Limits,
        required: bool,
    ) -> Result<Self, Failure> {
        if !limits.validate() {
            return Err(Failure::Admission);
        }
        let until = Instant::now() + Duration::from_millis(limits.operation_ms);
        let expires_at = crate::approval::state::wall_now()?
            .checked_add(limits.operation_ms)
            .ok_or(Failure::Deadline)?;
        let (live, approval_run, approval_requests) = if let Some(policy) = &host.approvals {
            if !policy.fits(limits) {
                return Err(Failure::Admission);
            }
            let live = policy.reserve_live()?;
            let (send, receive) = crate::approval::notices();
            (
                Some(live),
                Some(crate::approval::ApprovalRun::new(policy.clone(), send)),
                Some(receive),
            )
        } else {
            (None, None, None)
        };
        // Runtime construction has no child/domain effect and fails synchronously.
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| Failure::Worker)?;
        let cancel = Arc::new(AtomicBool::new(false));
        let signal = cancel.clone();
        let (reply, result) = oneshot::channel();
        let workspace = Arc::new(Mutex::new(None));
        let handoff = workspace.clone();
        let (gate, registration) = Gate::new();
        let worker = std::thread::Builder::new()
            .name("hagency-owned-dispatch".into())
            .spawn(move || {
                let mut report = Box::new(Report::new(handoff));
                report.registration = required.then_some(gate);
                report.live = live;
                report.approvals = approval_run;
                #[cfg(test)]
                if let Some(run) = &mut report.approvals {
                    run.set_fault(host.approval_fault, host.approval_gate.clone());
                }
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    runtime.block_on(execute(
                        &domain,
                        &capability,
                        host,
                        limits,
                        &signal,
                        Deadline { until, expires_at },
                        &mut report,
                    ))
                }))
                .unwrap_or(Err(Failure::Worker));
                if let Some(usage) = &mut report.usage {
                    usage.close();
                }
                if let Err(failure) = outcome {
                    // execute retains any returned owner; stop before negative domain
                    // observation too. This can never authorize lease release.
                    if report.runtime_observation.is_none()
                        && let Some(owner) = &report.owner
                    {
                        report.runtime_observation =
                            Some(RuntimeObservation::capture(report.runtime_stage, owner));
                    }
                    report.retry_stop();
                    report.failure = Some(failure);
                    report.reconciliation =
                        Some((domain.clone(), capability.clone(), failure.observation()));
                    report.settlement = match runtime
                        .block_on(domain.observe_owned_failure(capability, failure.observation()))
                    {
                        Ok(value) => {
                            report.reconciliation = None;
                            Settlement::Negative(value)
                        }
                        Err(_) => Settlement::Unknown,
                    };
                }
                if let Some(approvals) = &mut report.approvals {
                    approvals.finish_notices();
                }
                // If the host dropped its handle, sending returns ownership and its
                // Drop still runs here before this retained worker exits.
                let _ = reply.send(report);
            })
            .map_err(|_| Failure::Worker)?;
        Ok(Self {
            cancel,
            worker: Some(worker),
            result,
            workspace,
            registration,
            approvals: approval_requests,
        })
    }
    /// Nonblocking, one-shot handoff. None means not ready, already taken, or
    /// unavailable. A late value remains sealed but refuses retired access.
    pub fn take_workspace_binding(&mut self) -> Option<StartedWorkspace> {
        self.workspace.try_lock().ok()?.take()
    }
    /// One bounded host handoff; unavailable in the ordinary start mode.
    pub fn take_workspace_registration(&mut self) -> Option<WorkspaceRegistration> {
        self.registration.try_lock().ok()?.take()
    }
    pub fn take_approval_requests(&mut self) -> Option<crate::ApprovalRequests> {
        self.approvals.take()
    }
    pub fn is_finished(&self) -> bool {
        self.worker
            .as_ref()
            .is_none_or(std::thread::JoinHandle::is_finished)
    }
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Release);
    }
    pub async fn wait(&mut self) -> Result<Report, Failure> {
        self.wait_boxed().await.map(|report| *report)
    }
    /// Keep the single retained result indirect across nested host futures.
    /// This changes storage location only; cancellation and ownership are identical.
    pub async fn wait_boxed(&mut self) -> Result<Box<Report>, Failure> {
        let mut guard = WaitGuard {
            cancel: &self.cancel,
            done: false,
        };
        let result = (&mut self.result).await.map_err(|_| Failure::Worker);
        guard.done = true;
        if let Some(worker) = self.worker.take() {
            worker.join().map_err(|_| Failure::Worker)?;
        }
        result
    }
}
impl Drop for Operation {
    fn drop(&mut self) {
        self.cancel();
        self.result.close(); // Failed send drops any retained owner on the worker.
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
struct WaitGuard<'a> {
    cancel: &'a AtomicBool,
    done: bool,
}
impl Drop for WaitGuard<'_> {
    fn drop(&mut self) {
        if !self.done {
            self.cancel.store(true, Ordering::Release);
        }
    }
}

fn checkpoint(cancel: &AtomicBool, until: Instant) -> Result<(), Failure> {
    if cancel.load(Ordering::Acquire) {
        Err(Failure::Cancelled)
    } else if Instant::now() >= until {
        Err(Failure::Deadline)
    } else {
        Ok(())
    }
}
pub(crate) async fn bounded<F: Future>(
    future: F,
    cancel: &AtomicBool,
    until: Instant,
) -> Result<F::Output, Failure> {
    tokio::pin!(future);
    let mut tick = interval(Duration::from_millis(20));
    tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        checkpoint(cancel, until)?;
        tokio::select! {
            biased;
            _ = tokio::time::sleep_until(until) => return Err(Failure::Deadline),
            _ = tick.tick() => {},
            result = &mut future => return Ok(result),
        }
    }
}
async fn watched<F: Future<Output = Result<T, session::Error>>, T>(
    future: F,
    domain: &DomainStore,
    cap: &RunnerCapability,
    expected: &str,
    cancel: &AtomicBool,
    until: Instant,
    status: &mut Option<TaskState>,
) -> Result<T, Failure> {
    tokio::pin!(future); // Held across checks; a cancelled future is never repolled.
    let mut tick = interval(Duration::from_millis(100));
    tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        checkpoint(cancel, until)?;
        tokio::select! {
            biased;
            _ = tokio::time::sleep_until(until) => return Err(Failure::Deadline),
            _ = tick.tick() => {
                let current = bounded(domain.renew_owned_dispatch(cap.clone(), expected.into(), 5_000), cancel, until).await?
                    .map_err(|_| Failure::LostAuthority)?;
                *status = Some(current.status);
            },
            result = &mut future => return result.map_err(|error| if error == session::Error::UnsupportedRequest { Failure::UnsupportedApproval } else { Failure::Protocol }),
        }
    }
}
pub(crate) struct Deadline {
    pub until: Instant,
    pub expires_at: u64,
}
async fn execute(
    domain: &DomainStore,
    cap: &RunnerCapability,
    host: Host,
    limits: Limits,
    cancel: &Arc<AtomicBool>,
    deadline: Deadline,
    report: &mut Report,
) -> Result<(), Failure> {
    let Deadline { until, expires_at } = deadline;
    let scope = bounded(domain.owned_dispatch_scope(cap.clone()), cancel, until)
        .await?
        .map_err(|_| Failure::Admission)?;
    let expected = scope.fingerprint().to_owned();
    let crate::host::Prepared {
        launch,
        settings,
        io_limits,
        input,
        root,
        account,
    } = host.prepare(&scope, cap, limits)?;
    report.account = account;
    checkpoint(cancel, until)?;
    let start_reply = bounded(
        domain.start_owned_dispatch(cap.clone(), expected.clone()),
        cancel,
        until,
    )
    .await?;
    // Delivery-loss test double only. DomainStore has actually committed the
    // start; production contains no way to manufacture or recover a lost reply.
    #[cfg(test)]
    let start_reply = if host.discard_start_reply && start_reply.is_ok() {
        Err(hagency_store::Error::OutcomeUnknown)
    } else {
        start_reply
    };
    let started = start_reply.map_err(|error| {
        if matches!(
            error,
            hagency_store::Error::OutcomeUnknown | hagency_store::Error::Unavailable
        ) {
            Failure::StartUnknown
        } else {
            Failure::Admission
        }
    })?;
    let approval_workspace = if host.approvals.is_some() {
        Some(root.approval_path()?)
    } else {
        None
    };
    let workspace = Binding::start(root, domain.clone(), cap, &started, cancel.clone())?;
    let _retire_workspace = workspace.retirement(); // all returns and unwinds
    report.workspace = Some(workspace.clone()); // before any child can exist
    let required = report.registration.is_some();
    if let Some(gate) = report.registration.take() {
        let acknowledged = gate.publish(workspace.handoff())?;
        bounded(acknowledged, cancel, until)
            .await?
            .map_err(|_| Failure::Admission)?;
    } else {
        *report.handoff.lock().map_err(|_| Failure::Worker)? = Some(workspace.handoff());
    }
    #[cfg(test)]
    if host.panic_after_workspace {
        panic!("offline post-Started workspace unwind");
    }
    report.canonical_status = Some(started.task().status);
    checkpoint(cancel, until)?;
    let binding = bounded(
        UsageRun::bind(domain.clone(), cap.clone(), started.clone()),
        cancel,
        until,
    )
    .await?;
    // Test build only: discard an actual acknowledged source binding, never
    // manufacture a successful write or reopen execution after receipt loss.
    #[cfg(test)]
    let binding = if host.discard_usage_binding_reply && binding.is_ok() {
        Err(hagency_store::Error::OutcomeUnknown)
    } else {
        binding
    };
    report.usage = Some(binding.map_err(|_| Failure::UsageBinding)?);
    checkpoint(cancel, until)?;
    if required {
        bounded(
            domain.check_owned_dispatch(cap.clone(), expected.clone()),
            cancel,
            until,
        )
        .await?
        .map_err(|_| Failure::LostAuthority)?;
        checkpoint(cancel, until)?;
    }
    workspace.check_root().map_err(|_| Failure::Admission)?;
    if let Some(account) = &report.account {
        account.check().map_err(|_| Failure::LostAuthority)?;
    }
    checkpoint(cancel, until)?;
    let approval_may_write = !settings.is_read_only();
    if let Some(live) = &mut report.live {
        live.possible();
    }
    report.owner = Some(
        OwnedSession::spawn(
            &host.guardian,
            &launch,
            settings,
            io_limits,
            limits.response_ms,
        )
        .map_err(|error| {
            if let hagency_runtime::owned::StartError::Uncertain { cleanup, .. } = error {
                report.cleanup = cleanup;
                if stopped(cleanup)
                    && let Some(live) = &mut report.live
                {
                    live.release();
                }
            } else if let Some(live) = &mut report.live {
                live.release();
            }
            Failure::SpawnFailed
        })?,
    );
    #[cfg(test)]
    if host.approval_fault == Some(crate::approval::Fault::SpawnPanic) {
        panic!("actual owned spawn unwind");
    }
    // The actual child owner is retained before initialize or any startup await.
    let runner = report.owner.as_mut().ok_or(Failure::SpawnFailed)?;
    let runtime_stage = &mut report.runtime_stage;
    let drive = async {
        watched(
            runner.initialize(),
            domain,
            cap,
            &expected,
            cancel,
            until,
            &mut report.canonical_status,
        )
        .await?;
        *runtime_stage = RuntimeStage::ThreadStart;
        let thread_id = watched(
            runner.start_thread(),
            domain,
            cap,
            &expected,
            cancel,
            until,
            &mut report.canonical_status,
        )
        .await?;
        *runtime_stage = RuntimeStage::TurnStart;
        let turn_id = watched(
            runner.start_turn(input),
            domain,
            cap,
            &expected,
            cancel,
            until,
            &mut report.canonical_status,
        )
        .await?;
        let usage = report.usage.as_mut().ok_or(Failure::UsageBinding)?;
        usage.attach(runner);
        if let Some(approvals) = &mut report.approvals {
            let connection = hagency_core::canonical::digest(&serde_json::json!([
                cap,
                runner.id(),
                thread_id,
                turn_id
            ]))
            .map_err(|_| Failure::Admission)?;
            let context = hagency_core::approvals::HostApprovalContext {
                id: format!("owned_{connection}"),
                connection_id: connection,
                thread_id,
                turn_id,
                workspace_resource: scope
                    .input()
                    .resources
                    .first()
                    .ok_or(Failure::Admission)?
                    .id
                    .clone(),
                workspace: approval_workspace.ok_or(Failure::Admission)?,
                windows_paths: cfg!(windows),
                environment_id: None,
                may_write: approval_may_write,
                yolo: false,
            };
            approvals
                .bind(
                    domain,
                    cap,
                    &expected,
                    context,
                    Deadline { until, expires_at },
                    runner,
                )
                .await?;
            *runtime_stage = RuntimeStage::Update;
            return approvals
                .drive(
                    crate::approval::Drive {
                        domain,
                        cap,
                        cancel,
                        until,
                        status: &mut report.canonical_status,
                        usage,
                        observation: &mut report.runtime_observation,
                        settlement_cause: &mut report.settlement_cause,
                    },
                    runner,
                )
                .await;
        }
        loop {
            *runtime_stage = RuntimeStage::Update;
            let (update, observation) = watched(
                runner.next_observed_update(),
                domain,
                cap,
                &expected,
                cancel,
                until,
                &mut report.canonical_status,
            )
            .await?;
            if usage.observe(&observation) {
                // Storage refusal closes capture only. Cancellation/deadline
                // still reaches the existing retained process cleanup path.
                let _ = bounded(usage.record_pending(), cancel, until).await?;
            }
            match update {
                Update::TurnEnded => break,
                Update::Approval(_) | Update::ApprovalResolved { .. } => {
                    return Err(Failure::UnsupportedApproval);
                }
                _ => {}
            }
        }
        Ok(())
    }
    .await;
    // Capture before coordinator stop/removal. The runtime's own failure guard
    // may already have stopped it; its first transport cause remains retained.
    // Observation cannot alter the original drive or cleanup result.
    report
        .runtime_observation
        .get_or_insert_with(|| RuntimeObservation::capture(*runtime_stage, runner));
    report.protocol = match runner.protocol_outcome() {
        Some(Outcome::Completed { text }) => {
            report.text = Some(text.clone());
            Protocol::Completed
        }
        Some(Outcome::Failed) => Protocol::Failed,
        Some(Outcome::Interrupted) => Protocol::Interrupted,
        Some(Outcome::UnsupportedRequest) => Protocol::Unsupported,
        _ => Protocol::Unknown,
    };
    report.cleanup = runner.stop();
    if stopped(report.cleanup) {
        if let Some(live) = &mut report.live {
            live.release();
        }
        if let Some(approvals) = &mut report.approvals {
            approvals.stopped();
        }
    }
    if host.task_helper_enabled() {
        // Observation only, after actual owner stop and before negative fencing.
        // This adds one bounded (2 s) fresh-clock writer read. Done changes the
        // epoch and still fails the exact renewal/settlement fingerprint. Never
        // use this status as execution, release, retry or reply authority.
        report.canonical_status = domain
            .runner_command(
                cap.clone(),
                RunnerCommand::Task {
                    id: scope.task().id.clone(),
                },
            )
            .await
            .ok()
            .and_then(|value| serde_json::from_value::<Task>(value).ok())
            .filter(|task| task.id == scope.task().id && task.session_id == scope.task().session_id)
            .map(|task| task.status);
    }
    checkpoint(cancel, until)?;
    // A matching explicit Done+body is completion custody, not a renewed task
    // epoch or permission to continue this process. The same runner was stopped
    // above. Scope is the opaque successful Start response, never admission data.
    if !matches!(
        drive,
        Err(Failure::Cancelled | Failure::Deadline | Failure::UnsupportedApproval)
    ) && let Some(reference) = domain
        .observe_owned_completion(cap.clone(), started.clone())
        .await
        .map_err(|error| settlement_failure(report, &error))?
    {
        report.canonical_status = Some(TaskState::Done);
        checkpoint(cancel, until)?;
        if !stopped(report.cleanup) {
            return Err(Failure::CleanupUnknown);
        }
        // Keep the receipt future alive. The writer checks this original signal
        // and monotonic deadline after queue/lock before admitting final content;
        // cancellation after that eligibility decision cannot undo its commit.
        domain
            .publish_owned_completion(
                cap.clone(),
                started,
                reference,
                cancel.clone(),
                until.into_std(),
            )
            .await
            .map_err(|error| match checkpoint(cancel, until).err() {
                Some(refused) => refused,
                None => settlement_failure(report, &error),
            })?;
        report.settlement = Settlement::CanonicalReplyReady;
        report.text = None; // Stored explicit content is the sole final body.
        report.owner.take();
        return Ok(());
    }
    drive?;
    checkpoint(cancel, until)?;
    if report.protocol != Protocol::Completed {
        return Err(Failure::Protocol);
    }
    if !stopped(report.cleanup) {
        return Err(Failure::CleanupUnknown);
    }
    let text = report.text.as_deref().ok_or(Failure::Protocol)?;
    // From this checkpoint the bounded writer commit is deliberately not
    // cancellation-raced. A later caller cancellation cannot undo a commit.
    let task = domain
        .complete_owned_dispatch(
            cap.clone(),
            expected,
            serde_json::json!({"upstream_text":text}),
        )
        .await
        .map_err(|error| settlement_failure(report, &error))?;
    report.canonical_status = Some(task.status);
    report.settlement = Settlement::Completed;
    report.owner.take();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Failure, Report, SettlementCause, settlement_failure};
    use hagency_store::Error;

    /// Every store refusal that can produce `Failure::SettlementUnknown` maps
    /// to its own marker, and anything else collapses to `Storage`. The
    /// mapping is a fixed diagnostic discriminant: it never carries the
    /// store's error text and never feeds authority, retry, reply or leases.
    #[test]
    fn native_settlement_cause_markers_are_distinct() {
        let mapped = [
            (Error::Busy, SettlementCause::QueueBusy),
            (Error::Unavailable, SettlementCause::QueueUnavailable),
            (Error::OutcomeUnknown, SettlementCause::ReplyTimedOut),
            (Error::RunnerAuthority, SettlementCause::RunnerAuthority),
            (Error::State, SettlementCause::State),
            (Error::Quarantined, SettlementCause::Quarantined),
        ];
        for (error, expected) in mapped {
            assert_eq!(SettlementCause::of(&error), expected, "{error:?}");
        }
        // Every other refusal — authority, IO, schema, capacity — reports as
        // durable storage, never as one of the queue or reply markers.
        let fallback = [
            Error::LocalAuthority,
            Error::Conflict,
            Error::NotFound,
            Error::Unqualified,
            Error::InsufficientCapacity,
            Error::Locked,
            Error::Schema,
            Error::Capacity,
            Error::Io(std::io::Error::other("fixture")),
            Error::Json(serde_json::from_str::<serde_json::Value>("{").unwrap_err()),
        ];
        for error in fallback {
            assert_eq!(
                SettlementCause::of(&error),
                SettlementCause::Storage,
                "{error:?}"
            );
        }
        // All eight markers are pairwise distinct discriminants. The eighth is
        // not derived from a store refusal (see `AcceptanceUnrecorded`), but it
        // must still be distinct from every refusal-derived marker so a trace
        // label can never be mistaken for a queue or reply attribution.
        let all = [
            SettlementCause::QueueBusy,
            SettlementCause::QueueUnavailable,
            SettlementCause::ReplyTimedOut,
            SettlementCause::RunnerAuthority,
            SettlementCause::State,
            SettlementCause::Quarantined,
            SettlementCause::Storage,
            SettlementCause::AcceptanceUnrecorded,
        ];
        for (i, left) in all.iter().enumerate() {
            for right in &all[i + 1..] {
                assert_ne!(left, right);
            }
        }
    }

    /// The first cause on a path is the root cause. `settlement_failure` fills an
    /// unset marker and never replaces one, so a later unrelated refusal cannot
    /// overwrite `AcceptanceUnrecorded` — the verdict that names the record that
    /// actually went missing.
    #[test]
    fn native_settlement_cause_keeps_the_first_marker() {
        fn blank() -> Report {
            Report::new(std::sync::Arc::new(std::sync::Mutex::new(None)))
        }
        let mut report = blank();
        assert_eq!(report.settlement_cause, None);
        // The reconcile's conclusive absence is the root cause.
        report.settlement_cause = Some(SettlementCause::AcceptanceUnrecorded);
        // A later refusal-derived marker must not replace it.
        assert_eq!(
            settlement_failure(&mut report, &Error::Busy),
            Failure::SettlementUnknown
        );
        assert_eq!(
            report.settlement_cause,
            Some(SettlementCause::AcceptanceUnrecorded)
        );
        assert_eq!(
            settlement_failure(&mut report, &Error::OutcomeUnknown),
            Failure::SettlementUnknown
        );
        assert_eq!(
            report.settlement_cause,
            Some(SettlementCause::AcceptanceUnrecorded)
        );
        // An unset marker is still filled, so the first writer wins.
        let mut fresh = blank();
        settlement_failure(&mut fresh, &Error::OutcomeUnknown);
        assert_eq!(fresh.settlement_cause, Some(SettlementCause::ReplyTimedOut));
        assert_eq!(
            settlement_failure(&mut fresh, &Error::Busy),
            Failure::SettlementUnknown
        );
        assert_eq!(
            fresh.settlement_cause,
            Some(SettlementCause::ReplyTimedOut),
            "the first refusal is the root cause"
        );
    }
}
