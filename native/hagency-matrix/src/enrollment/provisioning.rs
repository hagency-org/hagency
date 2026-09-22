//! Original Started/Reserved factory scope, never an Active transport bypass.
use super::{Scope as EnrollmentScope, checkpoint};
use crate::{CancellationToken, Collector, Error, HostConfig, collector::Inner};
use hagency_core::{
    authority::{ProjectRequest, Registration},
    canonical, project,
    replies::RoomPrivacy,
};
use hagency_store::{DomainStore, Effect, EffectOutcome};
use serde_json::json;
use std::{
    collections::BTreeSet,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::time::{Instant, timeout_at};

pub(crate) struct Scope {
    effect: Effect,
    registration: Registration,
    request: ProjectRequest,
}
impl Scope {
    pub(crate) fn new(
        effect: Effect,
        registration: Registration,
        config: &HostConfig,
    ) -> Result<Self, Error> {
        let request: ProjectRequest =
            serde_json::from_value(effect.payload.get("request").ok_or(Error::Config)?.clone())
                .map_err(|_| Error::Config)?;
        request.validate(&registration).map_err(|_| Error::Config)?;
        let fingerprint =
            canonical::digest(&serde_json::to_value(&registration).map_err(|_| Error::Config)?)
                .map_err(|_| Error::Config)?;
        if request.engagement_id().map_err(|_| Error::Config)? != effect.engagement_id
            || config.approval
            || config.reception_room.is_some()
            || config.provisioning.is_some()
            || config.identity.transport.engagement_id != effect.engagement_id
            || config.identity.registration_fingerprint != fingerprint
            || config.identity.transport.registration_generation != registration.generation
            || config.identity.server_name != registration.server_name
            || config.identity.transport.sender_mxid
                != format!(
                    "@{}_{}:{}",
                    registration.fleet_id, effect.engagement_id, registration.server_name
                )
            || config.identity.transport.device_id != format!("DEVICE_{}", effect.engagement_id)
            || config.rooms.len() != 2
        {
            return Err(Error::Config);
        }
        let mut group = false;
        let mut direct = false;
        for room in &config.rooms {
            match &room.privacy {
                RoomPrivacy::Group {} if room.room_id == request.target_room_id => group = true,
                RoomPrivacy::Direct { human_mxid }
                    if human_mxid == &request.owner_mxid
                        && room.room_id != request.target_room_id
                        && room.room_id != request.owner_dm_room_id
                        && room.room_id != registration.reception_room_id =>
                {
                    direct = true
                }
                _ => return Err(Error::Config),
            }
        }
        if !group || !direct {
            return Err(Error::Config);
        }
        Ok(Self {
            effect,
            registration,
            request,
        })
    }
    pub(crate) async fn current(
        &self,
        inner: &Inner,
        cancel: &CancellationToken,
    ) -> Result<Vec<String>, Error> {
        self.current_at(inner, cancel, false).await
    }
    async fn validate(&self, domain: &DomainStore, active: bool) -> Result<(), Error> {
        if active {
            domain
                .validate_active_provision_account(self.effect.clone(), self.registration.clone())
                .await?;
        } else {
            domain
                .validate_provision_account(self.effect.clone(), self.registration.clone())
                .await?;
        }
        Ok(())
    }
    async fn current_at(
        &self,
        inner: &Inner,
        cancel: &CancellationToken,
        active: bool,
    ) -> Result<Vec<String>, Error> {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        self.validate(&inner.domain, active).await?;
        inner.whoami(cancel).await?;
        let sender = &inner.config.identity.transport.sender_mxid;
        let mut users = BTreeSet::from([sender.clone()]);
        for target in &inner.config.rooms {
            let value = inner
                .http
                .request(
                    &["_matrix", "client", "v3", "rooms", &target.room_id, "state"],
                    None,
                    cancel,
                )
                .await?
                .success()?;
            let (room, facts) = inner.room(target, value)?;
            if !room.joined.contains(sender) || !room.joined.contains(&self.request.owner_mxid) {
                return Err(Error::Recipients);
            }
            match &target.privacy {
                RoomPrivacy::Direct { .. }
                    if !room.invite_only || !room.encrypted || room.joined.len() != 2 =>
                {
                    return Err(Error::Recipients);
                }
                RoomPrivacy::Group {} => {
                    let binding = facts.binding.as_ref().ok_or(Error::Recipients)?;
                    let power = facts
                        .powers
                        .get(&self.request.owner_mxid)
                        .copied()
                        .unwrap_or(facts.default_power);
                    if binding["v"] != 1
                        || binding["purpose"] != "project"
                        || binding["authVersion"] != 1
                        || binding["fleetId"] != self.request.fleet_id
                        || binding["projectId"] != self.request.target_project_id
                        || binding["ownerMxid"] != self.request.owner_mxid
                        || power < facts.invite_power
                    {
                        return Err(Error::Recipients);
                    }
                }
                _ => {}
            }
            if room.encrypted {
                users.extend(room.joined);
            }
            if users.len() > 17 {
                return Err(Error::Capacity);
            }
        }
        let users = users.into_iter().collect::<Vec<_>>();
        inner
            .config
            .enrollment
            .as_ref()
            .ok_or(Error::Config)?
            .users(sender, &users)?;
        // This read follows the last actual HTTP await, so revoke/rotation while
        // a room GET was held cannot authorize the original subsequent POST.
        if let Some(guard) = &inner.config.as_guard {
            guard.check(cancel).await?;
        }
        self.validate(&inner.domain, active).await?;
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        Ok(users)
    }
}

#[derive(Default)]
pub(crate) struct Jobs(Mutex<Option<Arc<Job>>>);
struct Job {
    profile: String,
    scope: Arc<Scope>,
    collector: Collector,
    result: Mutex<Option<Result<(), Error>>>,
    closed: AtomicBool,
    closure: Mutex<Option<Result<(), Error>>>,
    activation: Mutex<Activation>,
}
enum Activation {
    Waiting,
    Running,
    Ready,
    Failed(Error),
}
impl Activation {
    fn before_activation(&self) -> Result<(), Error> {
        match self {
            Self::Waiting => Ok(()),
            Self::Running => Err(Error::Busy),
            Self::Ready => Err(Error::Generation),
            Self::Failed(error) => Err(error.clone()),
        }
    }
}
/// A failed final Active read must fence a positive collect already committed
/// by this job. CAS never revives or retires another owner.
async fn fence_failed_activation(inner: &Inner, result: Result<(), Error>) -> Result<(), Error> {
    match result {
        Ok(()) => Ok(()),
        Err(error) => {
            match inner
                .domain
                .matrix_transport_state(inner.config.identity.transport.engagement_id.clone())
                .await
            {
                Ok(Some(state))
                    if state.available && state.observation == inner.config.identity.transport =>
                {
                    inner.fence_observation(state.observation, error).await
                }
                Ok(_)
                | Err(hagency_store::Error::RunnerAuthority | hagency_store::Error::NotFound) => {
                    Err(error)
                }
                Err(_) => Err(Error::OutcomeUnknown),
            }
        }
    }
}
impl Jobs {
    pub(crate) fn admitted(&self) -> Result<bool, Error> {
        Ok(self.0.lock().map_err(|_| Error::OutcomeUnknown)?.is_some())
    }
    pub(crate) async fn run(
        &self,
        config: HostConfig,
        domain: DomainStore,
        scope: Scope,
        cancel: &CancellationToken,
    ) -> Result<Collector, Error> {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let profile = canonical::transport_digest(&json!({
            "binding":config.binding()?, "identity":config.identity, "rooms":config.rooms,
            "enrollment":config.enrollment, "key":project::hash(&config.key),
            "provision":[scope.effect,scope.registration],
        }))
        .map_err(|_| Error::Config)?;
        let job = {
            let mut guard = self.0.lock().map_err(|_| Error::OutcomeUnknown)?;
            if let Some(job) = guard.as_ref() {
                if job.profile != profile {
                    return Err(Error::Conflict);
                }
                if job.closed.load(Ordering::Acquire) {
                    return Err(Error::Storage);
                }
                job.activation
                    .lock()
                    .map_err(|_| Error::OutcomeUnknown)?
                    .before_activation()?;
                match job
                    .result
                    .lock()
                    .map_err(|_| Error::OutcomeUnknown)?
                    .as_ref()
                {
                    Some(Ok(())) => {}
                    Some(Err(error)) => return Err(error.clone()),
                    None => return Err(Error::Busy),
                }
                job.clone()
            } else {
                let job = Arc::new(Job {
                    profile,
                    scope: Arc::new(scope),
                    collector: Collector::new(config, domain)?,
                    result: Mutex::new(None),
                    closed: AtomicBool::new(false),
                    closure: Mutex::new(None),
                    activation: Mutex::new(Activation::Waiting),
                });
                *guard = Some(job.clone());
                job
            }
        };
        let permit = job
            .collector
            .inner
            .busy
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Busy)?;
        if job.closed.load(Ordering::Acquire) {
            return Err(Error::Storage);
        }
        job.activation
            .lock()
            .map_err(|_| Error::OutcomeUnknown)?
            .before_activation()?;
        let deadline = Instant::now() + job.collector.inner.config.limits.sdk;
        let cancel = cancel.clone();
        let operation = job.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let inner = &operation.collector.inner;
            let scope = &operation.scope;
            let result = timeout_at(
                deadline,
                inner.enroll(EnrollmentScope::Provision(scope), &cancel, deadline),
            )
            .await
            .unwrap_or(Err(Error::Timeout));
            let result = match result {
                Ok(()) => checkpoint(&cancel, deadline),
                Err(error) => Err(error),
            };
            // Enrollment does not complete the factory. Any failed original
            // physical step is retained as unknown, never NotApplied/Active.
            let result = if result.is_err()
                && inner
                    .domain
                    .observe_effect(
                        scope.effect.id.clone(),
                        scope.effect.fence,
                        EffectOutcome::Unknown,
                    )
                    .await
                    .is_err()
            {
                Err(Error::OutcomeUnknown)
            } else {
                result
            };
            if let Ok(mut slot) = operation.result.lock() {
                *slot = Some(result.clone());
            }
            result
        })
        .await
        .map_err(|_| Error::OutcomeUnknown)??;
        Ok(Collector {
            inner: job.collector.inner.clone(),
        })
    }
    /// A restart re-attaching an account whose enrollment this custody already
    /// completed. The existing SDK store is opened, never bootstrapped, and an
    /// incomplete enrollment ledger is refused rather than enrolled.
    pub(crate) async fn reattach_enrolled(
        &self,
        config: HostConfig,
        domain: DomainStore,
        scope: Scope,
        cancel: &CancellationToken,
    ) -> Result<Collector, Error> {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let profile = canonical::transport_digest(&json!({
            "binding":config.binding()?, "identity":config.identity, "rooms":config.rooms,
            "enrollment":config.enrollment, "key":project::hash(&config.key),
            "provision":[scope.effect,scope.registration],
        }))
        .map_err(|_| Error::Config)?;
        let job = {
            let mut guard = self.0.lock().map_err(|_| Error::OutcomeUnknown)?;
            if guard.is_some() {
                return Err(Error::Busy);
            }
            let job = Arc::new(Job {
                profile,
                scope: Arc::new(scope),
                collector: Collector::new(config, domain)?,
                result: Mutex::new(None),
                closed: AtomicBool::new(false),
                closure: Mutex::new(None),
                activation: Mutex::new(Activation::Running),
            });
            *guard = Some(job.clone());
            job
        };
        let permit = job
            .collector
            .inner
            .busy
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Busy)?;
        let deadline = Instant::now() + job.collector.inner.config.limits.sdk;
        let cancel = cancel.clone();
        let operation = job.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let inner = &operation.collector.inner;
            let result = timeout_at(deadline, async {
                let users = operation.scope.current_at(inner, &cancel, true).await?;
                checkpoint(&cancel, deadline)?;
                {
                    let mut owner = inner.owner.lock().await;
                    if owner.is_none() {
                        *owner = Some(crate::sdk::Owner::open_existing(&inner.config).await?);
                    }
                    if !matches!(
                        owner
                            .as_ref()
                            .ok_or(Error::Storage)?
                            .enrollment_handle_for(crate::sdk::enrollment::Purpose::Agent)
                            .command(crate::sdk::enrollment::Command::Status)
                            .await?,
                        super::state::View::Complete
                    ) {
                        return Err(Error::Storage);
                    }
                }
                checkpoint(&cancel, deadline)?;
                inner.collect(&cancel).await?;
                inner
                    .enroll(EnrollmentScope::Agent, &cancel, deadline)
                    .await?;
                if operation.scope.current_at(inner, &cancel, true).await? != users {
                    return Err(Error::Recipients);
                }
                checkpoint(&cancel, deadline)
            })
            .await
            .unwrap_or(Err(Error::Timeout));
            let result = fence_failed_activation(inner, result).await;
            if let Ok(mut slot) = operation.result.lock() {
                *slot = Some(result.clone());
            }
            let mut phase = operation
                .activation
                .lock()
                .map_err(|_| Error::OutcomeUnknown)?;
            *phase = match &result {
                Ok(()) => Activation::Ready,
                Err(error) => Activation::Failed(error.clone()),
            };
            result
        })
        .await
        .map_err(|_| Error::OutcomeUnknown)??;
        Ok(Collector {
            inner: job.collector.inner.clone(),
        })
    }
    /// The one enrolled owner moves forward only after genuine current Active
    /// evidence. A lost waiter cannot discard its job/permit/negative result.
    pub(crate) async fn active(&self, cancel: &CancellationToken) -> Result<Collector, Error> {
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let job = self
            .0
            .lock()
            .map_err(|_| Error::OutcomeUnknown)?
            .clone()
            .ok_or(Error::Config)?;
        match job
            .result
            .lock()
            .map_err(|_| Error::OutcomeUnknown)?
            .as_ref()
        {
            Some(Ok(())) => {}
            Some(Err(error)) => return Err(error.clone()),
            None => return Err(Error::Busy),
        }
        let permit = job
            .collector
            .inner
            .busy
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Busy)?;
        if job.closed.load(Ordering::Acquire) {
            return Err(Error::Storage);
        }
        {
            let mut phase = job.activation.lock().map_err(|_| Error::OutcomeUnknown)?;
            match &*phase {
                Activation::Waiting | Activation::Ready => {}
                Activation::Running => return Err(Error::Busy),
                Activation::Failed(error) => return Err(error.clone()),
            }
            *phase = Activation::Running;
        }
        let deadline = Instant::now() + job.collector.inner.config.limits.sdk;
        let cancel = cancel.clone();
        let operation = job.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let inner = &operation.collector.inner;
            let result = timeout_at(deadline, async {
                let users = operation.scope.current_at(inner, &cancel, true).await?;
                checkpoint(&cancel, deadline)?;
                {
                    // Never open even an existing replacement owner. Closure
                    // also needs this busy permit, so this exact owner remains.
                    let owner = inner.owner.lock().await;
                    let owner = owner.as_ref().ok_or(Error::Storage)?;
                    if !matches!(
                        owner
                            .enrollment_handle_for(crate::sdk::enrollment::Purpose::Agent)
                            .command(crate::sdk::enrollment::Command::Status)
                            .await?,
                        super::state::View::Complete
                    ) {
                        return Err(Error::Storage);
                    }
                }
                checkpoint(&cancel, deadline)?;
                inner.collect(&cancel).await?;
                inner
                    .enroll(EnrollmentScope::Agent, &cancel, deadline)
                    .await?;
                if operation.scope.current_at(inner, &cancel, true).await? != users {
                    return Err(Error::Recipients);
                }
                checkpoint(&cancel, deadline)
            })
            .await
            .unwrap_or(Err(Error::Timeout));
            let result = fence_failed_activation(inner, result).await;
            let mut phase = operation
                .activation
                .lock()
                .map_err(|_| Error::OutcomeUnknown)?;
            *phase = match &result {
                Ok(()) => Activation::Ready,
                Err(error) => Activation::Failed(error.clone()),
            };
            result
        })
        .await
        .map_err(|_| Error::OutcomeUnknown)??;
        Ok(Collector {
            inner: job.collector.inner.clone(),
        })
    }
    /// SDK-custody closure only, not a Domain transport/route cleanup receipt.
    /// Closing does not clear the retained failed job or permit a new operation.
    pub(crate) async fn close_sdk(&self) -> Result<(), Error> {
        let job = self.0.lock().map_err(|_| Error::OutcomeUnknown)?.clone();
        let Some(job) = job else {
            return Ok(());
        };
        if job.closed.load(Ordering::Acquire) {
            return job
                .closure
                .lock()
                .map_err(|_| Error::OutcomeUnknown)?
                .as_ref()
                .cloned()
                .unwrap_or(Err(Error::OutcomeUnknown));
        }
        let permit = job
            .collector
            .inner
            .busy
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Busy)?;
        if job.closed.swap(true, Ordering::AcqRel) {
            return Err(Error::OutcomeUnknown);
        }
        tokio::spawn(async move {
            let _permit = permit;
            let result = if let Some(owner) = job.collector.inner.owner.lock().await.take() {
                owner.close().await
            } else {
                Ok(())
            };
            if let Ok(mut slot) = job.closure.lock() {
                *slot = Some(result.clone());
            }
            result
        })
        .await
        .map_err(|_| Error::OutcomeUnknown)?
    }
    #[cfg(test)]
    pub(crate) fn observed_active(&self) -> Option<Result<(), Error>> {
        let job = self.0.lock().unwrap().clone()?;
        let phase = job.activation.lock().unwrap();
        match &*phase {
            Activation::Ready => Some(Ok(())),
            Activation::Failed(error) => Some(Err(error.clone())),
            _ => None,
        }
    }
    #[cfg(test)]
    pub(crate) fn observed(&self) -> Option<Result<Collector, Error>> {
        let job = self.0.lock().unwrap().clone()?;
        let result = job.result.lock().unwrap().clone()?;
        Some(result.map(|_| Collector {
            inner: job.collector.inner.clone(),
        }))
    }
}
