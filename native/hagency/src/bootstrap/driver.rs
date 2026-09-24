#[path = "inbox.rs"]
mod inbox;
#[path = "notice.rs"]
mod notice;
use super::{
    DriverMode, Failure, Shared, StatusHandle, config::Prepared, workspace::WorkspaceAccess,
};
use hagency_execution::{ApprovalRequests, Operation, Report, SharedHost};
use hagency_matrix::{CancellationToken, Collector, HostIntakePlan};
use hagency_runtime::owned::Cleanup;
use hagency_store::{
    AttemptClock, AttemptEvent, AttemptPhase, DomainStore, FenceReason, OwnedClaimProfile,
};
use std::{
    sync::mpsc::{self, SyncSender, TrySendError},
    thread::JoinHandle,
    time::Duration,
};
use tokio::sync::oneshot;

enum Command {
    Close(oneshot::Sender<Result<(), Failure>>),
}
enum RuntimeOwner {
    Ordinary(SharedHost),
    Factory(Box<hagency_matrix::ProvisionedAgent>),
}
struct Configuration {
    owner: RuntimeOwner,
    profile: OwnedClaimProfile,
    limits: hagency_execution::Limits,
    max_live: u32,
    enrollment: bool,
    receive_inbox: Option<hagency_core::received_files::ReceiveInboxPlan>,
    intake_sessions: Vec<String>,
    agent_inboxes: Vec<hagency_core::agent_inbox::AgentInboxPlan>,
    #[cfg(test)]
    discard_claim_reply: bool,
}
pub(super) struct Driver {
    cancel: CancellationToken,
    control: SyncSender<Command>,
    thread: Option<JoinHandle<()>>,
    pending_close: Option<oneshot::Receiver<Result<(), Failure>>>,
    close_unknown: bool,
}
impl Driver {
    pub fn start(
        prepared: Prepared,
        shared: Shared,
        files: Option<crate::file_service::FileHandle>,
        status: StatusHandle,
        // PC-C0 (plan v4 Q1): the ONE buildable handoff — the driver has no
        // usable `&mut Operation` window after `Box::pin(wait_boxed())`, so
        // it takes the single-consumer value once, immediately after the
        // operation exists, and forwards it; the pump owns the receiver.
        notices: Option<tokio::sync::mpsc::Sender<ApprovalRequests>>,
        mode: DriverMode,
    ) -> Result<Self, Failure> {
        start_owner(
            Configuration {
                owner: RuntimeOwner::Ordinary(prepared.host.into_shared()),
                profile: prepared.claim,
                limits: prepared.limits,
                max_live: prepared.max_live,
                enrollment: prepared.enrollment,
                receive_inbox: prepared.receive_inbox,
                intake_sessions: prepared.intake_sessions,
                agent_inboxes: prepared.agent_inboxes,
                #[cfg(test)]
                discard_claim_reply: prepared.discard_claim_reply,
            },
            shared,
            files,
            status,
            notices,
            mode,
        )
    }
    pub(super) async fn start_agent(
        agent: hagency_matrix::ProvisionedAgent,
        shared: Shared,
        files: Option<crate::file_service::FileHandle>,
        status: StatusHandle,
        notices: tokio::sync::mpsc::Sender<ApprovalRequests>,
        limits: hagency_execution::Limits,
    ) -> Result<Self, Failure> {
        let profile = agent
            .claim_profile()
            .await
            .map_err(|_| Failure::OutcomeUnknown)?;
        let inbox = hagency_core::agent_inbox::AgentInboxPlan {
            session_id: agent.session().id.clone(),
            workspace_id: agent.workspace_id().into(),
        };
        start_owner(
            Configuration {
                owner: RuntimeOwner::Factory(Box::new(agent)),
                profile,
                limits,
                max_live: 8,
                enrollment: false,
                receive_inbox: None,
                intake_sessions: vec![inbox.session_id.clone()],
                agent_inboxes: vec![inbox],
                #[cfg(test)]
                discard_claim_reply: false,
            },
            shared,
            files,
            status,
            Some(notices),
            DriverMode::Continuous,
        )
    }
}
fn start_owner(
    mut prepared: Configuration,
    shared: Shared,
    files: Option<crate::file_service::FileHandle>,
    status: StatusHandle,
    notices: Option<tokio::sync::mpsc::Sender<ApprovalRequests>>,
    mode: DriverMode,
) -> Result<Driver, Failure> {
    let (control, commands) = mpsc::sync_channel(1);
    let cancel = CancellationToken::new();
    let signal = cancel.clone();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| Failure::Worker)?;
    let thread = std::thread::Builder::new()
        .name(match mode {
            DriverMode::Continuous => "hagency-agent-driver".into(),
            _ => "hagency-development-driver".into(),
        })
        .spawn(move || {
            let Shared {
                domain,
                collector,
                workspace,
            } = shared;
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let attempt = Attempt {
                    domain: &domain,
                    // Bootstrap already consumed the original account into
                    // both this Host and its exact claim profile.
                    owner: &mut prepared.owner,
                    profile: prepared.profile,
                    limits: prepared.limits,
                    max_live: prepared.max_live,
                    collector: &collector,
                    workspace: &workspace,
                    files: files.as_ref(),
                    enrollment: prepared.enrollment,
                    receive_inbox: prepared.receive_inbox,
                    intake_sessions: prepared.intake_sessions,
                    agent_inboxes: prepared.agent_inboxes,
                    cancel: &signal,
                    status: &status,
                    notices: notices.as_ref(),
                    #[cfg(test)]
                    discard_claim_reply: prepared.discard_claim_reply,
                    runner: if mode == DriverMode::Continuous {
                        "native_agent"
                    } else {
                        "native_development"
                    },
                };
                let future = async {
                    if mode == DriverMode::Continuous {
                        run_continuous(attempt).await
                    } else {
                        run(attempt)
                            .await
                            .map(|value| value.map(|completed| completed.report))
                    }
                };
                runtime.block_on(Box::pin(future))
            }));
            workspace.retire();
            let unowned_failure = matches!(&result, Err(_) | Ok(Err(Failure::Worker)));
            let mut report = match result {
                Ok(Ok(report)) => report,
                Ok(Err(error)) => {
                    status.fail(error);
                    None
                }
                Err(_) => {
                    status.fail(Failure::Worker);
                    None
                }
            };
            while let Ok(Command::Close(reply)) = commands.recv() {
                signal.cancel();
                workspace.retire();
                let outcome = if unowned_failure || report.as_mut().is_some_and(|r| !stopped(r)) {
                    Err(Failure::OutcomeUnknown)
                } else {
                    Ok(()) // Shared Collector closes only after the file owner too.
                };
                if outcome.is_ok() {
                    drop(report.take());
                    status.phase("closed");
                    let _ = reply.send(Ok(()));
                    return;
                }
                status.fail(Failure::OutcomeUnknown);
                let _ = reply.send(outcome); // original report stays in this owner
            }
            // Abrupt handle abandonment is not a successful close. Destruction
            // remains on the retained OS worker and can never release a DB lease.
            drop(report);
        })
        .map_err(|_| Failure::Worker)?;
    Ok(Driver {
        cancel,
        control,
        thread: Some(thread),
        pending_close: None,
        close_unknown: false,
    })
}
impl Driver {
    pub fn cancel(&self) {
        self.cancel.cancel();
    }
    /// Non-consuming close. Unknown keeps both the worker and pending receipt.
    pub async fn close(&mut self) -> Result<(), Failure> {
        self.cancel();
        if self.close_unknown {
            return Err(Failure::OutcomeUnknown);
        }
        if self.thread.is_none() {
            return Ok(());
        }
        if self.pending_close.is_none() {
            let (send, receive) = oneshot::channel();
            match self.control.try_send(Command::Close(send)) {
                Ok(()) => self.pending_close = Some(receive),
                Err(TrySendError::Full(_)) => return Err(Failure::OutcomeUnknown),
                Err(TrySendError::Disconnected(_)) => return Err(Failure::Worker),
            }
        }
        let pending = self.pending_close.as_mut().ok_or(Failure::Worker)?;
        let result = match tokio::time::timeout(Duration::from_secs(2), pending).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => {
                self.pending_close = None;
                self.close_unknown = true;
                return Err(Failure::OutcomeUnknown);
            }
            Err(_) => return Err(Failure::OutcomeUnknown),
        };
        self.pending_close = None;
        if result.is_err() {
            self.close_unknown = true;
        }
        result?;
        if let Some(worker) = self.thread.take() {
            // ACK precedes return by only fixed local operations. Never joins
            // an executing model/SDK owner from an HTTP request handler.
            if worker.join().is_err() {
                self.close_unknown = true;
                return Err(Failure::OutcomeUnknown);
            }
        }
        Ok(())
    }
}
impl Drop for Driver {
    fn drop(&mut self) {
        self.cancel();
    }
}
fn stopped(report: &mut Report) -> bool {
    report.retry_stop();
    !report.retains_process_custody()
}
struct Attempt<'a> {
    domain: &'a DomainStore,
    owner: &'a mut RuntimeOwner,
    profile: OwnedClaimProfile,
    limits: hagency_execution::Limits,
    max_live: u32,
    collector: &'a Collector,
    workspace: &'a WorkspaceAccess,
    files: Option<&'a crate::file_service::FileHandle>,
    enrollment: bool,
    receive_inbox: Option<hagency_core::received_files::ReceiveInboxPlan>,
    intake_sessions: Vec<String>,
    agent_inboxes: Vec<hagency_core::agent_inbox::AgentInboxPlan>,
    cancel: &'a CancellationToken,
    status: &'a StatusHandle,
    notices: Option<&'a tokio::sync::mpsc::Sender<ApprovalRequests>>,
    #[cfg(test)]
    discard_claim_reply: bool,
    runner: &'static str,
}
struct Completed {
    report: Box<Report>,
    capability: hagency_core::tasks::RunnerCapability,
}

async fn run_continuous(input: Attempt<'_>) -> Result<Option<Box<Report>>, Failure> {
    let mut first = true;
    let engagement = input.profile.engagement_id().to_owned();
    loop {
        if input.cancel.is_cancelled() {
            return Ok(None);
        }
        // A fenced agent claims nothing (the store's gate); say so and wait.
        if custody(&input, &engagement).await {
            tokio::select! {
                _ = input.cancel.cancelled() => return Ok(None),
                _ = tokio::time::sleep(Duration::from_secs(1)) => {}
            }
            continue;
        }
        let outcome = run(Attempt {
            domain: input.domain,
            owner: &mut *input.owner,
            profile: input.profile.clone(),
            limits: input.limits,
            max_live: input.max_live,
            collector: input.collector,
            workspace: input.workspace,
            files: input.files,
            enrollment: first && input.enrollment,
            receive_inbox: None,
            intake_sessions: input.intake_sessions.clone(),
            agent_inboxes: input.agent_inboxes.clone(),
            cancel: input.cancel,
            status: input.status,
            notices: input.notices,
            #[cfg(test)]
            discard_claim_reply: input.discard_claim_reply,
            runner: input.runner,
        })
        .await;
        first = false;
        let Some(mut completed) = (match outcome {
            Ok(value) => value,
            Err(Failure::Cancelled) if input.cancel.is_cancelled() => return Ok(None),
            // A refused handoff is that attempt's failure (ADR-182): it is
            // recorded, and the worker goes on after the retained product's
            // flat launch backoff.
            Err(Failure::Worker) => {
                input.status.phase("launch_refused");
                tokio::select! {
                    _ = input.cancel.cancelled() => return Ok(None),
                    _ = tokio::time::sleep(LAUNCH_RETRY) => {}
                }
                continue;
            }
            Err(error) => return Err(error),
        }) else {
            tokio::select! {
                _ = input.cancel.cancelled() => return Ok(None),
                _ = tokio::time::sleep(Duration::from_secs(1)) => {}
            }
            continue;
        };
        let settled = matches!(
            completed.report.settlement,
            hagency_execution::Settlement::Completed
                | hagency_execution::Settlement::CanonicalReplyReady
        );
        let physically_stopped = stopped(&mut completed.report);
        if completed.report.failure.is_some() || !settled {
            // The retained product tells the thread when a run's outcome is
            // unknown ("Result uncertain… will not be run again automatically")
            // and keeps the agent up; this worker used to go quiet, so the room
            // saw an agent that simply stopped answering. The store queues that
            // notice when it fences the dispatch; post it before waiting for the
            // operator or giving up. Best effort by construction: the attempt
            // has already failed, and a refused send changes nothing about it.
            if let RuntimeOwner::Factory(agent) = &*input.owner {
                let engagement = agent.session().engagement_id.clone();
                let _ = notice::deliver(
                    input.domain,
                    input.collector,
                    &engagement,
                    input.cancel,
                    None,
                )
                .await;
            }
        }
        // One fact, one blast radius (ADR-182). A failed attempt ended its
        // dispatch: the store fenced it and quarantined its session, the
        // notice went out above, and this worker goes on to its next claim —
        // the retained product's rule. Only a tree the guardian could not
        // prove gone reaches further: it fences the AGENT, durably, in the
        // store, and only then is the in-memory owner dropped (its guardian
        // reaped, the process-group backstop ending what the stop could
        // not prove ended). The store refusing the fence is the one case that
        // still retains the owner: fail closed, as before.
        if !physically_stopped {
            let reason = match completed.report.cleanup {
                Cleanup::Unknown { .. } => FenceReason::CleanupUnknown,
                _ => FenceReason::CleanupUnproven,
            };
            match input
                .domain
                .write_agent_fence(
                    engagement.clone(),
                    completed.capability.dispatch_id.clone(),
                    completed.capability.fence,
                    reason,
                    now_ms(),
                )
                .await
            {
                Ok(fence) => {
                    tracing::warn!(dispatch_id = %fence.dispatch_id, reason = reason.as_str(),
                        "agent fenced: cleanup not proven; owner released");
                    input.status.custody(0, Some(fence.dispatch_id));
                }
                Err(error) => {
                    tracing::error!(dispatch_id = %completed.capability.dispatch_id, error = ?error,
                        "agent fence not written; owner retained");
                    return Ok(Some(completed.report));
                }
            }
        }
        input.workspace.release(&completed.capability)?;
        drop(completed.report);
        input.status.phase("idle");
    }
}
/// The retained product's flat backoff after a launch that never started
/// (`RUNNER_LAUNCH_RETRY_MS`): the worker's own pause and the requeued
/// dispatch's `not_before`.
const LAUNCH_RETRY_MS: u64 = 5_000;
const LAUNCH_RETRY: Duration = Duration::from_millis(LAUNCH_RETRY_MS);
/// What the store holds against this agent (ADR-182): its unresolved
/// dispatches and any open fence. Read once per pass; the store's own gates
/// are what refuse the work, this only says so in the status.
async fn custody(input: &Attempt<'_>, engagement: &str) -> bool {
    let unresolved = input
        .domain
        .unresolved_dispatches_for_engagement(engagement.to_owned())
        .await
        .unwrap_or_default();
    let fence = input
        .domain
        .open_agent_fence(engagement.to_owned())
        .await
        .ok()
        .flatten();
    let fenced = fence.is_some();
    input
        .status
        .custody(unresolved, fence.map(|fence| fence.dispatch_id));
    fenced
}

/// Whether a factory agent's fresh inbox resolution has moved past `plan`.
/// Only the original factory re-resolves inboxes; an ordinary host's static
/// plan has no newer generation to move to, so its refusal stays a failure.
async fn superseded(
    owner: &RuntimeOwner,
    profile: &OwnedClaimProfile,
    plan: &hagency_core::agent_inbox::AgentInboxPlan,
) -> bool {
    let RuntimeOwner::Factory(agent) = owner else {
        return false;
    };
    match agent.inboxes(profile.clone()).await {
        Ok((_, current)) => !current
            .iter()
            .any(|inbox| inbox.session_id == plan.session_id),
        Err(_) => false,
    }
}

async fn run(input: Attempt<'_>) -> Result<Option<Completed>, Failure> {
    let Attempt {
        domain,
        owner,
        profile,
        limits,
        max_live,
        collector,
        workspace,
        files,
        enrollment,
        receive_inbox,
        intake_sessions,
        agent_inboxes,
        cancel,
        status,
        notices,
        #[cfg(test)]
        discard_claim_reply,
        runner,
    } = input;
    status.begin_attempt();
    let refresh = collector.collect(cancel).await;
    // Fresh collect initializes the SDK. Failed refresh may only recover an old
    // protected receipt; it can never turn historical success into readiness.
    // Resume itself is inspect/settle-only: even a revoked current token must
    // not hide an original protected acceptance or cause another Matrix write.
    let resumed = collector.resume_outgoing_custody(cancel).await;
    if refresh.is_ok() {
        // Reconcile every protected outgoing journal before scheduling new
        // work. Canonical agent replies use the same custody path as file and
        // enrollment traffic, including in otherwise message-only hosts.
        let result = resumed.map_err(|_| Failure::OutcomeUnknown)?;
        if result.state == hagency_matrix::OutgoingState::Uncertain {
            return Err(Failure::OutcomeUnknown);
        }
    }
    refresh.map_err(|e| {
        status.matrix_refusal(&e);
        tracing::warn!(error = ?e, "Matrix refresh refused");
        if matches!(e, hagency_matrix::Error::OutcomeUnknown) {
            Failure::OutcomeUnknown
        } else {
            Failure::Refresh
        }
    })?;
    if enrollment {
        status.phase("enrolling");
        collector
            .enroll_fresh_account(cancel)
            .await
            .map_err(|error| {
                status.matrix_refusal(&error);
                tracing::warn!(error = ?error, "Matrix crypto enrollment refused");
                if error == hagency_matrix::Error::OutcomeUnknown {
                    Failure::OutcomeUnknown
                } else {
                    Failure::Startup
                }
            })?;
    }
    if let Some(plan) = receive_inbox
        && !inbox::prepare(domain, collector, plan, cancel, status).await?
    {
        status.phase("no_work");
        return Ok(None);
    }
    let (profile, intake_sessions, agent_inboxes) = if let RuntimeOwner::Factory(agent) = &*owner {
        let (profile, inboxes) = agent.inboxes(profile).await.map_err(|error| {
            status.matrix_refusal(&error);
            tracing::warn!(error=?error,"factory inbox observation refused");
            Failure::Refresh
        })?;
        let mut sessions: Vec<String> = inboxes
            .iter()
            .map(|inbox| inbox.session_id.clone())
            .collect();
        // A delegated task's thread is this agent's to read too: the follow-ups
        // the owner posts in it are admitted through the delegated session, as
        // the retained product routes a thread message to the task bound to
        // that thread (task rust-delegated-thread-followup).
        sessions.extend(
            domain
                .intent_sessions(agent.session().engagement_id.clone())
                .await
                .map_err(|_| Failure::Refresh)?,
        );
        (profile, sessions, inboxes)
    } else {
        (profile, intake_sessions, agent_inboxes)
    };
    if !intake_sessions.is_empty() {
        status.phase("receiving");
        let plan = HostIntakePlan::new(intake_sessions).map_err(|_| Failure::Config)?;
        collector.intake(plan, cancel).await.map_err(|error| {
            status.matrix_refusal(&error);
            tracing::warn!(error = ?error, "Matrix inbox intake refused");
            if cancel.is_cancelled() {
                Failure::Cancelled
            } else if error == hagency_matrix::Error::OutcomeUnknown {
                Failure::OutcomeUnknown
            } else {
                Failure::Refresh
            }
        })?;
    }
    // ADR180 delegation is inline-factory only: an ordinary host owns no
    // engagement a notice could be scoped to. The assignee posts its own notice
    // before scheduling, because that delivery is what activates the intent.
    let delegated = if let RuntimeOwner::Factory(agent) = &*owner {
        let engagement = agent.session().engagement_id.clone();
        notice::deliver(domain, collector, &engagement, cancel, Some(status)).await?;
        Some((engagement, agent.workspace_id().to_owned()))
    } else {
        None
    };
    if !agent_inboxes.is_empty() {
        status.phase("scheduling");
        for plan in agent_inboxes {
            match domain.select_agent_inbox(plan.clone()).await {
                Ok(_) => {}
                // This poll's own intake can observe a membership change that
                // advances a shared project's generation (ADR153) and retires
                // the session this plan was resolved from a moment earlier.
                // When a fresh resolution no longer names that session the plan
                // was superseded, not refused: end the poll without work and let
                // the next one schedule the current generation (ADR178). A
                // session the fresh resolution still names stays a failure.
                Err(hagency_store::Error::RunnerAuthority)
                    if superseded(owner, &profile, &plan).await =>
                {
                    tracing::info!(session_id = %plan.session_id, "factory inbox plan superseded by a newer room generation");
                    status.phase("no_work");
                    return Ok(None);
                }
                Err(_) => return Err(Failure::OutcomeUnknown),
            }
        }
    }
    if let Some((engagement, workspace_id)) = &delegated {
        notice::schedule(domain, engagement, workspace_id).await?;
    }
    if let Some(files) = files {
        files.initialize().await.map_err(|e| {
            if e == crate::file_service::FileError::Unknown {
                Failure::OutcomeUnknown
            } else {
                Failure::Startup
            }
        })?;
    }
    if cancel.is_cancelled() {
        return Err(Failure::Cancelled);
    }
    // Reserve before claim/Started: a full handoff must neither lose the
    // original receiver nor delay a running operation's workspace ACK. Waiting
    // owns no task capability or workspace lease and remains cancellable.
    let notice_slot = if let Some(sender) = notices {
        status.phase("waiting_approval_capacity");
        Some(tokio::select! {
            biased;
            _=cancel.cancelled()=>return Err(Failure::Cancelled),
            slot=sender.reserve()=>slot.map_err(|_|Failure::OutcomeUnknown)?,
        })
    } else {
        None
    };
    status.phase("claiming");
    let capability_ms = limits.capability_ms().map_err(|_| Failure::Config)?;
    let capability = domain
        .claim_owned_dispatch_for_host(profile, runner.into(), 60_000, capability_ms, max_live)
        .await
        .map_err(|_| Failure::OutcomeUnknown)?;
    // Test-build only: discard an actual committed claim response. No host
    // profile, runtime input or production branch can request this fault.
    #[cfg(test)]
    if discard_claim_reply && capability.is_some() {
        drop(capability);
        return Err(Failure::OutcomeUnknown);
    }
    let Some(capability) = capability else {
        status.phase("no_work");
        return Ok(None);
    };
    // The attempt's first record (ADR-181): the claim, with the budgets it
    // was given. Best effort, like every phase record after it.
    record_phase(
        domain,
        &capability,
        AttemptPhase::Claimed,
        serde_json::json!({"runner": runner, "capability_ms": capability_ms, "max_live": max_live}),
    )
    .await;
    if cancel.is_cancelled() {
        domain
            .observe_owned_failure(capability, hagency_store::OwnedFailure::Cancelled)
            .await
            .map_err(|_| Failure::OutcomeUnknown)?;
        return Err(Failure::Cancelled);
    }
    let operation = match owner {
        RuntimeOwner::Ordinary(host) => Operation::start_requiring_workspace_shared(
            domain.clone(),
            capability.clone(),
            host.clone(),
            limits,
        ),
        RuntimeOwner::Factory(agent) => agent.dispatch(capability.clone(), limits).await,
    };
    let mut operation = match operation {
        Ok(operation) => operation,
        Err(error) => {
            status.handoff_refusal(&capability.dispatch_id, &error);
            // A refusal before any Operation exists is still this attempt's
            // failure record (ADR-181): the same fixed labels, uncollapsed.
            record_phase(
                domain,
                &capability,
                AttemptPhase::Failed,
                serde_json::json!({"handoff": super::owned_failure_label(&error),
                    "status": serde_json::to_value(status.get()).expect("fixed status serializes")}),
            )
            .await;
            tracing::warn!(dispatch_id = %capability.dispatch_id, failure = super::owned_failure_label(&error),
                "original dispatch handoff refused; attempt recorded, worker continues");
            // Nothing started, so the dispatch goes back to the queue with the
            // retained product's launch backoff (ADR-182 decision 2) instead
            // of holding its lease for the full minute. A refused requeue
            // leaves the lease to expire on its own; the attempt is recorded
            // either way.
            if let Err(error) = domain
                .fail_before_start(capability.clone(), now_ms(), LAUNCH_RETRY_MS)
                .await
            {
                tracing::warn!(dispatch_id = %capability.dispatch_id, error = ?error,
                    "refused handoff not requeued; its lease expires on its own");
            }
            return Err(Failure::Worker);
        }
    };
    // PC-C0 (plan v4 Q1): the one `&mut` window — immediately after the
    // operation exists, before `wait_boxed()` pins it for the whole run. The
    // single-consumer value is taken once and forwarded to the pump, which
    // owns the receiver on the service runtime; this driver never drains it.
    // The pre-claim reservation keeps this transfer synchronous even when
    // several agents are starting; no source is discarded on a full queue.
    if let Some(slot) = notice_slot
        && let Some(requests) = operation.take_approval_requests()
    {
        slot.send(requests);
    }
    status.phase("registering");
    loop {
        if cancel.is_cancelled() {
            operation.cancel();
        }
        if let Some(registration) = operation.take_workspace_registration() {
            let (binding, ack) = registration.into_parts();
            if let Err(rejected) = workspace.register(capability.clone(), binding).await {
                drop(rejected.capability);
                drop(rejected.binding);
                operation.cancel();
                drop(ack);
            } else if workspace.check(&capability).await.is_ok() && !cancel.is_cancelled() {
                status.registered();
                if ack.registered().is_err() {
                    operation.cancel();
                }
            } else {
                operation.cancel();
                drop(ack);
            }
            break;
        }
        if operation.is_finished() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    status.phase("running");
    if let Err(error) = domain
        .set_attempt_clock(
            capability.dispatch_id.clone(),
            capability.fence,
            AttemptClock::Started,
            now_ms(),
        )
        .await
    {
        tracing::warn!(dispatch_id = %capability.dispatch_id, error = ?error, "attempt start clock not recorded");
    }
    let mut wait = Box::pin(operation.wait_boxed());
    let report = tokio::select! {
            value = &mut wait => value.map_err(|_| Failure::Worker)?,
            _ = cancel.cancelled() => {
                // Dropping wait triggers the existing operation cancellation guard;
                // then await the SAME operation owner, never start it again.
                drop(wait);
                operation.cancel();
                operation.wait_boxed().await.map_err(|_| Failure::Worker)?
            }
    };
    finish_attempt(domain, capability, report, collector, cancel, status)
        .await
        .map(Some)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| u64::try_from(d.as_millis()).ok())
        .unwrap_or_default()
}
/// One phase record of the attempt (ADR-181): the same fixed labels go to
/// the service log; the store keeps it in its own savepoint; a refused
/// record changes nothing here.
async fn record_phase(
    domain: &DomainStore,
    capability: &hagency_core::tasks::RunnerCapability,
    phase: AttemptPhase,
    detail: serde_json::Value,
) {
    tracing::info!(dispatch_id = %capability.dispatch_id, fence = capability.fence, phase = phase.as_str(),
        detail = %detail, "owned attempt phase");
    let event = AttemptEvent {
        dispatch_id: capability.dispatch_id.clone(),
        fence: capability.fence,
        phase,
        detail,
    };
    if let Err(error) = domain.record_attempt_event(event, now_ms()).await {
        tracing::warn!(dispatch_id = %capability.dispatch_id, phase = phase.as_str(), error = ?error,
            "owned attempt phase not recorded");
    }
}

async fn finish_attempt(
    domain: &DomainStore,
    capability: hagency_core::tasks::RunnerCapability,
    report: Box<Report>,
    collector: &Collector,
    cancel: &CancellationToken,
    status: &StatusHandle,
) -> Result<Completed, Failure> {
    status.result(&capability.dispatch_id, &report);
    // The attempt's last records (ADR-181): the full status, uncollapsed, as
    // the `failed` or `settled` event; the clock; and the retained product's
    // `terminal_reason` shape, `<failure>:<exit identity>:<stderr tail>`.
    let projected = serde_json::to_value(status.get()).expect("fixed status serializes");
    let failure_label = report.failure.as_ref().map(super::owned_failure_label);
    record_phase(
        domain,
        &capability,
        if report.failure.is_some() {
            AttemptPhase::Failed
        } else {
            AttemptPhase::Settled
        },
        serde_json::json!({"status": projected, "exit_identity": report.exit_identity, "stderr_tail": report.stderr_tail,
            "guardian_stderr_tail": report.guardian_stderr_tail}),
    )
    .await;
    let reason = format!(
        "{}:{}:{}",
        failure_label.unwrap_or("completed"),
        report.exit_identity.as_deref().unwrap_or("none"),
        if report.failure.is_some() {
            report.stderr_tail.as_str()
        } else {
            ""
        }
    );
    for outcome in [
        domain
            .set_attempt_clock(
                capability.dispatch_id.clone(),
                capability.fence,
                AttemptClock::Settled,
                now_ms(),
            )
            .await,
        domain
            .set_attempt_terminal_reason(capability.dispatch_id.clone(), capability.fence, reason)
            .await,
    ] {
        if let Err(error) = outcome {
            tracing::warn!(dispatch_id = %capability.dispatch_id, error = ?error, "attempt record not written");
        }
    }
    if report.failure.is_some() {
        tracing::warn!(dispatch_id = %capability.dispatch_id, status = %projected,
            "original owned attempt failed; recorded");
    }
    // An operation result is not stopped-owner/workspace-inspection authority
    // for every pending stop (or for its own uncertain failure). The original
    // owned worker publishes successful completion; negative scopes retain
    // their leases until exact explicit recovery. G8 automatic stop settlement
    // remains owed, never inferred from a formatted Report or global queue.
    if report.settlement == hagency_execution::Settlement::CanonicalReplyReady {
        status.phase("delivering");
        let claim = domain
            .claim_final_reply_for_dispatch(capability.clone(), 60_000)
            .await
            .map_err(|_| Failure::OutcomeUnknown)?
            .ok_or(Failure::OutcomeUnknown)?;
        let delivered = collector
            .send_final(claim, cancel)
            .await
            .map_err(|_| Failure::OutcomeUnknown)?;
        if delivered.state != hagency_matrix::OutgoingState::Delivered {
            return Err(Failure::OutcomeUnknown);
        }
        status.phase("delivered");
    }
    Ok(Completed { report, capability })
}

#[cfg(test)]
use crate::file_service::test_common;
#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use hagency_core::{replies::*, tasks::*};
    use hagency_execution::{Host, Limits};
    use hagency_store::{OwnedClaimProfile, OwnedClaimRoom, private};
    use std::collections::{BTreeMap, BTreeSet};

    #[tokio::test]
    async fn native_fleet_approval_handoff_backpressure() {
        for finish in ["cancel", "release", "closed"] {
            let f = test_common::Fixture::new();
            let mut fake = test_common::Fake::start(true).await;
            let mut prepared = fixture(&f, &fake.endpoint).await;
            // After capacity becomes available, observe a real committed claim
            // but withhold its ACK so this fixture never launches a child.
            prepared.discard_claim_reply = true;
            let shared = Shared::new(prepared.matrix.take().unwrap(), f.store.clone()).unwrap();
            let status = StatusHandle::new(true);
            let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
            let held = sender.reserve().await.unwrap();
            let mut driver = Driver::start(
                prepared,
                shared.clone(),
                None,
                status.clone(),
                Some(sender.clone()),
                DriverMode::OneAttempt,
            )
            .unwrap();
            test_common::success(&mut fake, "approval_capacity").await;
            tokio::time::timeout(Duration::from_secs(3), async {
                while status.state() != "waiting_approval_capacity" {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            let inspect =
                rusqlite::Connection::open(f.root.path().join("domain/domain.sqlite3")).unwrap();
            let attempts = || {
                inspect
                    .query_row("SELECT COUNT(*) FROM runner_attempts", [], |r| {
                        r.get::<_, u64>(0)
                    })
                    .unwrap()
            };
            assert_eq!(attempts(), 0, "full handoff cannot consume task authority");
            match finish {
                "cancel" => {
                    driver.close().await.unwrap();
                    drop(held);
                }
                "closed" => {
                    receiver.close();
                    drop(held);
                    tokio::time::timeout(Duration::from_secs(3), async {
                        while status.state() != "outcome_unknown" {
                            tokio::task::yield_now().await;
                        }
                    })
                    .await
                    .unwrap();
                    driver.close().await.unwrap();
                }
                _ => {
                    drop(held);
                    tokio::time::timeout(Duration::from_secs(3), async {
                        while status.state() != "outcome_unknown" {
                            tokio::task::yield_now().await;
                        }
                    })
                    .await
                    .unwrap();
                    driver.close().await.unwrap();
                }
            }
            assert_eq!(attempts(), u64::from(finish == "release"));
            assert!(!status.get().workspace_registered);
            assert!(!f.root.path().join("work/owned-mcp.requests").exists());
            assert_eq!(
                sender.capacity(),
                1,
                "original reservation must be released"
            );
            drop(inspect);
            shared.collector.close().await.unwrap();
            test_common::shutdown_domain(&f.store, "approval pre-claim backpressure").await;
        }
    }

    #[tokio::test]
    async fn native_bootstrap_custody_closed_reply() {
        let (reply, pending) = oneshot::channel();
        drop(reply);
        let (control, commands) = mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || drop(commands));
        let mut driver = Driver {
            cancel: CancellationToken::new(),
            control,
            thread: Some(worker),
            pending_close: Some(pending),
            close_unknown: false,
        };
        assert_eq!(driver.close().await, Err(Failure::OutcomeUnknown));
        assert_eq!(driver.close().await, Err(Failure::OutcomeUnknown));
        assert!(driver.close_unknown);
        assert!(driver.pending_close.is_none());
        assert!(driver.thread.is_some()); // A retained wrapper does not prove worker liveness.
        driver.thread.take().unwrap().join().unwrap();
    }

    async fn fixture(f: &test_common::Fixture, endpoint: &str) -> Prepared {
        named_fixture(f, endpoint, "").await
    }
    async fn named_fixture(f: &test_common::Fixture, endpoint: &str, suffix: &str) -> Prepared {
        let name = |prefix: &str| {
            if suffix.is_empty() {
                prefix.to_owned()
            } else {
                format!("{prefix}_{suffix}")
            }
        };
        let transport = f.identity.transport.clone();
        f.store
            .observe_matrix_transport(transport.clone())
            .await
            .unwrap();
        f.store
            .observe_matrix_room(MatrixRoomObservation {
                engagement_id: transport.engagement_id.clone(),
                registration_generation: 1,
                transport_generation: 1,
                room_id: "!direct:example.test".into(),
                generation: 1,
                privacy: RoomPrivacy::Direct {
                    human_mxid: "@owner:example.test".into(),
                },
                joined: BTreeSet::from([
                    "@worker:example.test".into(),
                    "@owner:example.test".into(),
                ]),
                invite_only: true,
                encrypted: true,
            })
            .await
            .unwrap();
        f.store
            .resolve_verified_matrix_session(SessionBinding {
                id: name("bootstrap"),
                engagement_id: transport.engagement_id.clone(),
                room_id: "!direct:example.test".into(),
                thread_root: (!suffix.is_empty()).then(|| format!("${suffix}")),
            })
            .await
            .unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        f.store
            .create_canonical_task(
                name("task"),
                name("bootstrap"),
                "Owned bootstrap".into(),
                now,
            )
            .await
            .unwrap();
        f.store.register_workspace(name("work")).await.unwrap();
        f.store
            .enqueue_dispatch(DispatchInput {
                id: name("dispatch"),
                session_id: name("bootstrap"),
                task_id: Some(name("task")),
                resources: vec![ResourceLease {
                    id: name("work"),
                    exclusive: true,
                }],
                payload: serde_json::json!({"instruction":"offline"}),
            })
            .await
            .unwrap();
        let root = f.root.path().join(name("work"));
        private::directory(&root).unwrap();
        let own = std::env::current_exe().unwrap();
        Prepared {
            host: Host::new(
                own.clone(),
                own,
                BTreeMap::new(),
                BTreeMap::from([(name("work"), root.canonicalize().unwrap())]),
            )
            .unwrap(),
            managed_account: None,
            approval: None,
            provisioning: None,
            warm: None,
            fleet: None,
            max_live: 1,
            matrix: Some(
                f.config(endpoint)
                    .with_root_pem(include_bytes!(
                        "../../../hagency-matrix/tests/fixtures/ca.pem"
                    ))
                    .unwrap(),
            ),
            files: None,
            receives: None,
            enrollment: false,
            receive_inbox: None,
            intake_sessions: Vec::new(),
            agent_inboxes: Vec::new(),
            claim: OwnedClaimProfile::new(
                transport,
                vec![
                    OwnedClaimRoom::new(
                        "!direct:example.test".into(),
                        1,
                        RoomPrivacy::Direct {
                            human_mxid: "@owner:example.test".into(),
                        },
                    )
                    .unwrap(),
                ],
                vec![name("work")],
            )
            .unwrap(),
            limits: Limits {
                operation_ms: 5000,
                response_ms: 1000,
            },
            discard_claim_reply: false,
        }
    }

    pub(crate) async fn workspace_operation(
        f: &test_common::Fixture,
        name: &str,
    ) -> (
        RunnerCapability,
        Operation,
        hagency_execution::WorkspaceRegistration,
    ) {
        let prepared = named_fixture(f, "https://127.0.0.1:1/", name).await;
        std::fs::write(
            f.root.path().join(format!("work_{name}/same.txt")),
            name.as_bytes(),
        )
        .unwrap();
        // This fixture needs17 real claims to test the registry's16-entry cap;
        // production claim limits and operation deadlines remain unchanged.
        let cap = f
            .store
            .claim_owned_dispatch_for_host(
                prepared.claim,
                format!("host_{name}"),
                60_000,
                60_000,
                17,
            )
            .await
            .unwrap()
            .unwrap();
        let mut operation = Operation::start_requiring_workspace(
            f.store.clone(),
            cap.clone(),
            prepared.host,
            prepared.limits,
        )
        .unwrap();
        let registration = tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if let Some(value) = operation.take_workspace_registration() {
                    break value;
                }
                assert!(!operation.is_finished());
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        (cap, operation, registration)
    }

    #[tokio::test]
    async fn native_bootstrap_fleet_workspace_isolation() {
        let f = test_common::Fixture::new();
        let (a, mut first, ra) = workspace_operation(&f, "first").await;
        let (b, mut second, rb) = workspace_operation(&f, "second").await;
        let access = WorkspaceAccess::new();
        let (one, ack_a) = ra.into_parts();
        let (two, ack_b) = rb.into_parts();
        assert!(access.register(a.clone(), one).await.is_ok());
        assert!(access.register(b.clone(), two).await.is_ok());
        let one = access.acquire(&a).await.unwrap();
        let two = access.acquire(&b).await.unwrap();
        let path = hagency_files::RelativeFile::new("same.txt").unwrap();
        assert_eq!(
            one.snapshot(&path, 4 * 1024 * 1024).unwrap().bytes(),
            b"first"
        );
        assert_eq!(
            two.snapshot(&path, 4 * 1024 * 1024).unwrap().bytes(),
            b"second"
        );
        for field in ["secret", "runner", "fence", "dispatch"] {
            let mut foreign = a.clone();
            match field {
                "secret" => foreign.secret = b.secret.clone(),
                "runner" => foreign.runner_id = b.runner_id.clone(),
                "fence" => foreign.fence += 1,
                "dispatch" => foreign.dispatch_id = b.dispatch_id.clone(),
                _ => unreachable!(),
            }
            assert!(access.acquire(&foreign).await.is_err());
        }
        access.retire();
        drop(ack_a);
        drop(ack_b);
        for operation in [&mut first, &mut second] {
            let report = operation.wait().await.unwrap();
            assert_eq!(report.protocol, hagency_execution::Protocol::NotStarted);
        }
        test_common::shutdown_domain(&f.store, "fleet workspace isolation").await;
    }

    #[tokio::test]
    async fn native_bootstrap_fleet_workspace_retirement() {
        let f = test_common::Fixture::new();
        let (a, mut first, ra) = workspace_operation(&f, "first").await;
        let (b, mut second, rb) = workspace_operation(&f, "second").await;
        let access = WorkspaceAccess::new();
        let (one, ack_a) = ra.into_parts();
        let (two, ack_b) = rb.into_parts();
        assert!(access.register(a.clone(), one).await.is_ok());
        assert!(access.register(b.clone(), two).await.is_ok());
        let one = access.acquire(&a).await.unwrap();
        let two = access.acquire(&b).await.unwrap();
        let path = hagency_files::RelativeFile::new("same.txt").unwrap();
        let snapshot = one.snapshot(&path, 4 * 1024 * 1024).unwrap();
        let mut foreign = a.clone();
        foreign.secret = b.secret.clone();
        assert!(access.release(&foreign).is_err());
        one.validate_current().await.unwrap();
        two.validate_current().await.unwrap();
        access.release(&a).unwrap();
        assert!(access.release(&a).is_err());
        assert!(access.acquire(&a).await.is_err());
        assert!(one.validate_current().await.is_err());
        assert!(one.snapshot(&path, 4 * 1024 * 1024).is_err());
        assert_eq!(snapshot.bytes(), b"first");
        access.check(&b).await.unwrap();
        assert_eq!(
            two.snapshot(&path, 4 * 1024 * 1024).unwrap().bytes(),
            b"second"
        );
        access.retire();
        assert!(two.validate_current().await.is_err());
        assert!(access.acquire(&b).await.is_err());
        drop(ack_a);
        drop(ack_b);
        drop(first.wait().await.unwrap());
        drop(second.wait().await.unwrap());
        test_common::shutdown_domain(&f.store, "fleet workspace retirement").await;
    }

    #[tokio::test]
    async fn native_bootstrap_fleet_workspace_capacity() {
        let f = test_common::Fixture::new();
        let access = WorkspaceAccess::new();
        let mut held = Vec::new();
        for index in 0..17 {
            let (cap, operation, registration) =
                workspace_operation(&f, &format!("capacity_{index}")).await;
            let (binding, ack) = registration.into_parts();
            let result = access.register(cap.clone(), binding).await;
            if index < 16 {
                assert!(result.is_ok());
            } else {
                assert!(result.is_err());
            }
            held.push((cap, operation, ack));
        }
        for (cap, _, _) in held.iter().take(16) {
            access.check(cap).await.unwrap();
        }
        access.retire();
        for (_, mut operation, ack) in held {
            drop(ack);
            assert_eq!(
                operation.wait().await.unwrap().protocol,
                hagency_execution::Protocol::NotStarted
            );
        }
        test_common::shutdown_domain(&f.store, "fleet workspace capacity").await;
    }
    #[tokio::test]
    async fn native_bootstrap_unknown_start_lost_claim() {
        let f = test_common::Fixture::new();
        let mut fake = test_common::Fake::start(true).await;
        let mut prepared = fixture(&f, &fake.endpoint).await;
        prepared.discard_claim_reply = true;
        let status = StatusHandle::new(true);
        let shared = Shared::new(prepared.matrix.take().unwrap(), f.store.clone()).unwrap();
        let mut driver = Driver::start(
            prepared,
            shared.clone(),
            None,
            status.clone(),
            None,
            DriverMode::OneAttempt,
        )
        .unwrap();
        test_common::success(&mut fake, "claim_loss").await;
        let until = tokio::time::Instant::now() + Duration::from_secs(5);
        while status.get().state != "outcome_unknown" {
            assert!(
                tokio::time::Instant::now() < until,
                "lost claim was not retained unknown"
            );
            tokio::task::yield_now().await;
        }
        assert_eq!(status.get().error, Some("outcome_unknown"));
        assert!(!status.get().workspace_registered);
        let inspect =
            rusqlite::Connection::open(f.root.path().join("domain/domain.sqlite3")).unwrap();
        assert_eq!(
            inspect
                .query_row("SELECT COUNT(*) FROM runner_attempts", [], |r| r
                    .get::<_, u64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            inspect
                .query_row(
                    "SELECT state FROM runner_dispatches WHERE id='dispatch'",
                    [],
                    |r| r.get::<_, String>(0)
                )
                .unwrap(),
            "leased"
        );
        assert!(!f.root.path().join("work/owned-mcp.requests").exists());
        driver.close().await.unwrap();
        assert_eq!(
            inspect
                .query_row("SELECT COUNT(*) FROM runner_attempts", [], |r| r
                    .get::<_, u64>(0))
                .unwrap(),
            1
        );
        drop(inspect);
        shared.collector.close().await.unwrap();
        test_common::shutdown_domain(&f.store, "bootstrap lost claim").await;
    }
    #[tokio::test]
    async fn native_bootstrap_custody_original_binding() {
        let f = test_common::Fixture::new();
        let fake = test_common::Fake::start(true).await;
        let prepared = fixture(&f, &fake.endpoint).await;
        let cap = f
            .store
            .claim_owned_dispatch_for_host(prepared.claim, "host".into(), 60_000, 60_000, 1)
            .await
            .unwrap()
            .unwrap();
        let mut operation = Operation::start_requiring_workspace(
            f.store.clone(),
            cap.clone(),
            prepared.host,
            prepared.limits,
        )
        .unwrap();
        let until = tokio::time::Instant::now() + Duration::from_secs(3);
        let (binding, ack) = loop {
            if let Some(r) = operation.take_workspace_registration() {
                break r.into_parts();
            }
            assert!(tokio::time::Instant::now() < until && !operation.is_finished());
            tokio::task::yield_now().await;
        };
        let access = WorkspaceAccess::new();
        let mut foreign = cap.clone();
        foreign.secret = "f".repeat(64);
        let rejected = match access.register(foreign, binding).await {
            Ok(()) => panic!("foreign capability registered"),
            Err(value) => value,
        };
        assert_eq!(rejected.capability.secret, "f".repeat(64));
        assert!(access.register(cap.clone(), rejected.binding).await.is_ok());
        access.check(&cap).await.unwrap();
        assert!(access.check(&rejected.capability).await.is_err());
        let other = access.clone();
        other.retire();
        assert!(access.check(&cap).await.is_err());
        drop(ack); // No child was ever given launch ACK.
        let mut report = operation.wait_boxed().await.unwrap();
        assert_eq!(report.protocol, hagency_execution::Protocol::NotStarted);
        assert_eq!(report.cleanup, hagency_runtime::owned::Cleanup::Pending);
        assert!(stopped(&mut report)); // Known pre-child result; domain lease still fenced.
        assert!(other.check(&cap).await.is_err());
        assert!(!f.root.path().join("work/owned-mcp.requests").exists());
        drop(report);
        test_common::shutdown_domain(&f.store, "bootstrap binding").await;
    }

    #[tokio::test]
    async fn native_bootstrap_fleet_stop_custody() {
        let f = test_common::Fixture::new();
        let (a, mut first, ra) = workspace_operation(&f, "a_original").await;
        let (b, mut second, rb) = workspace_operation(&f, "z_foreign").await;
        let shared = Shared::new(f.config("https://127.0.0.1:1/"), f.store.clone()).unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let stop = f
            .store
            .stop_dispatch_for_agent(f.identity.transport.engagement_id.clone(), now)
            .await
            .unwrap();
        assert_eq!(stop["dispatch_id"], b.dispatch_id);
        assert_eq!(stop["stop_pending"], true);
        assert_eq!(stop["stopped"], false);
        // Both bindings and ACKs came from real original Started operations.
        // Withhold launch ACK; no native child or positive stop proof is faked.
        drop(ra);
        let report = first.wait_boxed().await.unwrap();
        assert_eq!(report.protocol, hagency_execution::Protocol::NotStarted);
        let inspect =
            rusqlite::Connection::open(f.root.path().join("domain/domain.sqlite3")).unwrap();
        let count = |query: &str| {
            inspect
                .query_row(query, [], |r| r.get::<_, u64>(0))
                .unwrap()
        };
        assert_eq!(
            count("SELECT COUNT(*) FROM dispatch_stops WHERE settled_at IS NULL"),
            2
        );
        let cancel = CancellationToken::new();
        let status = StatusHandle::new(true);
        let completed = finish_attempt(&f.store, a, report, &shared.collector, &cancel, &status)
            .await
            .unwrap();
        assert_eq!(
            count("SELECT COUNT(*) FROM dispatch_stops WHERE settled_at IS NULL"),
            2,
            "one returned report must not settle the fleet"
        );
        assert_eq!(count("SELECT COUNT(*) FROM resource_leases"), 2);
        assert_eq!(
            count("SELECT COUNT(*) FROM workspace_resources WHERE dirty=1"),
            2
        );
        drop(completed);
        drop(rb);
        let report = second.wait_boxed().await.unwrap();
        assert_eq!(report.protocol, hagency_execution::Protocol::NotStarted);
        drop(
            finish_attempt(&f.store, b, report, &shared.collector, &cancel, &status)
                .await
                .unwrap(),
        );
        assert_eq!(
            count("SELECT COUNT(*) FROM dispatch_stops WHERE settled_at IS NULL"),
            2
        );
        assert_eq!(count("SELECT COUNT(*) FROM resource_leases"), 2);
        assert_eq!(
            count("SELECT COUNT(*) FROM workspace_resources WHERE dirty=1"),
            2
        );
        drop(inspect);
        shared.collector.close().await.unwrap();
        test_common::shutdown_domain(&f.store, "fleet stop custody").await;
    }
}
