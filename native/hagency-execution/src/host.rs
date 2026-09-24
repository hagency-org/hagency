use crate::workspace::{Root, Workspaces};
use hagency_core::tasks::RunnerCapability;
use hagency_platform::Launch;
use hagency_runtime::codex::{
    session::{Settings, TASK_MCP_ENV, TaskMcp},
    transport,
};
use hagency_store::OwnedDispatchScope;
use std::sync::Arc;
use std::{collections::BTreeMap, ffi::OsString, net::SocketAddr, path::PathBuf};

/// The exact argv the host ever passes to the Codex CLI (ADR-139): one
/// argument, `app-server`. Sandbox policy and approval mode travel in the
/// typed initialize/thread request, and stdio is the pinned CLI's default
/// transport — so no `--sandbox`, `--ask-for-approval`, `--cd`, `--listen` or
/// `--stdio` flag may ever appear here. Both launch sites (the Host
/// constructor's admission validation and `Host::prepare`) build argv from
/// this one function, which `native_codex_argv_is_app_server_only` pins.
fn app_server_arguments() -> Vec<OsString> {
    vec!["app-server".into()]
}

/// One operation uses a 100 ms..20 min absolute monotonic execution deadline.
/// Native RPC response/write waits are 10 ms..2 s. Acknowledged turn silence
/// uses the same original operation budget. Cancellation is checked every
/// 20 ms; authority is scheduled every 100 ms and its receipt may take up to 2 s.
/// Synchronous join has a conservative operation-budget-plus-30 s combined wait
/// allowance (execution plus platform startup/stop/drop and final domain receipts).
/// This bounds library waits, not OS scheduling or a stalled kernel syscall.
#[derive(Clone, Copy)]
pub struct Limits {
    pub operation_ms: u64,
    pub response_ms: u64,
}
impl Limits {
    pub fn validate(self) -> bool {
        (100..=hagency_core::tasks::MAX_OWNED_OPERATION_MS).contains(&self.operation_ms)
            && (10..=2_000).contains(&self.response_ms)
            && self.response_ms <= self.operation_ms
    }
    /// Host claim lifetime only. The short renewable lease remains independent;
    /// this never extends an existing capability or operation deadline.
    pub fn capability_ms(self) -> Result<u64, super::Failure> {
        if !self.validate() {
            return Err(super::Failure::Admission);
        }
        Ok((self.operation_ms + 30_000).max(60_000))
    }
}

/// Host configuration only: no Deserialize and no runtime/HTTP constructor.
/// Opens and retains already-private workspace roots. The caller must keep
/// their fixed paths and ancestors stable for the entire operation. Retained
/// source custody and comparison checks do not isolate hostile namespace
/// mutation, and this API must not enable a production runner catalog.
pub struct Host {
    pub(crate) guardian: PathBuf,
    pub(crate) approvals: Option<crate::ApprovalHost>,
    executable: PathBuf,
    environment: BTreeMap<OsString, OsString>,
    workspaces: Workspaces,
    managed_account: Option<hagency_store::ManagedAccount>,
    pub(crate) local_codex: Option<Arc<crate::LocalCodex>>,
    task_helper: Option<(PathBuf, SocketAddr)>,
    pub(crate) task_context: Option<Arc<hagency_store::task_context::RetainedTaskContext>>,
    file_tools: bool,
    receive_tools: bool,
    coordination_tools: bool,
    #[cfg(test)]
    pub(crate) discard_start_reply: bool,
    #[cfg(test)]
    pub(crate) approval_fault: Option<crate::approval::Fault>,
    #[cfg(test)]
    pub(crate) approval_gate: Option<Arc<crate::approval::Gate>>,
    #[cfg(test)]
    pub(crate) discard_usage_binding_reply: bool,
    #[cfg(test)]
    pub(crate) panic_after_workspace: bool,
    // Test-double marker (ADR-053 amendment); no production builder sets it.
    pub(crate) guardian_prepare_stall: bool,
}

/// A retained immutable host configuration that may be reused for sequential
/// owned operations. Sharing does not duplicate account authority, reopen a
/// workspace root, or create a new runner configuration: every operation
/// prepares from the same originally-admitted [`Host`].
///
/// The handle intentionally exposes no access to the underlying host. It is
/// only accepted by the operation constructors that re-run all per-dispatch
/// admission and authority checks.
#[derive(Clone)]
pub struct SharedHost(pub(crate) Arc<Host>);

impl Host {
    pub(crate) fn bind_claim_profile(
        &self,
        profile: hagency_store::OwnedClaimProfile,
    ) -> Result<hagency_store::OwnedClaimProfile, super::Failure> {
        if let Some(local) = &self.local_codex {
            return local.bind_claim(profile);
        }
        match &self.managed_account {
            Some(account) => account
                .bind_claim_profile(profile)
                .map_err(|error| super::Failure::lost(super::AuthoritySite::HostClaim, &error)),
            None => Ok(profile),
        }
    }
    pub fn new(
        guardian: PathBuf,
        executable: PathBuf,
        environment: BTreeMap<OsString, OsString>,
        workspaces: BTreeMap<String, PathBuf>,
    ) -> Result<Self, super::Failure> {
        if !guardian.is_absolute()
            || !executable.is_absolute()
            || workspaces.is_empty()
            || workspaces.len() > 16
            || environment.keys().any(|key| {
                key.to_string_lossy()
                    .eq_ignore_ascii_case(TaskMcp::FILE_TOOLS_ENV)
                    || key
                        .to_string_lossy()
                        .eq_ignore_ascii_case(TaskMcp::RECEIVE_TOOLS_ENV)
            })
        {
            return Err(super::Failure::Admission);
        }
        let workspaces = Workspaces::open(workspaces)?;
        // Reuse the launch validator for exact environment/argv byte limits.
        Launch {
            executable: executable.clone(),
            arguments: app_server_arguments(),
            directory: workspaces.first_path()?.to_path_buf(),
            environment: environment.clone(),
            require_crash_containment: false,
        }
        .validate()
        .map_err(|_| super::Failure::Admission)?;
        Ok(Self {
            guardian,
            approvals: None,
            executable,
            environment,
            workspaces,
            managed_account: None,
            local_codex: None,
            task_helper: None,
            task_context: None,
            file_tools: false,
            receive_tools: false,
            coordination_tools: false,
            #[cfg(test)]
            discard_start_reply: false,
            #[cfg(test)]
            approval_fault: None,
            #[cfg(test)]
            approval_gate: None,
            #[cfg(test)]
            discard_usage_binding_reply: false,
            #[cfg(test)]
            panic_after_workspace: false,
            guardian_prepare_stall: false,
        })
    }
    /// Consume the original registry binding. Managed scope cannot fall through
    /// to the fixed development HOME, and another account cannot replace it.
    pub fn with_managed_account(
        mut self,
        account: hagency_store::ManagedAccount,
    ) -> Result<Self, super::Failure> {
        if self.managed_account.is_some() || self.local_codex.is_some() {
            return Err(super::Failure::Admission);
        }
        self.managed_account = Some(account);
        Ok(self)
    }
    /// Explicit provider-owned local login, separate from managed readiness.
    pub fn with_local_codex(self, local: crate::LocalCodex) -> Result<Self, super::Failure> {
        self.with_retained_local_codex(Arc::new(local))
    }
    pub(crate) fn with_retained_local_codex(
        mut self,
        local: Arc<crate::LocalCodex>,
    ) -> Result<Self, super::Failure> {
        if self.managed_account.is_some() || self.local_codex.is_some() {
            return Err(super::Failure::Admission);
        }
        local.apply(&mut self.environment)?;
        self.local_codex = Some(local);
        Ok(self)
    }
    /// Host-selected native executable and literal loopback endpoint only. The
    /// task/capability are supplied later from the validated owned dispatch.
    /// The host must protect the executable, workspace and Codex config/home;
    /// executable path checks are not executable custody, and retained source
    /// roots do not establish stable namespace provisioning.
    pub fn with_approvals(
        mut self,
        approvals: crate::ApprovalHost,
    ) -> Result<Self, super::Failure> {
        if self.approvals.is_some() {
            return Err(super::Failure::Admission);
        }
        self.approvals = Some(approvals);
        Ok(self)
    }
    pub fn with_task_helper(
        mut self,
        executable: PathBuf,
        address: SocketAddr,
    ) -> Result<Self, super::Failure> {
        if !address.ip().is_loopback()
            || address.port() == 0
            || matches!(address, SocketAddr::V6(v) if v.scope_id()!=0 || v.flowinfo()!=0)
            || !executable.is_file()
            || self.environment.keys().any(|key| {
                TASK_MCP_ENV
                    .iter()
                    .any(|reserved| key.to_string_lossy().eq_ignore_ascii_case(reserved))
            })
        {
            return Err(super::Failure::Admission);
        }
        TaskMcp::new(
            executable.clone(),
            "validation_only".into(),
            self.system_root()?,
        )
        .map_err(|_| super::Failure::Admission)?;
        self.task_helper = Some((executable, address));
        Ok(self)
    }
    /// Explicit one-shot task-context bridge for the same initialized owner.
    /// It does not permit pre-Started spawn or create a runtime readiness fact.
    pub fn with_retained_task_context(
        mut self,
        context: Arc<hagency_store::task_context::RetainedTaskContext>,
    ) -> Result<Self, super::Failure> {
        if self.task_helper.is_none() || self.task_context.is_some() {
            return Err(super::Failure::Admission);
        }
        let mut environment = BTreeMap::new();
        context
            .apply_environment(&mut environment)
            .map_err(|_| super::Failure::Admission)?;
        self.task_context = Some(context);
        Ok(self)
    }
    /// Select a smaller copy profile before starting an operation. Source
    /// handles are duplicated from the same retained roots, never reopened.
    pub fn with_file_limit(mut self, max_bytes: usize) -> Result<Self, super::Failure> {
        self.workspaces = self.workspaces.limit(max_bytes)?;
        Ok(self)
    }
    /// Fixed development presentation opt-in, after configuring the required
    /// helper. File admission still requires the service's original authority.
    pub fn with_file_tools(mut self) -> Result<Self, super::Failure> {
        if self.task_helper.is_none() {
            return Err(super::Failure::Admission);
        }
        self.file_tools = true;
        Ok(self)
    }
    /// Test double only (ADR-053 amendment): delay the blocking spawn thread
    /// past the granted operation budget, so the bounded-spawn expiry is
    /// exercised deterministically and a REAL late child still appears through
    /// the custody handoff. Compiled unconditionally so the integration
    /// selector can reach it (a `#[cfg(test)]` double is invisible to
    /// integration tests); no production caller sets it, and the stall itself
    /// is a multiple of the granted operation budget — never a bare literal.
    pub fn with_guardian_prepare_stall(mut self) -> Self {
        self.guardian_prepare_stall = true;
        self
    }
    /// Presentation only (ADR180): lets the owned runtime call the coordination
    /// tools its helper already serves. Project and fleet authority stay in the
    /// service behind the runner capability.
    pub fn with_coordination_tools(mut self) -> Result<Self, super::Failure> {
        if self.task_helper.is_none() {
            return Err(super::Failure::Admission);
        }
        self.coordination_tools = true;
        Ok(self)
    }
    /// Presentation only, after configuring the original native task helper.
    pub fn with_receive_tools(mut self) -> Result<Self, super::Failure> {
        if self.task_helper.is_none() {
            return Err(super::Failure::Admission);
        }
        self.receive_tools = true;
        Ok(self)
    }
    /// Retain this exact admitted host for more than one owned operation.
    /// Per-operation state remains in [`crate::Operation`]; this only shares
    /// the immutable configuration and retained workspace/account handles.
    pub fn into_shared(self) -> SharedHost {
        SharedHost(Arc::new(self))
    }
    fn system_root(&self) -> Result<Option<String>, super::Failure> {
        let mut roots = self
            .environment
            .iter()
            .filter(|(key, _)| key.to_string_lossy().eq_ignore_ascii_case("SystemRoot"));
        let first = roots
            .next()
            .map(|(_, v)| {
                v.to_str()
                    .map(str::to_owned)
                    .ok_or(super::Failure::Admission)
            })
            .transpose()?;
        if roots.next().is_some() {
            return Err(super::Failure::Admission);
        }
        Ok(first)
    }
    pub(crate) fn task_helper_enabled(&self) -> bool {
        self.task_helper.is_some()
    }
    pub(crate) fn prepare(
        &self,
        scope: &OwnedDispatchScope,
        capability: &RunnerCapability,
        limits: Limits,
    ) -> Result<Prepared, super::Failure> {
        self.prepare_bound(scope, capability, limits, self.task_context.as_ref())
    }
    /// A positively retired factory operation may start distinct work through
    /// the ordinary direct binding. No initial warm context is reused or changed.
    pub(crate) fn prepare_followup(
        &self,
        scope: &OwnedDispatchScope,
        capability: &RunnerCapability,
        limits: Limits,
    ) -> Result<Prepared, super::Failure> {
        self.prepare_bound(scope, capability, limits, None)
    }
    fn prepare_bound(
        &self,
        scope: &OwnedDispatchScope,
        capability: &RunnerCapability,
        limits: Limits,
        task_context: Option<&Arc<hagency_store::task_context::RetainedTaskContext>>,
    ) -> Result<Prepared, super::Failure> {
        // ADR-142: a dispatch naming a framework with no native runner is
        // refused by name before any workspace is resolved, custody-checked
        // or process spawned. Hoisted above the resource slice and the
        // workspace get/check so "no process and no workspace work" is
        // literal, and distinct from the generic Admission refusal below.
        if scope.resource().framework == "claude" {
            return Err(super::Failure::UnsupportedRunner {
                framework: scope.resource().framework.clone(),
            });
        }
        if let Some(local) = &self.local_codex {
            local.admit(scope)?;
        }
        let [workspace] = scope.input().resources.as_slice() else {
            return Err(super::Failure::Admission);
        };
        if !workspace.exclusive || !limits.validate() {
            return Err(super::Failure::Admission);
        }
        let root = self.workspaces.get(&workspace.id)?;
        root.check().map_err(|_| super::Failure::Admission)?;
        // ADR-116 amendment: the session and process working directory string
        // is the ordinary projection of the retained root. On Windows the
        // canonical root is a verbatim `\\?\` path; a child launched there
        // reports that form as its callback cwd, which the untrusted path
        // parser refuses, so no owner grant could ever gain a reusable scope.
        // Directory custody still comes from the retained handle checked above;
        // the projection refuses device namespaces and ambiguous aliases.
        let path = std::path::PathBuf::from(root.approval_path()?);
        if let Some(local) = &self.local_codex {
            local.separate_from(root.path())?;
        }
        let resource = scope.resource();
        if resource.framework != "codex"
            || resource.provider.as_deref().is_some_and(|v| v != "openai")
        {
            return Err(super::Failure::Admission);
        }
        let effort = resource.reasoning.as_deref().unwrap_or("medium");
        if !["none", "minimal", "low", "medium", "high", "xhigh"].contains(&effort) {
            return Err(super::Failure::Admission);
        }
        let mut settings = Settings::new(path.clone(), resource.model.clone(), effort.into())
            .map_err(|_| super::Failure::Admission)?;
        // Every byte comes from the immutable dispatch payload. This informational
        // text grants neither task maintenance, process authority nor approval.
        let input = hagency_core::canonical::encode_payload(&scope.input().payload)
            .map_err(|_| super::Failure::Admission)?;
        if input.len() > hagency_runtime::codex::session::MAX_TEXT_BYTES {
            return Err(super::Failure::Admission);
        }
        let mut environment = self.environment.clone();
        let mut late_helper = None;
        let account = match &self.managed_account {
            Some(account) => {
                let launch = account
                    .prepare_launch(scope)
                    .map_err(|_| super::Failure::Admission)?;
                launch
                    .apply_codex_environment(&mut environment)
                    .map_err(|_| super::Failure::Admission)?;
                Some(launch)
            }
            None if scope.requires_managed_account() => return Err(super::Failure::Admission),
            None => None,
        };
        if let Some(local) = &self.local_codex {
            local.apply(&mut environment)?;
        }
        if let Some((executable, address)) = &self.task_helper {
            let mut helper = TaskMcp::new(
                executable.clone(),
                scope.task().id.clone(),
                self.system_root()?,
            )
            .map_err(|_| super::Failure::Admission)?;
            environment.insert(TASK_MCP_ENV[0].into(), address.to_string().into());
            if let Some(context) = task_context {
                context
                    .separate_from(&path)
                    .map_err(|_| super::Failure::Admission)?;
                context
                    .apply_environment(&mut environment)
                    .map_err(|_| super::Failure::Admission)?;
            } else {
                let encoded =
                    serde_json::to_string(capability).map_err(|_| super::Failure::Admission)?;
                if encoded.len() > 4096 {
                    return Err(super::Failure::Admission);
                }
                environment.insert(TASK_MCP_ENV[1].into(), encoded.into());
                environment.insert(TASK_MCP_ENV[2].into(), scope.task().id.clone().into());
            }
            if self.file_tools {
                environment.insert(TaskMcp::FILE_TOOLS_ENV.into(), "1".into());
                helper = helper.with_file_tools();
            }
            if self.receive_tools {
                environment.insert(TaskMcp::RECEIVE_TOOLS_ENV.into(), "1".into());
                helper = helper.with_receive_tools();
            }
            if self.coordination_tools {
                helper = helper.with_coordination_tools();
            }
            if task_context.is_some() {
                late_helper = Some(helper);
            } else {
                settings = settings.with_task_mcp(helper);
            }
        }
        // ADR-139: argv is deliberately just "app-server" — sandbox and
        // approval travel in the typed initialize request, and stdio is the
        // pinned CLI's default transport (see adr-139-native-codex-launch-surface.md).
        // Both launch sites build argv from `app_server_arguments`, so
        // `native_codex_argv_is_app_server_only` pins every argument the
        // host can ever pass to the Codex CLI.
        let launch = self.launch(path.clone(), environment)?;
        Ok(Prepared {
            launch,
            settings,
            io_limits: transport::Limits {
                write_timeout_ms: limits.response_ms,
                // Tool execution need not produce unsolicited events within
                // an RPC response interval. Fixed lifetime and the original
                // operation deadline still bound every quiet turn.
                event_wait_ms: limits.operation_ms,
                lifetime_ms: limits.operation_ms,
            },
            input,
            root,
            account,
            late_helper,
        })
    }
    fn launch(
        &self,
        path: std::path::PathBuf,
        environment: BTreeMap<OsString, OsString>,
    ) -> Result<Launch, super::Failure> {
        let launch = Launch {
            executable: self.executable.clone(),
            arguments: app_server_arguments(),
            directory: path,
            environment,
            require_crash_containment: false,
        };
        launch.validate().map_err(|_| super::Failure::Admission)?;
        Ok(launch)
    }
    /// The checks `prepare_warm` makes on the scope, the home and the workspace
    /// root, without preparing a launch: a re-attached agent starts no warm
    /// child, its next task launches through the ordinary follow-up binding.
    pub(crate) fn reattach_root(
        &self,
        scope: &hagency_store::OwnedProvisionScope,
        home: &hagency_store::agent_home::ManagedAgentHome,
        workspace_id: &str,
    ) -> Result<Arc<crate::workspace::Root>, super::Failure> {
        if self.task_helper.is_none() || self.task_context.is_some() {
            return Err(super::Failure::Admission);
        }
        let resource = scope.resource();
        if resource.framework == "claude" {
            return Err(super::Failure::UnsupportedRunner {
                framework: resource.framework.clone(),
            });
        }
        if resource.framework != "codex"
            || resource.provider.as_deref().is_some_and(|v| v != "openai")
        {
            return Err(super::Failure::Admission);
        }
        home.check_provision_scope(scope)
            .map_err(|_| super::Failure::Admission)?;
        let root = self.workspaces.get(workspace_id)?;
        root.check().map_err(|_| super::Failure::Admission)?;
        if root.path()
            != home
                .workdir_path()
                .map_err(|_| super::Failure::Admission)?
                .as_path()
        {
            return Err(super::Failure::Admission);
        }
        if let Some(local) = &self.local_codex {
            local.admit_provision(scope)?;
            local.separate_from(root.path())?;
        }
        Ok(root)
    }
    pub(crate) fn prepare_warm(
        &self,
        scope: &hagency_store::OwnedProvisionScope,
        home: &hagency_store::agent_home::ManagedAgentHome,
        workspace_id: &str,
        limits: Limits,
    ) -> Result<Prepared, super::Failure> {
        if !limits.validate() || self.task_helper.is_none() || self.task_context.is_none() {
            return Err(super::Failure::Admission);
        }
        let resource = scope.resource();
        if resource.framework == "claude" {
            return Err(super::Failure::UnsupportedRunner {
                framework: resource.framework.clone(),
            });
        }
        if resource.framework != "codex"
            || resource.provider.as_deref().is_some_and(|v| v != "openai")
        {
            return Err(super::Failure::Admission);
        }
        home.check_provision_scope(scope)
            .map_err(|_| super::Failure::Admission)?;
        let root = self.workspaces.get(workspace_id)?;
        root.check().map_err(|_| super::Failure::Admission)?;
        if root.path()
            != home
                .workdir_path()
                .map_err(|_| super::Failure::Admission)?
                .as_path()
        {
            return Err(super::Failure::Admission);
        }
        let path = std::path::PathBuf::from(root.approval_path()?);
        let effort = resource.reasoning.as_deref().unwrap_or("medium");
        if !["none", "minimal", "low", "medium", "high", "xhigh"].contains(&effort) {
            return Err(super::Failure::Admission);
        }
        let settings = Settings::new(path.clone(), resource.model.clone(), effort.into())
            .map_err(|_| super::Failure::Admission)?;
        let mut environment = self.environment.clone();
        if let Some(local) = &self.local_codex {
            local.admit_provision(scope)?;
            local.separate_from(root.path())?;
            local.apply(&mut environment)?;
        }
        let account = match &self.managed_account {
            Some(account) => {
                let launch = account
                    .prepare_provision_launch(scope)
                    .map_err(|_| super::Failure::Admission)?;
                launch
                    .apply_codex_environment(&mut environment)
                    .map_err(|_| super::Failure::Admission)?;
                Some(launch)
            }
            None if scope.requires_managed_account() => return Err(super::Failure::Admission),
            None => None,
        };
        let (_, address) = self.task_helper.as_ref().ok_or(super::Failure::Admission)?;
        environment.insert(TASK_MCP_ENV[0].into(), address.to_string().into());
        let context = self
            .task_context
            .as_ref()
            .ok_or(super::Failure::Admission)?;
        context
            .separate_from(&path)
            .map_err(|_| super::Failure::Admission)?;
        context
            .apply_environment(&mut environment)
            .map_err(|_| super::Failure::Admission)?;
        if self.file_tools {
            environment.insert(TaskMcp::FILE_TOOLS_ENV.into(), "1".into());
        }
        if self.receive_tools {
            environment.insert(TaskMcp::RECEIVE_TOOLS_ENV.into(), "1".into());
        }
        Ok(Prepared {
            launch: self.launch(path, environment)?,
            settings,
            io_limits: transport::Limits {
                write_timeout_ms: limits.response_ms,
                event_wait_ms: limits.operation_ms,
                lifetime_ms: limits.operation_ms,
            },
            input: String::new(),
            root,
            account,
            late_helper: None,
        })
    }
}

pub(crate) struct Prepared {
    pub(crate) launch: Launch,
    pub(crate) settings: Settings,
    pub(crate) io_limits: transport::Limits,
    pub(crate) input: String,
    pub(crate) root: Arc<Root>,
    pub(crate) account: Option<hagency_store::ManagedLaunch>,
    pub(crate) late_helper: Option<TaskMcp>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_owned_long_operation_limits() {
        use hagency_core::tasks::{MAX_OWNED_CAPABILITY_MS, MAX_OWNED_OPERATION_MS};
        assert_eq!(
            MAX_OWNED_OPERATION_MS,
            hagency_runtime::codex::MAX_REQUEST_MS
        );
        for (operation_ms, capability_ms) in [
            (100, 60_000),
            (30_000, 60_000),
            (45_000, 75_000),
            (MAX_OWNED_OPERATION_MS, MAX_OWNED_CAPABILITY_MS),
        ] {
            let limits = Limits {
                operation_ms,
                response_ms: 100,
            };
            assert!(limits.validate());
            assert_eq!(limits.capability_ms().unwrap(), capability_ms);
        }
        for operation_ms in [0, 99, MAX_OWNED_OPERATION_MS + 1, u64::MAX] {
            let limits = Limits {
                operation_ms,
                response_ms: 100,
            };
            assert!(!limits.validate());
            assert!(limits.capability_ms().is_err());
        }
        for response_ms in [0, 9, 2001, u64::MAX] {
            assert!(
                !Limits {
                    operation_ms: MAX_OWNED_OPERATION_MS,
                    response_ms
                }
                .validate()
            );
        }
        assert!(
            !Limits {
                operation_ms: 100,
                response_ms: 101
            }
            .validate()
        );
        let approvals = crate::ApprovalHost::new(4, 2, 598_000, 2000).unwrap();
        assert!(approvals.fits(Limits {
            operation_ms: MAX_OWNED_OPERATION_MS,
            response_ms: 2000
        }));
        assert!(!approvals.fits(Limits {
            operation_ms: 599_999,
            response_ms: 2000
        }));
        assert!(crate::ApprovalHost::new(4, 2, 598_001, 2000).is_err());
        assert!(crate::ApprovalHost::new(4, 2, u64::MAX, 2000).is_err());
        // Increasing active execution must not silently increase pre-activation
        // startup's distinct original thirty-second budget.
        assert!(
            crate::WarmLimits {
                initialize: Limits {
                    operation_ms: 30_000,
                    response_ms: 2000
                },
                idle_ms: 1000
            }
            .validate()
        );
        assert!(
            !crate::WarmLimits {
                initialize: Limits {
                    operation_ms: 30_001,
                    response_ms: 2000
                },
                idle_ms: 1000
            }
            .validate()
        );
    }

    #[test]
    fn native_codex_argv_is_app_server_only() {
        // ADR-139: the host passes exactly one argument, `app-server`. Both
        // launch sites (the Host constructor's admission validation and
        // `Host::prepare`) build argv from this single function, so pinning
        // its output pins every argument the host can ever pass.
        let arguments = app_server_arguments();
        assert_eq!(
            arguments,
            vec![OsString::from("app-server")],
            "the spawn argv must be exactly one argument: app-server"
        );
        // No sandbox, approval, directory or transport flag may ever appear
        // on argv: policy travels in the typed initialize/thread request and
        // stdio is the pinned CLI's default transport.
        assert!(
            arguments
                .iter()
                .all(|argument| { !argument.to_string_lossy().starts_with("--") }),
            "no flag may ever appear on the Codex spawn argv"
        );
    }

    #[test]
    fn native_receive_tools_host_profile() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("workspace");
        hagency_store::private::directory(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let executable = std::env::current_exe().unwrap();
        let host = |environment| {
            Host::new(
                executable.clone(),
                executable.clone(),
                environment,
                BTreeMap::from([("workspace".into(), root.clone())]),
            )
        };
        let default = host(BTreeMap::new()).unwrap();
        assert!(!default.receive_tools);
        assert!(matches!(
            default.with_receive_tools(),
            Err(super::super::Failure::Admission)
        ));
        for send in [false, true] {
            let mut configured = host(BTreeMap::new())
                .unwrap()
                .with_task_helper(executable.clone(), "127.0.0.1:13300".parse().unwrap())
                .unwrap();
            if send {
                configured = configured.with_file_tools().unwrap();
            }
            let configured = configured.with_receive_tools().unwrap();
            assert!(configured.receive_tools);
            assert_eq!(configured.file_tools, send);
        }
        for key in [
            "HAGENCY_RECEIVE_FILE_TOOLS",
            "hagency_receive_file_tools",
            "Hagency_Receive_File_Tools",
        ] {
            assert!(matches!(
                host(BTreeMap::from([(key.into(), "1".into())])),
                Err(super::super::Failure::Admission)
            ));
        }
    }

    #[test]
    fn native_file_tools_host_profile() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("workspace");
        hagency_store::private::directory(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let executable = std::env::current_exe().unwrap();
        let host = |environment| {
            Host::new(
                executable.clone(),
                executable.clone(),
                environment,
                BTreeMap::from([("workspace".into(), root.clone())]),
            )
        };
        let default = host(BTreeMap::new()).unwrap();
        assert!(!default.file_tools);
        assert!(matches!(
            default.with_file_tools(),
            Err(super::super::Failure::Admission)
        ));
        let configured = host(BTreeMap::new())
            .unwrap()
            .with_task_helper(executable.clone(), "127.0.0.1:13300".parse().unwrap())
            .unwrap()
            .with_file_tools()
            .unwrap();
        assert!(configured.file_tools);
        for key in [
            "HAGENCY_FILE_TOOLS",
            "hagency_file_tools",
            "Hagency_File_Tools",
        ] {
            // Reserved even without a helper/profile. An ambient inherited
            // marker must never opt an otherwise default Host into file tools.
            assert!(matches!(
                host(BTreeMap::from([(key.into(), "1".into())])),
                Err(super::super::Failure::Admission)
            ));
        }
    }
}
