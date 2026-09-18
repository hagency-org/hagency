//! Consume the original physical job, SDK and initialized native owner inline.
use super::*;
use crate::Collector;
use hagency_core::{
    project::EngagementState,
    replies::RoomPrivacy,
    tasks::{RunnerCapability, SessionBinding},
};
use hagency_execution::{FactoryRuntime, Failure, Operation, WarmHostPlan};
use hagency_store::{Effect, OwnedClaimProfile, OwnedClaimRoom};
use std::sync::atomic::{AtomicBool, Ordering};

pub(super) struct Custody {
    runtime: tokio::sync::Mutex<Option<FactoryRuntime>>,
    collector: Mutex<Option<Collector>>,
    binding: Mutex<Option<SessionBinding>>,
    taken: AtomicBool,
    closed: AtomicBool,
    closure: Mutex<Closure>,
}
enum Closure {
    Waiting,
    Running,
    Complete(Result<(), Error>),
}
impl Custody {
    pub(super) fn new() -> Self {
        Self {
            runtime: tokio::sync::Mutex::new(None),
            collector: Mutex::new(None),
            binding: Mutex::new(None),
            taken: AtomicBool::new(false),
            closed: AtomicBool::new(false),
            closure: Mutex::new(Closure::Waiting),
        }
    }
    async fn ready(&self, cancel: &CancellationToken) -> Result<(), Error> {
        let mut runtime = self.runtime.lock().await;
        let owner = runtime.as_mut().ok_or(Error::OutcomeUnknown)?;
        let result = tokio::select! {result=owner.ready()=>result.map_err(|_|Error::OutcomeUnknown),_ = cancel.cancelled()=>Err(Error::Cancelled)};
        if result.is_err() {
            owner.cancel();
        }
        result
    }
    pub(super) async fn cancel(&self) {
        if let Some(runtime) = self.runtime.lock().await.as_ref() {
            runtime.cancel();
        }
    }
    async fn activate(
        &self,
        cancel: &CancellationToken,
    ) -> Result<hagency_core::project::Engagement, Error> {
        let mut runtime = self.runtime.lock().await;
        let owner = runtime.as_mut().ok_or(Error::OutcomeUnknown)?;
        let result = tokio::select! {result=owner.activate()=>result.map_err(|_|Error::OutcomeUnknown),_ = cancel.cancelled()=>Err(Error::Cancelled)};
        if result.is_err() {
            owner.cancel();
        }
        result
    }
    pub(super) async fn fence_failure(&self, error: Error) -> Error {
        let collector = match self.collector.lock() {
            Ok(owner) => owner.as_ref().map(|c| Collector {
                inner: c.inner.clone(),
            }),
            Err(_) => return Error::OutcomeUnknown,
        };
        let Some(collector) = collector else {
            return error;
        };
        let inner = &collector.inner;
        match inner
            .domain
            .matrix_transport_state(inner.config.identity.transport.engagement_id.clone())
            .await
        {
            Ok(Some(state))
                if state.available && state.observation == inner.config.identity.transport =>
            {
                match inner
                    .fence_observation::<()>(state.observation, error)
                    .await
                {
                    Err(error) => error,
                    Ok(()) => Error::OutcomeUnknown,
                }
            }
            Ok(_) => error,
            Err(_) => Error::OutcomeUnknown,
        }
    }
    pub(super) async fn close(self: &Arc<Self>) -> Result<(), Error> {
        {
            let closure = self.closure.lock().map_err(|_| Error::OutcomeUnknown)?;
            match &*closure {
                Closure::Waiting => {}
                Closure::Running => return Err(Error::Busy),
                Closure::Complete(result) => return result.clone(),
            }
        }
        let collector = self
            .collector
            .lock()
            .map_err(|_| Error::OutcomeUnknown)?
            .as_ref()
            .map(|c| Collector {
                inner: c.inner.clone(),
            });
        // Busy is known pre-effect refusal. Once admitted, the exact SDK busy
        // permit stays with the original join/close job through caller loss.
        let permit = collector
            .as_ref()
            .map(|c| {
                c.inner
                    .busy
                    .clone()
                    .try_acquire_owned()
                    .map_err(|_| Error::Busy)
            })
            .transpose()?;
        {
            let mut closure = self.closure.lock().map_err(|_| Error::OutcomeUnknown)?;
            match &*closure {
                Closure::Waiting => {}
                Closure::Running => return Err(Error::Busy),
                Closure::Complete(result) => return result.clone(),
            }
            *closure = Closure::Running;
        }
        self.closed.store(true, Ordering::Release);
        let original = self.clone();
        // Retained owned cleanup survives caller loss. A blocking join is not
        // a whole-tree cleanup observation or permission to free dirty leases.
        tokio::spawn(async move {
            let result = async {
                let runtime = original.runtime.lock().await.take();
                tokio::task::spawn_blocking(move || {
                    if let Some(runtime) = runtime {
                        runtime.cancel();
                        drop(runtime);
                    }
                })
                .await
                .map_err(|_| Error::OutcomeUnknown)?;
                if let (Some(collector), Some(permit)) = (collector, permit) {
                    collector.close_with_permit(permit).await?;
                }
                Ok(())
            }
            .await;
            *original.closure.lock().map_err(|_| Error::OutcomeUnknown)? =
                Closure::Complete(result.clone());
            result
        })
        .await
        .map_err(|_| Error::OutcomeUnknown)?
    }
}
/// One non-cloneable sequential-dispatch owner. Collector references retain the very
/// same enrolled SDK; no credential, replacement config or ready setter escapes.
pub struct ProvisionedAgent {
    custody: Arc<Custody>,
    collector: Arc<Collector>,
    binding: SessionBinding,
    workspace: String,
}
impl ProvisionedAgent {
    pub fn collector(&self) -> &Collector {
        &self.collector
    }
    /// Share the same enrolled queue/owner with this agent's native file
    /// services. This does not construct a new SDK or export configuration.
    pub fn shared_collector(&self) -> Arc<Collector> {
        self.collector.clone()
    }
    pub fn session(&self) -> &SessionBinding {
        &self.binding
    }
    pub fn workspace_id(&self) -> &str {
        &self.workspace
    }
    /// Fresh host scheduling metadata after this original collector refreshed.
    /// Old sessions remain retired; no runtime-facing request can supply a room.
    pub async fn inboxes(
        &self,
        profile: OwnedClaimProfile,
    ) -> Result<
        (
            OwnedClaimProfile,
            Vec<hagency_core::agent_inbox::AgentInboxPlan>,
        ),
        Error,
    > {
        use hagency_core::agent_inbox::AgentInboxPlan;
        if self.custody.closed.load(Ordering::Acquire) {
            return Err(Error::Generation);
        }
        let inner = &self.collector.inner;
        let transport = inner.expected_transport().await?;
        let mut inboxes = vec![AgentInboxPlan {
            session_id: self.binding.id.clone(),
            workspace_id: self.workspace.clone(),
        }];
        let mut rooms = Vec::new();
        for room in &inner.config.rooms {
            let generation = inner.observed_room_generation(room).await?;
            let selected =
                OwnedClaimRoom::new(room.room_id.clone(), generation, room.privacy.clone())?;
            rooms.push(if matches!(room.privacy, RoomPrivacy::Group {}) {
                selected.with_plaintext_project()?
            } else {
                selected
            });
            if !matches!(room.privacy, RoomPrivacy::Group {}) {
                continue;
            }
            if inner.config.factory_rooms.is_none() {
                return Err(Error::Config);
            }
            let binding = SessionBinding {
                id: format!(
                    "project_{}_{}_{}",
                    transport.engagement_id, transport.generation, generation
                ),
                engagement_id: transport.engagement_id.clone(),
                room_id: room.room_id.clone(),
                thread_root: None,
            };
            let resolved = inner
                .domain
                .resolve_verified_matrix_session(binding.clone())
                .await?;
            if resolved.id != binding.id
                || resolved.engagement_id != binding.engagement_id
                || resolved.room_id != binding.room_id
                || resolved.thread_root != binding.thread_root
            {
                return Err(Error::Conflict);
            }
            let route = inner.domain.matrix_intake_route(binding.id.clone()).await?;
            if route.room_generation != generation
                || route.transport_generation != transport.generation
                || route.privacy != room.privacy
            {
                return Err(Error::Generation);
            }
            inboxes.push(AgentInboxPlan {
                session_id: binding.id,
                workspace_id: self.workspace.clone(),
            });
        }
        let profile = profile.refresh_matrix_rooms(transport, rooms)?;
        Ok((profile, inboxes))
    }
    pub async fn claim_profile(&self) -> Result<OwnedClaimProfile, Error> {
        if self.custody.closed.load(Ordering::Acquire) {
            return Err(Error::Generation);
        }
        let transport = self.collector.inner.expected_transport().await?;
        let rooms = self
            .collector
            .inner
            .config
            .rooms
            .iter()
            .map(|room| {
                OwnedClaimRoom::new(room.room_id.clone(), room.generation, room.privacy.clone())
            })
            .collect::<Result<Vec<_>, _>>()?;
        let profile = OwnedClaimProfile::new(transport, rooms, vec![self.workspace.clone()])?;
        self.custody
            .runtime
            .lock()
            .await
            .as_ref()
            .ok_or(Error::OutcomeUnknown)?
            .bind_claim_profile(profile)
            .map_err(|_| Error::OutcomeUnknown)
    }
    pub async fn dispatch(
        &mut self,
        capability: RunnerCapability,
        limits: hagency_execution::Limits,
    ) -> Result<Operation, Failure> {
        let ticket = {
            let mut runtime = self.custody.runtime.lock().await;
            if self.custody.closed.load(Ordering::Acquire) {
                return Err(Failure::Admission);
            }
            runtime
                .as_mut()
                .ok_or(Failure::Admission)?
                .reserve_dispatch(capability, limits)?
        };
        let original = self.custody.clone();
        // The original runtime spends its exact ticket before enqueue. A later
        // ticket requires the original operation's private acknowledged witness;
        // public Report output and lost waiters cannot authorize one.
        tokio::task::spawn_blocking(move || {
            let mut runtime = original.runtime.blocking_lock();
            if original.closed.load(Ordering::Acquire) {
                return Err(Failure::Admission);
            }
            runtime
                .as_mut()
                .ok_or(Failure::Admission)?
                .dispatch_reserved(ticket)
        })
        .await
        .map_err(|_| Failure::Worker)?
    }
    pub async fn close(self) -> Result<(), Error> {
        self.custody.close().await
    }
}
impl TokenProvisioningHost {
    /// Concrete full inline runtime consumer; existing checkpoint profiles are
    /// unchanged unless this additional private Host capability is configured.
    pub fn with_warm_runtime(
        mut self,
        plan: WarmHostPlan,
        approvals: Arc<crate::ApprovalCollector>,
    ) -> Result<Self, Error> {
        if self.warm.is_some() || self.homes.is_none() || self.rooms.is_none() {
            return Err(Error::Config);
        }
        let config = &approvals.inner.config;
        if !config.approval
            || config.endpoint != self.endpoint
            || config.identity.server_name != self.registration.server_name
            || config.identity.registration_fingerprint != self.fingerprint
            || config.identity.transport.registration_generation != self.registration.generation
            || config.identity.transport.sender_mxid != self.registration.approval_bot_mxid
        {
            return Err(Error::Config);
        }
        self.factory_approvals = Some(approvals);
        self.warm = Some(plan);
        Ok(self)
    }
    pub(super) async fn finish_factory(
        &self,
        domain: &DomainStore,
        effect: &Effect,
        account: &Arc<ProvisionedTokenAccount>,
        job: &Arc<Job>,
        cancel: &CancellationToken,
        activated: &mut bool,
    ) -> Result<(), Error> {
        let plan = self.warm.as_ref().ok_or(Error::Config)?;
        let scope = domain
            .provision_runtime_scope(effect.clone(), self.registration.clone())
            .await?;
        let approvals = self.factory_approvals.as_ref().ok_or(Error::Config)?;
        // Only the original producing writer may contribute this capability.
        approvals
            .inner
            .domain
            .validate_warm_runtime_scope(scope.clone())
            .await?;
        let home = job
            .home
            .lock()
            .map_err(|_| Error::OutcomeUnknown)?
            .as_ref()
            .cloned()
            .ok_or(Error::Config)?;
        let custody = job.factory.as_ref().ok_or(Error::Config)?;
        let runtime = plan
            .start(domain.clone(), scope.clone(), home)
            .await
            .map_err(|_| Error::OutcomeUnknown)?;
        *custody.runtime.lock().await = Some(runtime);
        custody.ready(cancel).await?;
        // GET-only current verification on the original successful SDK job;
        // its Complete ledger prevents any signing upload/session claim replay.
        let rooms = self.rooms.as_ref().ok_or(Error::Config)?;
        account
            .enroll_created_rooms(1, self.key, rooms.anchors.clone(), cancel)
            .await?;
        custody.ready(cancel).await?;
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let acknowledgment = custody.activate(cancel).await?;
        if acknowledgment.id != effect.engagement_id
            || acknowledgment.state != EngagementState::Active
        {
            return Err(Error::OutcomeUnknown);
        }
        *activated = true; // Only this received original writer ACK grants forward custody.
        // Native approval control needs its own authenticated current binding;
        // an Agent SDK/DM cannot stand in for the fixed shared approval bot.
        {
            let _turn = approvals.service_turn(cancel).await?;
            approvals
                .observe_factory_engagement(scope.clone(), cancel)
                .await?;
        }
        let collector = account.active_collector(cancel).await?;
        *custody
            .collector
            .lock()
            .map_err(|_| Error::OutcomeUnknown)? = Some(collector);
        custody.ready(cancel).await?;
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let room_id = account.created_agent_dm()?.ok_or(Error::Recipients)?;
        {
            let owner = custody
                .collector
                .lock()
                .map_err(|_| Error::OutcomeUnknown)?;
            if !owner
                .as_ref()
                .ok_or(Error::OutcomeUnknown)?
                .inner
                .config
                .rooms
                .iter()
                .any(|room| {
                    room.room_id == room_id && matches!(room.privacy, RoomPrivacy::Direct { .. })
                })
            {
                return Err(Error::Recipients);
            }
        }
        let workspace = format!("work_{}", effect.engagement_id);
        domain.register_workspace(workspace).await?;
        let binding = SessionBinding {
            id: format!("session_{}", effect.engagement_id),
            engagement_id: effect.engagement_id.clone(),
            room_id,
            thread_root: None,
        };
        let resolved = domain
            .resolve_verified_matrix_session(binding.clone())
            .await?;
        if resolved.id != binding.id
            || resolved.engagement_id != binding.engagement_id
            || resolved.room_id != binding.room_id
            || resolved.thread_root != binding.thread_root
        {
            return Err(Error::Conflict);
        }
        *custody.binding.lock().map_err(|_| Error::OutcomeUnknown)? = Some(binding);
        Ok(())
    }
    pub(super) fn take_agent(&self, engagement: &str) -> Result<ProvisionedAgent, Error> {
        let job = self
            .jobs
            .lock()
            .map_err(|_| Error::OutcomeUnknown)?
            .get(&format!("provision_{engagement}"))
            .cloned()
            .ok_or(Error::Config)?;
        match job
            .result
            .lock()
            .map_err(|_| Error::OutcomeUnknown)?
            .as_ref()
        {
            Some(Ok(_)) => {}
            Some(Err(error)) => return Err(error.clone()),
            None => return Err(Error::OutcomeUnknown),
        }
        let custody = job.factory.as_ref().cloned().ok_or(Error::Config)?;
        if custody.closed.load(Ordering::Acquire) {
            return Err(Error::Generation);
        }
        let binding = custody
            .binding
            .lock()
            .map_err(|_| Error::OutcomeUnknown)?
            .clone()
            .ok_or(Error::OutcomeUnknown)?;
        let collector = custody
            .collector
            .lock()
            .map_err(|_| Error::OutcomeUnknown)?
            .as_ref()
            .map(|c| Collector {
                inner: c.inner.clone(),
            })
            .ok_or(Error::OutcomeUnknown)?;
        if custody.taken.swap(true, Ordering::AcqRel) {
            return Err(Error::Busy);
        }
        Ok(ProvisionedAgent {
            custody,
            collector: Arc::new(collector),
            binding,
            workspace: format!("work_{engagement}"),
        })
    }
    fn take_next_agent(&self) -> Result<Option<ProvisionedAgent>, Error> {
        if self.closed.load(Ordering::Acquire) {
            return Err(Error::Generation);
        }
        // Metadata inspection never reconstructs an owner from canonical
        // Active state. Only the retained job's actual successful result and
        // non-cloneable take below can admit an original agent.
        let jobs = self.jobs.lock().map_err(|_| Error::OutcomeUnknown)?;
        let mut next = None;
        for (id, job) in jobs.iter() {
            let Some(custody) = &job.factory else {
                continue;
            };
            if custody.closed.load(Ordering::Acquire) || custody.taken.load(Ordering::Acquire) {
                continue;
            }
            if matches!(
                job.result
                    .lock()
                    .map_err(|_| Error::OutcomeUnknown)?
                    .as_ref(),
                Some(Ok(_))
            ) {
                next = Some(
                    id.strip_prefix("provision_")
                        .ok_or(Error::Config)?
                        .to_owned(),
                );
                break;
            }
        }
        drop(jobs);
        next.map(|id| self.take_agent(&id)).transpose()
    }
    pub(super) async fn close_agents(&self) -> Result<(), Error> {
        self.closed.store(true, Ordering::Release);
        let jobs = self
            .jobs
            .lock()
            .map_err(|_| Error::OutcomeUnknown)?
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for job in &jobs {
            if let Some(custody) = &job.factory {
                custody.closed.store(true, Ordering::Release);
            }
        }
        let mut failed = false;
        for job in jobs {
            if let Some(custody) = &job.factory {
                if custody.close().await.is_err() {
                    failed = true;
                }
                let account = job
                    .account
                    .lock()
                    .map(|owner| owner.as_ref().cloned())
                    .map_err(|_| Error::OutcomeUnknown);
                match account {
                    Ok(Some(account)) => {
                        if account.close_enrollment_sdk().await.is_err() {
                            failed = true;
                        }
                    }
                    Ok(None) => {}
                    Err(_) => failed = true,
                }
            }
        }
        // This admitted finite drain may already have closed other owners.
        // Individual exact results remain in their original custody; Busy is
        // not an honest aggregate pre-effect verdict after partial shutdown.
        if failed {
            Err(Error::OutcomeUnknown)
        } else {
            Ok(())
        }
    }
}
impl Collector {
    /// Bounded discovery of an actual successful original inline job. Unknown,
    /// unfinished, closed and already-taken jobs never create a replacement.
    pub fn take_next_provisioned_agent(&self) -> Result<Option<ProvisionedAgent>, Error> {
        self.inner
            .config
            .provisioning
            .as_ref()
            .ok_or(Error::Config)?
            .take_next_agent()
    }
    /// Called by the owning native Host after an actual inline verdict returned.
    /// A failed/lost factory result never yields an agent or replacement runtime.
    pub fn take_provisioned_agent(&self, engagement: &str) -> Result<ProvisionedAgent, Error> {
        self.inner
            .config
            .provisioning
            .as_ref()
            .ok_or(Error::Config)?
            .take_agent(engagement)
    }
    pub async fn close_provisioned_agents(&self) -> Result<(), Error> {
        let permit = self
            .inner
            .busy
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Busy)?;
        let inner = self.inner.clone();
        tokio::spawn(async move {
            let _permit = permit;
            if let Some(host) = &inner.config.provisioning {
                host.close_agents().await?;
            }
            Ok(())
        })
        .await
        .map_err(|_| Error::OutcomeUnknown)?
    }
}
