//! One pre-activation owner, consumed once by the original dispatch worker.
//! No factory receipt, launcher, capability or canonical task is created here.
use crate::operation::{NativeStartup, OwnedWork, bounded, spawn_prepared};
use crate::{Failure, Limits, Operation, Report, SharedHost};
use hagency_core::{canonical, tasks::RunnerCapability};
use hagency_runtime::{codex::session, owned::OwnedSession};
use hagency_store::{
    DomainStore, OwnedDispatchScope, OwnedProvisionScope, agent_home::ManagedAgentHome,
};
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
    sync::{Notify, mpsc, oneshot},
    time::{Instant, MissedTickBehavior, interval},
};

#[derive(Clone, Copy)]
/// Finite initial operation (100 ms..30 s) and idle budget (100 ms..20 min).
/// These are distinct phases, not extensions of a normal/active operation.
pub struct WarmLimits {
    pub initialize: Limits,
    pub idle_ms: u64,
}
impl WarmLimits {
    pub(crate) fn validate(self) -> bool {
        self.initialize.validate()
            && self.initialize.operation_ms <= 30_000
            && (100..=hagency_runtime::codex::MAX_REQUEST_MS).contains(&self.idle_ms)
    }
}
struct Ready {
    result: Mutex<Option<Result<(), Failure>>>,
    changed: Notify,
}
impl Ready {
    fn read(&self) -> Result<Option<Result<(), Failure>>, Failure> {
        self.result
            .lock()
            .map(|result| result.clone())
            .map_err(|_| Failure::Worker)
    }
    fn set(&self, result: Result<(), Failure>) {
        if let Ok(mut current) = self.result.lock() {
            // A later cancellation/teardown cannot rewrite the original
            // observed failure (including an expired retained ready wait).
            if !matches!(current.as_ref(), Some(Err(_))) {
                *current = Some(result);
            }
        }
        self.changed.notify_one();
    }
}
/// Owning, non-cloneable handle. A dropped ready wait leaves this exact worker
/// intact; dropping the handle explicitly cancels and synchronously joins it.
/// Like Operation::drop, join must not run on a latency-sensitive HTTP/UI worker.
pub struct WarmRuntime {
    worker: Option<JoinHandle<()>>,
    command: mpsc::Sender<Command>,
    inspection: Option<Inspection>,
    activation: Option<Activation>,
    activated: bool,
    response_ms: u64,
    cancel: Arc<AtomicBool>,
    ready: Arc<Ready>,
    domain: DomainStore,
    host: SharedHost,
}
enum Command {
    Dispatch {
        work: OwnedWork,
        until: Instant,
    },
    Observe {
        until: Instant,
        reply: oneshot::Sender<Result<(), Failure>>,
    },
    Activate {
        until: Instant,
        reply: oneshot::Sender<Result<hagency_core::project::Engagement, Failure>>,
    },
}
struct Inspection {
    until: Instant,
    reply: oneshot::Receiver<Result<(), Failure>>,
}
struct Activation {
    until: Instant,
    reply: oneshot::Receiver<Result<hagency_core::project::Engagement, Failure>>,
}
#[derive(Clone)]
pub(crate) struct Binding {
    scope: OwnedProvisionScope,
    home: Arc<ManagedAgentHome>,
    root: Arc<crate::workspace::Root>,
    workspace_id: String,
    failure: Option<Failure>,
    local_codex: Option<Arc<crate::LocalCodex>>,
}
impl Binding {
    async fn current(
        &self,
        domain: &DomainStore,
        cancel: &AtomicBool,
        until: Instant,
    ) -> Result<(), Failure> {
        if let Some(local) = &self.local_codex {
            local.admit_provision(&self.scope)?;
        }
        self.root.check().map_err(|_| Failure::LostAuthority)?;
        self.home
            .check_provision_scope(&self.scope)
            .map_err(|_| Failure::LostAuthority)?;
        bounded(
            domain.validate_warm_runtime_scope(self.scope.clone()),
            cancel,
            until,
        )
        .await?
        .map_err(|_| Failure::LostAuthority)?;
        self.root.check().map_err(|_| Failure::LostAuthority)?;
        self.home
            .check_provision_scope(&self.scope)
            .map_err(|_| Failure::LostAuthority)?;
        if let Some(local) = &self.local_codex {
            local.admit_provision(&self.scope)?;
        }
        Ok(())
    }
    pub(crate) async fn check(
        &self,
        domain: &DomainStore,
        dispatch: &OwnedDispatchScope,
        cancel: &AtomicBool,
        until: Instant,
    ) -> Result<(), Failure> {
        if let Some(failure) = &self.failure {
            return Err(failure.clone());
        }
        let [workspace] = dispatch.input().resources.as_slice() else {
            return Err(Failure::Admission);
        };
        if dispatch.engagement_id() != self.scope.engagement_id()
            || !workspace.exclusive
            || workspace.id != self.workspace_id
            || canonical::transport_digest(&serde_json::json!(dispatch.resource()))
                .map_err(|_| Failure::Admission)?
                != canonical::transport_digest(&serde_json::json!(self.scope.resource()))
                    .map_err(|_| Failure::Admission)?
        {
            return Err(Failure::Admission);
        }
        self.current(domain, cancel, until).await
    }
}
impl WarmRuntime {
    pub fn start(
        domain: DomainStore,
        scope: OwnedProvisionScope,
        home: Arc<ManagedAgentHome>,
        host: SharedHost,
        workspace_id: String,
        limits: WarmLimits,
    ) -> Result<Self, Failure> {
        let until = Instant::now() + Duration::from_millis(limits.initialize.operation_ms);
        Self::start_at(domain, scope, home, host, workspace_id, limits, until)
    }
    pub(crate) fn start_at(
        domain: DomainStore,
        scope: OwnedProvisionScope,
        home: Arc<ManagedAgentHome>,
        host: SharedHost,
        workspace_id: String,
        limits: WarmLimits,
        until: Instant,
    ) -> Result<Self, Failure> {
        if !limits.validate() {
            return Err(Failure::Admission);
        }
        if Instant::now() >= until {
            return Err(Failure::Deadline);
        }
        // Shared by all recaptured/cloned scopes in the producing writer. A
        // failed/unknown original job never grants another warm attempt.
        scope.claim_warm().map_err(|_| Failure::Admission)?;
        let prepared = host
            .0
            .prepare_warm(&scope, &home, &workspace_id, limits.initialize)?;
        let live = match &host.0.approvals {
            // Initialize has no turn/approval channel. Reserve original process
            // capacity here; the active handoff separately enforces fits().
            Some(policy) => Some(policy.reserve_live()?),
            None => None,
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| Failure::Worker)?;
        let (command, mut receive) = mpsc::channel(1);
        let cancel = Arc::new(AtomicBool::new(false));
        let signal = cancel.clone();
        let ready = Arc::new(Ready {
            result: Mutex::new(None),
            changed: Notify::new(),
        });
        let notice = ready.clone();
        let source = domain.clone();
        let fixed = host.clone();
        // Keep the original deadline including preparation and worker/queue delay.
        let worker = std::thread::Builder::new()
            .name("hagency-warm-owned-runtime".into())
            .spawn(move || {
                let mut report = Box::new(Report::new(Arc::new(Mutex::new(None))));
                report.warm = Some(Binding {
                    scope,
                    home,
                    root: prepared.root.clone(),
                    workspace_id,
                    failure: None,
                    local_codex: fixed.0.local_codex.clone(),
                });
                report.account = prepared.account;
                report.live = live;
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    runtime.block_on(retain(
                        &source,
                        &fixed,
                        NativeStartup {
                            launch: prepared.launch,
                            settings: prepared.settings,
                            io_limits: prepared.io_limits,
                        },
                        limits,
                        until,
                        Controls {
                            cancel: &signal,
                            ready: &notice,
                            receive: &mut receive,
                        },
                        &mut report,
                    ))
                }))
                .unwrap_or(Err(Failure::Worker));
                match outcome {
                    // block_on has returned, so the original work runs on this very
                    // runtime/thread, never inside a nested or replacement reactor.
                    Ok(work) => work(&runtime, Some(report)),
                    Err(failure) => {
                        // Serialize failure with admission: an already-enqueued
                        // dispatch must still get the original worker's negative
                        // finalization, not a silently discarded result channel.
                        notice.set(Err(failure.clone()));
                        match receive.try_recv() {
                            Ok(Command::Dispatch { work, .. }) => {
                                if let Some(binding) = &mut report.warm {
                                    binding.failure = Some(failure);
                                }
                                work(&runtime, Some(report));
                            }
                            Ok(Command::Observe { reply, .. }) => {
                                let _ = reply.send(Err(failure));
                                report.retry_stop();
                            }
                            Ok(Command::Activate { reply, .. }) => {
                                let _ = reply.send(Err(failure));
                                report.retry_stop();
                            }
                            Err(_) => {
                                report.retry_stop();
                            }
                        }
                    }
                }
            })
            .map_err(|_| Failure::Worker)?;
        Ok(Self {
            worker: Some(worker),
            command,
            inspection: None,
            activation: None,
            activated: false,
            response_ms: limits.initialize.response_ms,
            cancel,
            ready,
            domain,
            host,
        })
    }
    /// Only the concrete factory bridge requests activation after checking its
    /// original enrolled SDK. The original worker qualifies its physical owner
    /// immediately before the same writer's scoped transaction, then rechecks.
    pub(crate) async fn activate(&mut self) -> Result<hagency_core::project::Engagement, Failure> {
        if self.activated || self.inspection.is_some() {
            return Err(Failure::Admission);
        }
        match self.ready.read()? {
            Some(Ok(())) => {}
            Some(Err(failure)) => return Err(failure),
            None => return Err(Failure::Admission),
        }
        if self.cancel.load(Ordering::Acquire) {
            return Err(Failure::Cancelled);
        }
        if self.activation.is_none() {
            let until = Instant::now() + Duration::from_millis(self.response_ms);
            let (reply, receive) = oneshot::channel();
            self.command
                .try_send(Command::Activate { until, reply })
                .map_err(|_| Failure::LostAuthority)?;
            self.activation = Some(Activation {
                until,
                reply: receive,
            });
        }
        let activation = self.activation.as_mut().ok_or(Failure::Worker)?;
        let until = activation.until;
        let mut result = match tokio::time::timeout_at(until, &mut activation.reply).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(Failure::LostAuthority),
            Err(_) => Err(Failure::Deadline),
        };
        if result.is_ok() {
            if let Some(Err(failure)) = self.ready.read()? {
                result = Err(failure);
            } else if self.cancel.load(Ordering::Acquire) {
                result = Err(Failure::Cancelled);
            } else if Instant::now() >= until {
                result = Err(Failure::Deadline);
            }
        }
        self.activation = None;
        match &result {
            Ok(_) => self.activated = true,
            Err(failure) => {
                self.ready.set(Err(failure.clone()));
                self.cancel();
            }
        }
        result
    }
    /// Fresh inspection on the original worker. A dropped wait retains its
    /// exact receiver/deadline; an expired buffered positive cannot be reused.
    pub async fn ready(&mut self) -> Result<(), Failure> {
        loop {
            let changed = self.ready.changed.notified();
            if let Some(result) = self.ready.read()? {
                result?;
                break;
            }
            changed.await;
        }
        if self.cancel.load(Ordering::Acquire) {
            return Err(Failure::Cancelled);
        }
        if self.inspection.is_none() {
            let until = Instant::now() + Duration::from_millis(self.response_ms);
            let (reply, receive) = oneshot::channel();
            self.command
                .try_send(Command::Observe { until, reply })
                .map_err(|_| Failure::LostAuthority)?;
            self.inspection = Some(Inspection {
                until,
                reply: receive,
            });
        }
        // Borrow the retained receiver: dropping this wait cannot discard or
        // enqueue a replacement inspection, or extend its original deadline.
        let inspection = self.inspection.as_mut().ok_or(Failure::Worker)?;
        let until = inspection.until;
        let mut result = match tokio::time::timeout_at(until, &mut inspection.reply).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(Failure::LostAuthority),
            Err(_) => Err(Failure::Deadline),
        };
        if result.is_ok() {
            if let Some(Err(failure)) = self.ready.read()? {
                result = Err(failure);
            } else if self.cancel.load(Ordering::Acquire) {
                result = Err(Failure::Cancelled);
            } else if Instant::now() >= until {
                result = Err(Failure::Deadline);
            }
        }
        self.inspection = None;
        if let Err(failure) = &result {
            self.ready.set(Err(failure.clone()));
            self.cancel();
        }
        result
    }
    pub fn is_finished(&self) -> bool {
        self.worker.as_ref().is_none_or(JoinHandle::is_finished)
    }
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Release);
    }
    pub fn dispatch(
        self,
        capability: RunnerCapability,
        limits: Limits,
    ) -> Result<Operation, Failure> {
        self.handoff(capability, limits, false)
    }
    pub fn dispatch_requiring_workspace(
        self,
        capability: RunnerCapability,
        limits: Limits,
    ) -> Result<Operation, Failure> {
        self.handoff(capability, limits, true)
    }
    fn handoff(
        mut self,
        capability: RunnerCapability,
        limits: Limits,
        required: bool,
    ) -> Result<Operation, Failure> {
        if self.cancel.load(Ordering::Acquire) {
            return Err(Failure::Cancelled);
        }
        if self.inspection.is_some() || self.activation.is_some() {
            return Err(Failure::Admission);
        }
        match self.ready.read()? {
            Some(Ok(())) => {}
            Some(Err(failure)) => return Err(failure),
            None => return Err(Failure::Admission),
        }
        let until = Instant::now() + Duration::from_millis(limits.operation_ms);
        let (mut operation, work) = Operation::prepare(
            self.domain.clone(),
            capability,
            self.host.clone(),
            limits,
            required,
            true,
        )?;
        let current = self.ready.result.lock().map_err(|_| Failure::Worker)?;
        match current.as_ref() {
            Some(Ok(())) => {}
            Some(Err(failure)) => return Err(failure.clone()),
            None => return Err(Failure::Admission),
        }
        self.command
            .try_send(Command::Dispatch { work, until })
            .map_err(|_| Failure::LostAuthority)?;
        drop(current);
        operation.adopt_worker(self.worker.take().ok_or(Failure::Worker)?);
        // With no worker left, Drop must not cancel the transferred owner.
        Ok(operation)
    }
}
impl Drop for WarmRuntime {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            self.cancel();
            let _ = worker.join();
        }
    }
}
async fn initialized<F: Future<Output = Result<(), session::Error>>>(
    future: F,
    binding: &Binding,
    domain: &DomainStore,
    account: Option<&hagency_store::ManagedLaunch>,
    cancel: &AtomicBool,
    until: Instant,
) -> Result<(), Failure> {
    tokio::pin!(future);
    let mut tick = interval(Duration::from_millis(100));
    tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            biased;
            _=tokio::time::sleep_until(until)=>return Err(Failure::Deadline),
            _=tick.tick()=>{binding.current(domain,cancel,until).await?;if let Some(account)=account {account.check().map_err(|_|Failure::LostAuthority)?;}},
            result=&mut future=>return result.map_err(|_|Failure::Protocol),
        }
    }
}
struct Controls<'owner> {
    cancel: &'owner Arc<AtomicBool>,
    ready: &'owner Ready,
    receive: &'owner mut mpsc::Receiver<Command>,
}
async fn qualify(
    domain: &DomainStore,
    binding: &Binding,
    owner: &mut OwnedSession,
    account: Option<&hagency_store::ManagedLaunch>,
    cancel: &AtomicBool,
    until: Instant,
) -> Result<(), Failure> {
    binding.current(domain, cancel, until).await?;
    if let Some(account) = account {
        account.check().map_err(|_| Failure::LostAuthority)?;
    }
    let remaining = until
        .checked_duration_since(Instant::now())
        .filter(|duration| !duration.is_zero())
        .ok_or(Failure::Deadline)?;
    owner
        .qualify_ready_owner(remaining.min(Duration::from_secs(5)))
        .map_err(|_| Failure::LostAuthority)?;
    // Actual physical IO precedes the final original writer observation.
    binding.current(domain, cancel, until).await?;
    if let Some(account) = account {
        account.check().map_err(|_| Failure::LostAuthority)?;
    }
    if cancel.load(Ordering::Acquire) {
        return Err(Failure::Cancelled);
    }
    if Instant::now() >= until {
        return Err(Failure::Deadline);
    }
    Ok(())
}
async fn retain(
    domain: &DomainStore,
    host: &SharedHost,
    startup: NativeStartup,
    limits: WarmLimits,
    until: Instant,
    controls: Controls<'_>,
    report: &mut Report,
) -> Result<OwnedWork, Failure> {
    let Controls {
        cancel,
        ready,
        receive,
    } = controls;
    report
        .warm
        .as_ref()
        .ok_or(Failure::Worker)?
        .current(domain, cancel, until)
        .await?;
    if let Some(live) = &mut report.live {
        live.possible();
    }
    spawn_prepared(
        host.0.clone(),
        startup,
        limits.initialize,
        cancel,
        until,
        report,
    )
    .await?;
    let binding = report.warm.as_ref().ok_or(Failure::Worker)?;
    let owner: &mut OwnedSession = report.owner.as_mut().ok_or(Failure::SpawnFailed)?;
    owner.reserve_warm_idle().map_err(|_| Failure::Protocol)?;
    bounded(
        initialized(
            owner.initialize(),
            binding,
            domain,
            report.account.as_ref(),
            cancel,
            until,
        ),
        cancel,
        until,
    )
    .await??;
    binding.current(domain, cancel, until).await?;
    if let Some(account) = &report.account {
        account.check().map_err(|_| Failure::LostAuthority)?;
    }
    let idle_until = Instant::now() + Duration::from_millis(limits.idle_ms);
    owner
        .enter_warm_idle(idle_until)
        .map_err(|_| Failure::Protocol)?;
    ready.set(Ok(()));
    let wait = async {
        let mut tick = interval(Duration::from_millis(100));
        tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                biased;
                _=tick.tick()=>qualify(domain,binding,owner,report.account.as_ref(),cancel,
                    idle_until.min(Instant::now()+Duration::from_millis(limits.initialize.response_ms))).await?,
                command=receive.recv()=>match command.ok_or(Failure::Cancelled)? {
                    Command::Dispatch {work,until}=>{
                        // Transfer the closure out of the cancellable wait
                        // before any inspection await can drop consumed work.
                        return Ok((work,until));
                    },
                    Command::Observe {until,reply}=>{
                        let result=qualify(domain,binding,owner,report.account.as_ref(),cancel,until.min(idle_until)).await;
                        let failure=result.clone().err();let _=reply.send(result);
                        if let Some(failure)=failure {return Err(failure);}
                    },
                    Command::Activate {until,reply}=>{
                        let until=until.min(idle_until);
                        let result=async {
                            qualify(domain,binding,owner,report.account.as_ref(),cancel,until).await?;
                            let acknowledgment=bounded(domain.complete_original_provision(binding.scope.clone()),cancel,until).await?
                                .map_err(|_|Failure::LostAuthority)?;
                            qualify(domain,binding,owner,report.account.as_ref(),cancel,until).await?;
                            Ok(acknowledgment)
                        }.await;
                        let failure=result.clone().err();let _=reply.send(result);
                        if let Some(failure)=failure {return Err(failure);}
                    },
                },
            }
        }
    };
    let (work, dispatch_until) = bounded(wait, cancel, idle_until).await??;
    // The owned work is now retained outside the bounded waiting future. Even
    // failed/expired physical qualification runs its original finalization.
    let failure = qualify(
        domain,
        report.warm.as_ref().ok_or(Failure::Worker)?,
        report.owner.as_mut().ok_or(Failure::Worker)?,
        report.account.as_ref(),
        cancel,
        dispatch_until
            .min(idle_until)
            .min(Instant::now() + Duration::from_millis(limits.initialize.response_ms)),
    )
    .await
    .err();
    if let Some(failure) = failure {
        report.warm.as_mut().ok_or(Failure::Worker)?.failure = Some(failure);
    }
    Ok(work)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_warm_ready_failure_sticky() {
        let ready = Ready {
            result: Mutex::new(None),
            changed: Notify::new(),
        };
        ready.set(Ok(()));
        assert_eq!(ready.read().unwrap(), Some(Ok(())));
        ready.set(Err(Failure::Deadline));
        ready.set(Err(Failure::Cancelled));
        ready.set(Ok(()));
        assert_eq!(ready.read().unwrap(), Some(Err(Failure::Deadline)));
    }
}
