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

/// One operation uses a 100 ms..30 s absolute monotonic execution deadline.
/// Native RPC response/write waits are 10 ms..2 s. Acknowledged turn silence
/// uses the same original operation budget. Cancellation is checked every
/// 20 ms; authority is scheduled every 100 ms and its receipt may take up to 2 s.
/// Synchronous join has a conservative 60 s maximum combined wait allowance
/// (30 s execution plus platform startup/stop/drop and final domain receipts).
/// This bounds library waits, not OS scheduling or a stalled kernel syscall.
#[derive(Clone, Copy)]
pub struct Limits {
    pub operation_ms: u64,
    pub response_ms: u64,
}
impl Limits {
    pub(crate) fn validate(self) -> bool {
        (100..=30_000).contains(&self.operation_ms)
            && (10..=2_000).contains(&self.response_ms)
            && self.response_ms <= self.operation_ms
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
    task_helper: Option<(PathBuf, SocketAddr)>,
    file_tools: bool,
    receive_tools: bool,
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
}
impl Host {
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
            arguments: vec!["app-server".into()],
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
            task_helper: None,
            file_tools: false,
            receive_tools: false,
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
        })
    }
    /// Consume the original registry binding. Managed scope cannot fall through
    /// to the fixed development HOME, and another account cannot replace it.
    pub fn with_managed_account(
        mut self,
        account: hagency_store::ManagedAccount,
    ) -> Result<Self, super::Failure> {
        if self.managed_account.is_some() {
            return Err(super::Failure::Admission);
        }
        self.managed_account = Some(account);
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
    /// Presentation only, after configuring the original native task helper.
    pub fn with_receive_tools(mut self) -> Result<Self, super::Failure> {
        if self.task_helper.is_none() {
            return Err(super::Failure::Admission);
        }
        self.receive_tools = true;
        Ok(self)
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
        if let Some((executable, address)) = &self.task_helper {
            let mut helper = TaskMcp::new(
                executable.clone(),
                scope.task().id.clone(),
                self.system_root()?,
            )
            .map_err(|_| super::Failure::Admission)?;
            let encoded =
                serde_json::to_string(capability).map_err(|_| super::Failure::Admission)?;
            if encoded.len() > 4096 {
                return Err(super::Failure::Admission);
            }
            environment.insert(TASK_MCP_ENV[0].into(), address.to_string().into());
            environment.insert(TASK_MCP_ENV[1].into(), encoded.into());
            environment.insert(TASK_MCP_ENV[2].into(), scope.task().id.clone().into());
            if self.file_tools {
                environment.insert(TaskMcp::FILE_TOOLS_ENV.into(), "1".into());
                helper = helper.with_file_tools();
            }
            if self.receive_tools {
                environment.insert(TaskMcp::RECEIVE_TOOLS_ENV.into(), "1".into());
                helper = helper.with_receive_tools();
            }
            settings = settings.with_task_mcp(helper);
        }
        let launch = Launch {
            executable: self.executable.clone(),
            arguments: vec!["app-server".into()],
            directory: path.clone(),
            environment,
            require_crash_containment: false,
        };
        launch.validate().map_err(|_| super::Failure::Admission)?;
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
