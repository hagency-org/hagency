//! Concrete inline Host bridge to the existing owned launcher, not a launcher.
use crate::{ApprovalHost, Failure, Host, Limits, Operation, WarmLimits, WarmRuntime};
use cap_std::{ambient_authority, fs::Dir};
use hagency_store::{
    DomainStore, OwnedClaimProfile, OwnedProvisionScope, agent_home::ManagedAgentHome,
    task_context::RetainedTaskContext,
};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    net::SocketAddr,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::time::Instant;

/// Fixed process Host capability. No runtime configuration/proof callback,
/// serialization, cloning, private-account getter or caller-supplied readiness.
pub struct WarmHostPlan {
    guardian: PathBuf,
    executable: PathBuf,
    environment: BTreeMap<OsString, OsString>,
    bridge: WarmTaskBridge,
    approvals: ApprovalHost,
    limits: WarmLimits,
    files: Option<(usize, bool, bool)>,
    coordination: bool,
    local_codex: Option<Arc<crate::LocalCodex>>,
}
/// The fixed native task helper, loopback origin and retained private context
/// root form one Host capability. No getters, cloning or serialization.
pub struct WarmTaskBridge {
    helper: PathBuf,
    address: SocketAddr,
    contexts: PathBuf,
    context_dir: Dir,
}
/// The actual original per-agent Host and one consumable initialized owner.
/// Drop/close has the same synchronous ownership-worker requirement as WarmRuntime.
pub struct FactoryRuntime {
    host: crate::SharedHost,
    domain: DomainStore,
    warm: Option<WarmRuntime>,
    phase: Phase,
    last: Option<String>,
    cancelled: AtomicBool,
}
enum Phase {
    Initial,
    /// A restart re-attached this agent: there is no warm child, and the first
    /// dispatch is already a follow-up from this rebuilt binding.
    Reattached(Box<crate::warm::Binding>),
    Queued(Arc<Admission>),
    Running(Arc<crate::operation::Continuation>),
    Spent,
}
struct Admission {
    capability: hagency_core::tasks::RunnerCapability,
    limits: Limits,
    binding: Option<crate::warm::Binding>,
}
/// Non-cloneable original admission. It carries no native process/worker, so
/// dropping a queued caller never joins a process on that async caller.
pub struct FactoryDispatch(Arc<Admission>);
impl WarmTaskBridge {
    pub fn new(helper: PathBuf, address: SocketAddr, contexts: PathBuf) -> Result<Self, Failure> {
        if !helper.is_absolute()
            || !helper.is_file()
            || !address.ip().is_loopback()
            || address.port() == 0
            || matches!(address,SocketAddr::V6(value) if value.scope_id()!=0 || value.flowinfo()!=0)
        {
            return Err(Failure::Admission);
        }
        hagency_store::task_context::validate_root(&contexts).map_err(|_| Failure::Admission)?;
        let context_dir = Dir::open_ambient_dir(&contexts, ambient_authority())
            .map_err(|_| Failure::Admission)?;
        Ok(Self {
            helper,
            address,
            contexts,
            context_dir,
        })
    }
}
impl WarmHostPlan {
    pub fn new(
        guardian: PathBuf,
        executable: PathBuf,
        environment: BTreeMap<OsString, OsString>,
        bridge: WarmTaskBridge,
        approvals: ApprovalHost,
        limits: WarmLimits,
    ) -> Result<Self, Failure> {
        if !limits.validate()
            || [&guardian, &executable]
                .iter()
                .any(|path| !path.is_absolute() || !path.is_file())
        {
            return Err(Failure::Admission);
        }
        Ok(Self {
            guardian,
            executable,
            environment,
            bridge,
            approvals,
            limits,
            files: None,
            coordination: false,
            local_codex: None,
        })
    }
    /// Pass the original Host's provider-owned binding without reopening it or
    /// exporting a credential/path capability. Per-agent task roots stay local.
    pub fn with_local_codex_from_host(mut self, host: &Host) -> Result<Self, Failure> {
        if self.local_codex.is_some() {
            return Err(Failure::Admission);
        }
        let local = host.local_codex.as_ref().ok_or(Failure::Admission)?;
        local.apply(&mut self.environment)?;
        self.local_codex = Some(local.clone());
        Ok(self)
    }
    /// Fixed presentation/capture limits, shared with the owning service's
    /// per-agent file backends. This grants no workspace or SDK authority.
    pub fn with_file_access(
        mut self,
        limit: usize,
        send: bool,
        receive: bool,
    ) -> Result<Self, Failure> {
        if self.files.is_some()
            || limit == 0
            || limit > hagency_core::file_delivery::MAX_FILE_BYTES as usize
            || (receive && hagency_core::received_files::receive_limit(limit).is_err())
        {
            return Err(Failure::Admission);
        }
        self.files = Some((limit, send, receive));
        Ok(self)
    }
    /// Presentation only (ADR180): every agent host may call coordination tools.
    pub fn with_coordination_tools(mut self) -> Self {
        self.coordination = true;
        self
    }
    /// The runtime of an agent a restart re-attached, from a scope and a home
    /// the store rebuilt and reopened. It starts no child and reads no retained
    /// task context: that file belongs to the original first task. The same
    /// per-agent Host is built as in `start`, its provider-managed account
    /// included (the reopened registry's own binding, gated on the same
    /// current facts), and the agent's next task launches through the
    /// ordinary follow-up binding. One runtime per scope per process.
    pub async fn reattach_runtime(
        &self,
        domain: DomainStore,
        scope: OwnedProvisionScope,
        home: Arc<ManagedAgentHome>,
    ) -> Result<FactoryRuntime, Failure> {
        let until = Instant::now() + Duration::from_millis(self.limits.initialize.operation_ms);
        let account =
            tokio::time::timeout_at(until, domain.reattach_runtime_account(scope.clone()))
                .await
                .map_err(|_| Failure::Deadline)?
                .map_err(|error| Failure::lost(crate::AuthoritySite::FactoryAccount, &error))?;
        let guardian = self.guardian.clone();
        let executable = self.executable.clone();
        let environment = self.environment.clone();
        let helper = self.bridge.helper.clone();
        let address = self.bridge.address;
        let approvals = self.approvals.clone();
        let files = self.files;
        let coordination = self.coordination;
        let local_codex = self.local_codex.clone();
        tokio::task::spawn_blocking(move || {
            if Instant::now() >= until {
                return Err(Failure::Deadline);
            }
            scope.claim_warm().map_err(|_| Failure::Admission)?;
            let workspace = format!("work_{}", scope.engagement_id());
            let work = home.workdir_path().map_err(|_| Failure::Admission)?;
            let mut environment = environment;
            if let Some(local) = &local_codex {
                // Explicit provider selection, never arbitrary coordinator HOME.
                if account.is_some() {
                    return Err(Failure::Admission);
                }
                local.admit_provision(&scope)?;
                local.separate_from(&work)?;
                local.apply(&mut environment)?;
            } else {
                let agent_home = home.home_path().map_err(|_| Failure::Admission)?;
                environment.insert("HOME".into(), agent_home.clone().into_os_string());
                environment.insert("CODEX_HOME".into(), agent_home.into_os_string());
            }
            let mut host = Host::new(
                guardian,
                executable,
                environment,
                BTreeMap::from([(workspace.clone(), work)]),
            )?
            .with_task_helper(helper, address)?
            .with_approvals(approvals)?;
            if let Some((limit, send, receive)) = files {
                host = host.with_file_limit(limit)?;
                if send {
                    host = host.with_file_tools()?;
                }
                if receive {
                    host = host.with_receive_tools()?;
                }
            }
            if coordination {
                host = host.with_coordination_tools()?;
            }
            if let Some(account) = account {
                host = host.with_managed_account(account)?;
            }
            if let Some(local) = local_codex.clone() {
                host = host.with_retained_local_codex(local)?;
            }
            let root = host.reattach_root(&scope, &home, &workspace)?;
            let binding =
                crate::warm::Binding::reattached(scope, home, root, workspace, local_codex);
            Ok(FactoryRuntime {
                host: host.into_shared(),
                domain,
                warm: None,
                phase: Phase::Reattached(Box::new(binding)),
                last: None,
                cancelled: AtomicBool::new(false),
            })
        })
        .await
        .map_err(|_| Failure::Worker)?
    }
    pub async fn start(
        &self,
        domain: DomainStore,
        scope: OwnedProvisionScope,
        home: Arc<ManagedAgentHome>,
    ) -> Result<FactoryRuntime, Failure> {
        // The original deadline includes the original writer and blocking Host
        // preparation. start_at never grants a fresh initialization interval.
        let until = Instant::now() + Duration::from_millis(self.limits.initialize.operation_ms);
        let account =
            tokio::time::timeout_at(until, domain.provision_runtime_account(scope.clone()))
                .await
                .map_err(|_| Failure::Deadline)?
                .map_err(|error| Failure::lost(crate::AuthoritySite::FactoryAccount, &error))?;
        let held = self
            .bridge
            .context_dir
            .try_clone()
            .map_err(|_| Failure::Admission)?
            .into_std_file();
        let guardian = self.guardian.clone();
        let executable = self.executable.clone();
        let environment = self.environment.clone();
        let helper = self.bridge.helper.clone();
        let address = self.bridge.address;
        let contexts = self.bridge.contexts.clone();
        let approvals = self.approvals.clone();
        let limits = self.limits;
        let files = self.files;
        let coordination = self.coordination;
        let local_codex = self.local_codex.clone();
        // A lost caller does not discard a possible owner on an HTTP worker:
        // this retained blocking task owns the returned WarmRuntime until handoff.
        tokio::task::spawn_blocking(move || {
            if Instant::now() >= until {
                return Err(Failure::Deadline);
            }
            let current = Dir::open_ambient_dir(&contexts, ambient_authority())
                .map_err(|_| Failure::Admission)?
                .into_std_file();
            if !hagency_platform::same_directory(&held, &current).map_err(|_| Failure::Admission)? {
                return Err(Failure::Admission);
            }
            let workspace = format!("work_{}", scope.engagement_id());
            let context_id = hagency_core::canonical::transport_digest(&serde_json::json!([
                "inline_factory",
                scope.engagement_id()
            ]))
            .map_err(|_| Failure::Admission)?;
            let context =
                RetainedTaskContext::new(contexts, &context_id).map_err(|_| Failure::Admission)?;
            let work = home.workdir_path().map_err(|_| Failure::Admission)?;
            let mut environment = environment;
            if let Some(local) = &local_codex {
                // Explicit provider selection, never arbitrary coordinator HOME.
                if account.is_some() {
                    return Err(Failure::Admission);
                }
                local.admit_provision(&scope)?;
                local.separate_from(&work)?;
                local.apply(&mut environment)?;
            } else {
                let agent_home = home.home_path().map_err(|_| Failure::Admission)?;
                environment.insert("HOME".into(), agent_home.clone().into_os_string());
                environment.insert("CODEX_HOME".into(), agent_home.into_os_string());
            }
            let mut host = Host::new(
                guardian,
                executable,
                environment,
                BTreeMap::from([(workspace.clone(), work)]),
            )?
            .with_task_helper(helper, address)?
            .with_retained_task_context(context)?
            .with_approvals(approvals)?;
            if let Some((limit, send, receive)) = files {
                host = host.with_file_limit(limit)?;
                if send {
                    host = host.with_file_tools()?;
                }
                if receive {
                    host = host.with_receive_tools()?;
                }
            }
            if coordination {
                host = host.with_coordination_tools()?;
            }
            if let Some(account) = account {
                host = host.with_managed_account(account)?;
            }
            if let Some(local) = local_codex {
                host = host.with_retained_local_codex(local)?;
            }
            let host = host.into_shared();
            let warm = WarmRuntime::start_at(
                domain.clone(),
                scope,
                home,
                host.clone(),
                workspace,
                limits,
                until,
            )?;
            Ok(FactoryRuntime {
                host,
                domain,
                warm: Some(warm),
                phase: Phase::Initial,
                last: None,
                cancelled: AtomicBool::new(false),
            })
        })
        .await
        .map_err(|_| Failure::Worker)?
    }
}
impl FactoryRuntime {
    pub async fn ready(&mut self) -> Result<(), Failure> {
        self.warm.as_mut().ok_or(Failure::Admission)?.ready().await
    }
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        if let Some(warm) = &self.warm {
            warm.cancel();
        }
        if let Phase::Running(original) = &self.phase {
            original.cancel();
        }
    }
    pub async fn activate(&mut self) -> Result<hagency_core::project::Engagement, Failure> {
        self.warm
            .as_mut()
            .ok_or(Failure::Admission)?
            .activate()
            .await
    }
    pub fn bind_claim_profile(
        &self,
        profile: OwnedClaimProfile,
    ) -> Result<OwnedClaimProfile, Failure> {
        self.host.0.bind_claim_profile(profile)
    }
    pub fn dispatch(
        &mut self,
        capability: hagency_core::tasks::RunnerCapability,
        limits: Limits,
    ) -> Result<Operation, Failure> {
        let ticket = self.reserve_dispatch(capability, limits)?;
        self.dispatch_reserved(ticket)
    }
    /// Spend original admission before queuing any possibly blocking handoff.
    /// A failed/lost queued job cannot select a replacement capability.
    pub fn reserve_dispatch(
        &mut self,
        capability: hagency_core::tasks::RunnerCapability,
        limits: Limits,
    ) -> Result<FactoryDispatch, Failure> {
        if self.cancelled.load(Ordering::Acquire) {
            return Err(Failure::Admission);
        }
        let fingerprint = hagency_core::canonical::transport_digest(&serde_json::json!(capability))
            .map_err(|_| Failure::Admission)?;
        if self.last.as_ref() == Some(&fingerprint) {
            return Err(Failure::Admission);
        }
        let binding = match &self.phase {
            Phase::Initial if self.warm.is_some() => None,
            Phase::Reattached(binding) => Some((**binding).clone()),
            Phase::Running(original) => Some(original.binding()?),
            _ => return Err(Failure::Admission),
        };
        let original = Arc::new(Admission {
            capability,
            limits,
            binding,
        });
        self.phase = Phase::Queued(original.clone());
        self.last = Some(fingerprint);
        Ok(FactoryDispatch(original))
    }
    /// Only the exact ticket from this original runtime can consume its slot.
    /// Call on the ownership worker: failed warm handoff/drop may join there.
    pub fn dispatch_reserved(&mut self, ticket: FactoryDispatch) -> Result<Operation, Failure> {
        if self.cancelled.load(Ordering::Acquire) {
            return Err(Failure::Admission);
        }
        if !matches!(&self.phase,Phase::Queued(original) if Arc::ptr_eq(original,&ticket.0)) {
            return Err(Failure::Admission);
        }
        self.phase = Phase::Spent;
        let admission = &ticket.0;
        // A refused handoff is that attempt's failure, not the agent's
        // (ADR-182 decision 2): the runtime keeps the follow-up root the
        // handoff was made from — the warm child's own binding, or the one a
        // restart rebuilt — so the next dispatch launches a follow-up, exactly
        // as after a restart, instead of finding a spent runtime.
        let (operation, fallback) = match &admission.binding {
            None => {
                let warm = self.warm.take().ok_or(Failure::Admission)?;
                let fallback = warm.binding();
                (
                    warm.dispatch_requiring_workspace(
                        admission.capability.clone(),
                        admission.limits,
                    ),
                    fallback,
                )
            }
            Some(binding) => (
                Operation::start_factory_followup(
                    self.domain.clone(),
                    admission.capability.clone(),
                    self.host.clone(),
                    admission.limits,
                    binding.clone(),
                ),
                binding.clone(),
            ),
        };
        let operation = match operation {
            Ok(operation) => operation,
            Err(failure) => {
                self.phase = Phase::Reattached(Box::new(fallback));
                return Err(failure);
            }
        };
        self.phase = Phase::Running(operation.continuation());
        Ok(operation)
    }
}
