//! Original inline physical-account owner. Account observation is deliberately
//! not a full factory result: home/rooms/SDK/runtime and domain completion follow.
use crate::{
    CancellationToken, Error, HostConfig, Limits, ProvisionedTokenAccount, TokenAccountProvision,
    config::host_endpoint,
};
use hagency_core::{authority::Registration, canonical};
use hagency_store::{DomainStore, EffectOutcome};
use reqwest::Url;
use std::{
    collections::BTreeMap,
    path::{Component, PathBuf},
    sync::{Arc, Mutex},
};

const MAX_JOBS: usize = 16;
mod factory;
pub use factory::ProvisionedAgent;
struct Job {
    factory: Option<Arc<factory::Custody>>,
    home: Mutex<Option<Arc<hagency_store::agent_home::ManagedAgentHome>>>,
    account: Mutex<Option<Arc<ProvisionedTokenAccount>>>,
    result: Mutex<Option<Result<Arc<ProvisionedTokenAccount>, Error>>>,
}
/// Private process Host capability. No Deserialize/Debug/Clone, credential
/// getter, generic adapter callback or public observed-result setter.
pub struct TokenProvisioningHost {
    factory_rooms: Arc<tokio::sync::Mutex<()>>,
    closed: std::sync::atomic::AtomicBool,
    warm: Option<hagency_execution::WarmHostPlan>,
    factory_approvals: Option<Arc<crate::ApprovalCollector>>,
    as_namespace: Option<String>,
    homes: Option<hagency_store::agent_home::ManagedHomePlan>,
    rooms: Option<RoomPlan>,
    registration: Registration,
    fingerprint: String,
    endpoint: Url,
    token: String,
    state: PathBuf,
    key: [u8; 32],
    limits: Limits,
    roots: Vec<reqwest::Certificate>,
    jobs: Mutex<BTreeMap<String, Arc<Job>>>,
}
struct RoomPlan {
    representative: String,
    anchors: Vec<(String, String)>,
}
impl TokenProvisioningHost {
    pub fn new(
        registration: Registration,
        endpoint: &str,
        token: &str,
        state: PathBuf,
        key: [u8; 32],
        limits: Limits,
    ) -> Result<Self, Error> {
        Self::configured(registration, endpoint, token, state, key, limits, None)
    }
    pub fn application_service(
        registration: Registration,
        endpoint: &str,
        credential: crate::ApplicationServiceCredential,
        state: PathBuf,
        key: [u8; 32],
        limits: Limits,
    ) -> Result<Self, Error> {
        Self::configured(
            registration,
            endpoint,
            &credential.token,
            state,
            key,
            limits,
            Some(credential.namespace),
        )
    }
    fn configured(
        registration: Registration,
        endpoint: &str,
        token: &str,
        state: PathBuf,
        key: [u8; 32],
        limits: Limits,
        as_namespace: Option<String>,
    ) -> Result<Self, Error> {
        registration.validate().map_err(|_| Error::Config)?;
        limits.validate()?;
        if if let Some(namespace) = &as_namespace {
            namespace != &format!("{}_", registration.fleet_id)
                || !(16..=4096).contains(&token.len())
                || !token.bytes().all(|b| (33..=126).contains(&b))
        } else {
            token.is_empty()
                || token.len() > 64
                || !token
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'=' | b'_' | b'-'))
        } || !state.is_absolute()
            || state.as_os_str().as_encoded_bytes().len() > 4096
            || state
                .components()
                .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
        {
            return Err(Error::Config);
        }
        let fingerprint =
            canonical::digest(&serde_json::to_value(&registration).map_err(|_| Error::Config)?)
                .map_err(|_| Error::Config)?;
        Ok(Self {
            factory_rooms: Arc::new(tokio::sync::Mutex::new(())),
            closed: std::sync::atomic::AtomicBool::new(false),
            warm: None,
            factory_approvals: None,
            as_namespace,
            homes: None,
            rooms: None,
            registration,
            fingerprint,
            endpoint: host_endpoint(endpoint)?,
            token: token.to_owned(),
            state,
            key,
            limits,
            roots: vec![],
            jobs: Mutex::new(BTreeMap::new()),
        })
    }
    pub fn with_root_pem(mut self, pem: &[u8]) -> Result<Self, Error> {
        if pem.len() > 16384 || self.roots.len() >= 4 {
            return Err(Error::Config);
        }
        self.roots
            .push(reqwest::Certificate::from_pem(pem).map_err(|_| Error::Config)?);
        Ok(self)
    }
    /// Explicit private physical-rooms/SDK checkpoint, not the full factory.
    pub fn with_agent_rooms_enrollment(
        mut self,
        representative_token: &str,
        anchors: Vec<(String, String)>,
    ) -> Result<Self, Error> {
        if self.rooms.is_some()
            || !(16..=4096).contains(&representative_token.len())
            || !representative_token
                .bytes()
                .all(|b| (33..=126).contains(&b))
        {
            return Err(Error::Config);
        }
        // Validate peer material before the new account is known. Its actual
        // self-exclusion remains mandatory when that account config is built.
        crate::enrollment::state::Profile::new(
            anchors.clone(),
            "",
            &self.registration.server_name,
        )?;
        self.rooms = Some(RoomPlan {
            representative: representative_token.to_owned(),
            anchors,
        });
        Ok(self)
    }
    /// Original physical v1 homes, still not runtime fulfillment or Applied.
    pub fn with_managed_homes(
        mut self,
        plan: hagency_store::agent_home::ManagedHomePlan,
    ) -> Result<Self, Error> {
        if self.homes.is_some() || self.rooms.is_none() {
            return Err(Error::Config);
        }
        plan.separate_from(&self.state)?;
        self.homes = Some(plan);
        Ok(self)
    }
    pub(crate) fn bind(&self, config: &HostConfig) -> Result<(), Error> {
        if config.approval
            || config.endpoint != self.endpoint
            || config.identity.registration_fingerprint != self.fingerprint
            || config.identity.server_name != self.registration.server_name
            || config.identity.transport.registration_generation != self.registration.generation
            || config.reception_room.as_ref().is_none_or(|room| {
                room.room_id != self.registration.reception_room_id
                    || room.generation != self.registration.generation
            })
        {
            return Err(Error::Config);
        }
        Ok(())
    }
    pub(crate) fn room_coordinator(&self) -> Option<Arc<tokio::sync::Mutex<()>>> {
        self.warm.as_ref().map(|_| self.factory_rooms.clone())
    }
    /// Called only inside the Collector's accepted owned intake job, AFTER
    /// representative verification and canonical approve. No effect worker.
    /// After a restart: bring back one agent this service's inline factory
    /// already completed. Nothing is claimed, registered, created or activated;
    /// every step reads what the original provision left and refuses when it is
    /// missing or changed. A failure here concerns this agent only and leaves
    /// its durable state exactly as it was, except that genuine negative Matrix
    /// evidence still fences as everywhere else.
    pub(crate) async fn reattach_completed(
        &self,
        domain: &DomainStore,
        engagement: &str,
        cancel: &CancellationToken,
    ) -> Result<(), Error> {
        if self.closed.load(std::sync::atomic::Ordering::Acquire) {
            return Err(Error::Generation);
        }
        if self.warm.is_none() || self.homes.is_none() || self.rooms.is_none() {
            return Err(Error::Config);
        }
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let (effect, registration, scope) = domain
            .reattach_provision_scope(engagement.to_owned())
            .await?;
        if canonical::digest(&serde_json::to_value(&registration).map_err(|_| Error::Config)?)
            .map_err(|_| Error::Config)?
            != self.fingerprint
        {
            return Err(Error::Generation);
        }
        let job = {
            let mut jobs = self.jobs.lock().map_err(|_| Error::OutcomeUnknown)?;
            if jobs.contains_key(&effect.id) {
                return Err(Error::Busy);
            }
            if jobs.len() >= MAX_JOBS {
                return Err(Error::Capacity);
            }
            let job = Arc::new(Job {
                factory: Some(Arc::new(factory::Custody::new())),
                home: Mutex::new(None),
                account: Mutex::new(None),
                result: Mutex::new(None),
            });
            jobs.insert(effect.id.clone(), job.clone());
            job
        };
        let result = async {
            let home =
                self.homes
                    .as_ref()
                    .ok_or(Error::Config)?
                    .reopen(&scope, &effect, &registration)?;
            *job.home.lock().map_err(|_| Error::OutcomeUnknown)? = Some(home);
            let mut operation = if let Some(namespace) = &self.as_namespace {
                TokenAccountProvision::application_service(
                    &registration,
                    &effect,
                    self.endpoint.as_str(),
                    crate::ApplicationServiceCredential::new(&self.token, namespace)?,
                    self.state.clone(),
                    self.key,
                    self.limits.clone(),
                )
            } else {
                TokenAccountProvision::new(
                    &registration,
                    &effect,
                    self.endpoint.as_str(),
                    &self.token,
                    self.state.clone(),
                    self.key,
                    self.limits.clone(),
                )
            }?
            .for_reattach()
            .with_domain(domain.clone(), effect.clone(), registration.clone());
            operation.factory_rooms = Some(self.factory_rooms.clone());
            operation.roots = self.roots.clone();
            let account = Arc::new(operation.execute(cancel).await?);
            *job.account.lock().map_err(|_| Error::OutcomeUnknown)? = Some(account.clone());
            self.reattach_factory(domain, &effect, scope, &account, &job, cancel)
                .await?;
            Ok(account)
        }
        .await;
        if result.is_err()
            && let Some(custody) = &job.factory
        {
            custody.cancel().await;
        }
        let response = result.as_ref().map(|_| ()).map_err(Clone::clone);
        *job.result.lock().map_err(|_| Error::OutcomeUnknown)? = Some(result);
        response
    }
    pub(crate) async fn account(
        &self,
        domain: &DomainStore,
        registration: &Registration,
        engagement: &str,
        cancel: &CancellationToken,
    ) -> Result<(), Error> {
        if self.closed.load(std::sync::atomic::Ordering::Acquire) {
            return Err(Error::Generation);
        }
        if canonical::digest(&serde_json::to_value(registration).map_err(|_| Error::Config)?)
            .map_err(|_| Error::Config)?
            != self.fingerprint
        {
            return Err(Error::Generation);
        }
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let id = format!("provision_{engagement}");
        let job = {
            let mut jobs = self.jobs.lock().map_err(|_| Error::OutcomeUnknown)?;
            if let Some(job) = jobs.get(&id) {
                // Historical step acknowledgement only; it cannot establish
                // current SDK/room/runner authority for the remaining factory.
                return match job
                    .result
                    .lock()
                    .map_err(|_| Error::OutcomeUnknown)?
                    .as_ref()
                {
                    Some(Ok(_)) => Ok(()),
                    Some(Err(error)) => Err(error.clone()),
                    None => Err(Error::OutcomeUnknown),
                };
            }
            if jobs.len() >= MAX_JOBS {
                return Err(Error::Capacity);
            }
            let job = Arc::new(Job {
                factory: self
                    .warm
                    .as_ref()
                    .map(|_| Arc::new(factory::Custody::new())),
                home: Mutex::new(None),
                account: Mutex::new(None),
                result: Mutex::new(None),
            });
            jobs.insert(id.clone(), job.clone());
            job
        };
        // Burn the admitted local claim attempt even when its acknowledgement
        // is lost. A restarted Host also cannot claim Started/Uncertain again.
        let claimed = domain.claim_effect_for(id).await;
        let result = match claimed {
            Ok(Some(effect)) => {
                let mut activated = false;
                let mut result = async {
                    domain.validate_provision_account(effect.clone(), registration.clone()).await?;
                    if let Some(plan)=&self.homes {
                        let cancelled=Arc::new(std::sync::atomic::AtomicBool::new(cancel.is_cancelled()));
                        let home=plan.materialize(domain.clone(),effect.clone(),registration.clone(),
                            std::time::Instant::now()+self.limits.sdk,cancelled.clone());
                        tokio::pin!(home);
                        let home=tokio::select! {
                            result=&mut home=>result,
                            _=cancel.cancelled()=>{cancelled.store(true,std::sync::atomic::Ordering::Release);home.await},
                        };
                        *job.home.lock().map_err(|_|Error::OutcomeUnknown)?=Some(home?);
                        domain.validate_provision_account(effect.clone(),registration.clone()).await?;
                    }
                    let mut operation=if let Some(namespace)=&self.as_namespace {
                        TokenAccountProvision::application_service(registration,&effect,self.endpoint.as_str(),
                            crate::ApplicationServiceCredential::new(&self.token,namespace)?,self.state.clone(),self.key,self.limits.clone())
                    } else {
                        TokenAccountProvision::new(registration,&effect,self.endpoint.as_str(),&self.token,self.state.clone(),self.key,self.limits.clone())
                    }?.with_domain(domain.clone(), effect.clone(), registration.clone());
                    if self.warm.is_some() {operation.factory_rooms=Some(self.factory_rooms.clone());}
                    operation.roots = self.roots.clone();
                    let account = Arc::new(operation.execute(cancel).await?);
                    *job.account.lock().map_err(|_| Error::OutcomeUnknown)? = Some(account.clone());
                    if let Some(plan) = &self.rooms {
                        account.create_agent_rooms(&plan.representative, cancel).await?;
                        account.enroll_created_rooms(1,self.key,plan.anchors.clone(),cancel).await?;
                    }
                    if self.warm.is_some() {self.finish_factory(domain,&effect,&account,&job,cancel,&mut activated).await?;}
                    Ok(account)
                }.await;
                if activated && let Err(error) = result {
                    result = Err(match &job.factory {
                        Some(custody) => custody.fence_failure(error).await,
                        None => error,
                    });
                }
                if result.is_err()
                    && let Some(custody) = &job.factory
                {
                    custody.cancel().await;
                }
                if result.is_err()
                    && !activated
                    && domain
                        .observe_effect(effect.id, effect.fence, EffectOutcome::Unknown)
                        .await
                        .is_err()
                {
                    Err(Error::OutcomeUnknown)
                } else {
                    result
                }
            }
            Ok(None) => Err(Error::OutcomeUnknown),
            Err(error) => Err(error.into()),
        };
        let response = result.as_ref().map(|_| ()).map_err(Clone::clone);
        // Keep the opaque observed account for the remaining physical stages.
        // Checkpoint-only jobs do not submit Applied; concrete factory jobs
        // retain the original runtime/SDK and their acknowledged derived route.
        *job.result.lock().map_err(|_| Error::OutcomeUnknown)? = Some(result);
        response
    }
    #[cfg(test)]
    pub(crate) fn application_service_profile(&self) -> bool {
        self.as_namespace.is_some()
    }
    #[cfg(test)]
    pub(crate) fn observed_home_handle(
        &self,
        engagement: &str,
    ) -> Option<Arc<hagency_store::agent_home::ManagedAgentHome>> {
        let jobs = self.jobs.lock().unwrap();
        jobs.get(&format!("provision_{engagement}"))?
            .home
            .lock()
            .unwrap()
            .as_ref()
            .cloned()
    }
    #[cfg(test)]
    pub(crate) fn observed_account(&self, engagement: &str) -> Option<(String, String)> {
        let jobs = self.jobs.lock().unwrap();
        let result = jobs
            .get(&format!("provision_{engagement}"))?
            .result
            .lock()
            .unwrap();
        let account = result.as_ref()?.as_ref().ok()?;
        Some((
            account.sender_mxid().to_owned(),
            account.device_id().to_owned(),
        ))
    }
    #[cfg(test)]
    pub(crate) fn observed_account_handle(&self, engagement: &str) -> Arc<ProvisionedTokenAccount> {
        let jobs = self.jobs.lock().unwrap();
        jobs.get(&format!("provision_{engagement}"))
            .unwrap()
            .account
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .clone()
    }
}
