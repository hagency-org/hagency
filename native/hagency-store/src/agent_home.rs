//! Original inline v1 physical homes, not runtime or Applied authority.
//! Fixed host roots are retained; hostile same-UID namespace changes are outside
//! the provisioning contract. Comparisons detect replacement, not lock topology.
use crate::{DomainStore, Effect, EffectOutcome, Error, private};
use cap_std::{ambient_authority, fs::Dir};
use hagency_core::{
    authority::{ProjectRequest, Registration},
    canonical, project,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Instant, SystemTime},
};
use tokio::sync::Semaphore;

const FILE_BYTES: usize = 4 * 1024 * 1024;
const TOTAL_BYTES: usize = 64 * 1024 * 1024;
const ENTRIES: usize = 4096;
const DEPTH: usize = 16;
const JOBS: usize = 16;
const CLAUDE: &str = include_str!("../../../docs/workspace-claude-md-template.md");
const AGENTS: &str = include_str!("../../../docs/workspace-agents-md-template.md");
const SUPERVISOR_CLAUDE: &str =
    include_str!("../../../docs/workspace-supervisor-claude-template.md");
const SUPERVISOR_AGENTS: &str =
    include_str!("../../../docs/workspace-supervisor-agents-template.md");

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectMode {
    Copy,
    Symlink,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HomeProject {
    pub project_id: String,
    pub source: PathBuf,
    pub mode: ProjectMode,
}
struct Root {
    path: PathBuf,
    dir: Dir,
    private: bool,
}
impl Root {
    fn open(path: PathBuf, is_private: bool) -> Result<Self, Error> {
        path_valid(&path)?;
        if path.canonicalize()? != path {
            return Err(Error::Private);
        }
        let dir = Dir::open_ambient_dir(&path, ambient_authority())?;
        let root = Self {
            path,
            dir,
            private: is_private,
        };
        root.check()?;
        Ok(root)
    }
    fn check(&self) -> Result<(), Error> {
        let held = self.dir.try_clone()?.into_std_file();
        if self.private {
            private::check_handle(&held)?;
        }
        if self.path.canonicalize()? != self.path {
            return Err(Error::Private);
        }
        let current = Dir::open_ambient_dir(&self.path, ambient_authority())?.into_std_file();
        if self.private {
            private::check_handle(&current)?;
        }
        if !hagency_platform::same_directory(&held, &current)? {
            return Err(Error::Private);
        }
        Ok(())
    }
}
struct Source {
    root: Root,
    mode: ProjectMode,
}
struct Binary {
    path: PathBuf,
    file: File,
    length: u64,
    modified: SystemTime,
}
impl Binary {
    fn check(&self) -> Result<(), Error> {
        if self.path.canonicalize()? != self.path
            || !std::fs::symlink_metadata(&self.path)?.is_file()
        {
            return Err(Error::Private);
        }
        let current = File::open(&self.path)?;
        if !hagency_platform::same_file(&self.file, &current)?
            || self.file.metadata()?.len() != self.length
            || self.file.metadata()?.modified()? != self.modified
        {
            return Err(Error::Private);
        }
        Ok(())
    }
}
/// Immutable private Host configuration. No serde/debug/clone or proof callback.
pub struct ManagedHomePlan {
    root: Arc<Root>,
    sources: BTreeMap<String, Arc<Source>>,
    binary: Arc<Binary>,
    jobs: Mutex<BTreeMap<String, Arc<Job>>>,
}
struct Job {
    binding: String,
    busy: Arc<Semaphore>,
    result: Mutex<Option<Result<Arc<ManagedAgentHome>, ErrorKind>>>,
    home: Mutex<Option<Arc<ManagedAgentHome>>>,
}
#[derive(Clone, Copy)]
enum ErrorKind {
    Unknown,
    Conflict,
    Private,
    Capacity,
    Authority,
}
impl ErrorKind {
    fn error(self) -> Error {
        match self {
            Self::Unknown => Error::OutcomeUnknown,
            Self::Conflict => Error::Conflict,
            Self::Private => Error::Private,
            Self::Capacity => Error::Capacity,
            Self::Authority => Error::RunnerAuthority,
        }
    }
    fn from(error: &Error) -> Self {
        match error {
            Error::Conflict => Self::Conflict,
            Error::Private => Self::Private,
            Error::Capacity => Self::Capacity,
            Error::RunnerAuthority | Error::State | Error::Generation => Self::Authority,
            _ => Self::Unknown,
        }
    }
}
/// Actual retained created directories and protected immutable manifest. This
/// does not prove a runtime start, sandbox, dispatch, allocation or session route.
pub struct ManagedAgentHome {
    home: Root,
    workdir: Root,
    custody: Root,
    root: Arc<Root>,
    source: Arc<Source>,
    binary: Arc<Binary>,
    project_root: Option<Root>,
    project_path: PathBuf,
    binding: String,
    manifest: String,
    resource: String,
    provision: String,
}
impl ManagedAgentHome {
    pub fn workdir_path(&self) -> Result<PathBuf, Error> {
        self.check()?;
        Ok(self.workdir.path.clone())
    }
    pub fn home_path(&self) -> Result<PathBuf, Error> {
        self.check()?;
        Ok(self.home.path.clone())
    }
    pub fn check_provision_scope(&self, scope: &crate::OwnedProvisionScope) -> Result<(), Error> {
        self.check()?;
        if self.provision != scope.provision_digest()?
            || self.resource != canonical::transport_digest(&json!(scope.resource()))?
        {
            return Err(Error::RunnerAuthority);
        }
        Ok(())
    }
    fn check(&self) -> Result<(), Error> {
        self.root.check()?;
        self.source.root.check()?;
        self.binary.check()?;
        self.home.check()?;
        self.workdir.check()?;
        self.custody.check()?;
        match &self.project_root {
            Some(project) => project.check()?,
            None => {
                if !std::fs::symlink_metadata(&self.project_path)?
                    .file_type()
                    .is_symlink()
                    || self.project_path.canonicalize()? != self.source.root.path
                {
                    return Err(Error::Private);
                }
            }
        }
        if bounded(&self.home.path.join("state/home-binding"), 64)? != self.binding.as_bytes()
            || project::hash(&bounded(&self.home.path.join("agent.json"), 32 * 1024)?)
                != self.manifest
            || bounded(&self.custody.path.join("possible"), 64)? != self.binding.as_bytes()
            || bounded(&self.custody.path.join("complete"), 64)? != self.manifest.as_bytes()
        {
            return Err(Error::Conflict);
        }
        let mut count = 0;
        for entry in std::fs::read_dir(&self.custody.path)? {
            count += 1;
            let name = entry?.file_name();
            if count > 2 || !matches!(name.to_str(), Some("possible" | "complete")) {
                return Err(Error::Private);
            }
        }
        if count != 2 {
            return Err(Error::Private);
        }
        Ok(())
    }
}
impl ManagedHomePlan {
    pub fn new(root: PathBuf, projects: Vec<HomeProject>, binary: PathBuf) -> Result<Self, Error> {
        if projects.is_empty() || projects.len() > 16 {
            return Err(Error::Capacity);
        }
        let root = Arc::new(Root::open(root, true)?);
        path_valid(&binary)?;
        if binary.canonicalize()? != binary
            || !std::fs::symlink_metadata(&binary)?.is_file()
            || binary
                .to_str()
                .is_none_or(|s| s.contains(['\'', '"', '%', '!', '\r', '\n']))
        {
            return Err(Error::Private);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if std::fs::metadata(&binary)?.mode() & 0o100 == 0 {
                return Err(Error::Private);
            }
        }
        let file = File::open(&binary)?;
        let metadata = file.metadata()?;
        let binary = Arc::new(Binary {
            path: binary,
            file,
            length: metadata.len(),
            modified: metadata.modified()?,
        });
        binary.check()?;
        let mut sources: BTreeMap<String, Arc<Source>> = BTreeMap::new();
        for item in projects {
            project::identifier(&item.project_id, 128)?;
            let source = Arc::new(Source {
                root: Root::open(item.source, false)?,
                mode: item.mode,
            });
            disjoint(&root.path, &source.root.path)?;
            for prior in sources.values() {
                disjoint(&prior.root.path, &source.root.path)?;
            }
            if sources.insert(item.project_id, source).is_some() {
                return Err(Error::Conflict);
            }
        }
        Ok(Self {
            root,
            sources,
            binary,
            jobs: Mutex::new(BTreeMap::new()),
        })
    }
    /// Host credential/SDK custody must not be a project or managed home tree.
    pub fn separate_from(&self, path: &Path) -> Result<(), Error> {
        let path = path.canonicalize()?;
        disjoint(&self.root.path, &path)?;
        for source in self.sources.values() {
            disjoint(&source.root.path, &path)?;
        }
        Ok(())
    }
    /// Reopen the home a completed provision already created, after a restart.
    /// It creates and writes nothing: every path is derived from the same facts
    /// as the original creation, and the recorded binding and manifest must
    /// still agree with a binding recomputed now. A missing, partial or foreign
    /// home is refused rather than repaired.
    pub fn reopen(
        &self,
        scope: &crate::OwnedProvisionScope,
        effect: &Effect,
        registration: &Registration,
    ) -> Result<Arc<ManagedAgentHome>, Error> {
        let request: ProjectRequest = serde_json::from_value(
            effect
                .payload
                .get("request")
                .ok_or(Error::Conflict)?
                .clone(),
        )?;
        request.validate(registration)?;
        if effect.engagement_id != request.engagement_id()?
            || effect.engagement_id != scope.engagement_id()
        {
            return Err(Error::Conflict);
        }
        let source = self
            .sources
            .get(&request.target_project_id)
            .cloned()
            .ok_or(Error::NotFound)?;
        let provision = canonical::transport_digest(&json!([effect, registration]))?;
        let binding = canonical::transport_digest(
            &json!({"kind":"native-agent-home-v1","effect":effect,"registration":registration,
            "root":self.root.path,"source":source.root.path,"mode":source.mode,"binary":self.binary.path,
            "binary_length":self.binary.length,"binary_modified":self.binary.modified}),
        )?;
        let mut jobs = self.jobs.lock().map_err(|_| Error::OutcomeUnknown)?;
        if jobs.contains_key(&effect.id) {
            return Err(Error::Busy);
        }
        if jobs.len() >= JOBS {
            return Err(Error::Capacity);
        }
        let home = self
            .root
            .path
            .join("agents")
            .join(format!("agent_{}", effect.engagement_id));
        let workdir = home.join("workdir");
        let project_path = workdir.join("projects").join(&request.target_project_id);
        let custody = self
            .root
            .path
            .join("custody")
            .join(format!("home-{}", effect.engagement_id));
        let manifest = String::from_utf8(bounded(&custody.join("complete"), 64)?)
            .map_err(|_| Error::Private)?;
        let reopened = Arc::new(ManagedAgentHome {
            project_root: match source.mode {
                ProjectMode::Copy => Some(Root::open(project_path.clone(), true)?),
                ProjectMode::Symlink => None,
            },
            home: Root::open(home, true)?,
            workdir: Root::open(workdir, true)?,
            custody: Root::open(custody, true)?,
            root: self.root.clone(),
            source,
            binary: self.binary.clone(),
            project_path,
            binding: binding.clone(),
            manifest,
            resource: canonical::transport_digest(&json!(scope.resource()))?,
            provision,
        });
        reopened.check_provision_scope(scope)?;
        jobs.insert(
            effect.id.clone(),
            Arc::new(Job {
                binding,
                busy: Arc::new(Semaphore::new(1)),
                result: Mutex::new(Some(Ok(reopened.clone()))),
                home: Mutex::new(Some(reopened.clone())),
            }),
        );
        Ok(reopened)
    }
    pub async fn materialize(
        &self,
        domain: DomainStore,
        effect: Effect,
        registration: Registration,
        deadline: Instant,
        cancel: Arc<AtomicBool>,
    ) -> Result<Arc<ManagedAgentHome>, Error> {
        checkpoint(deadline, &cancel)?;
        let request: ProjectRequest = serde_json::from_value(
            effect
                .payload
                .get("request")
                .ok_or(Error::Conflict)?
                .clone(),
        )?;
        request.validate(&registration)?;
        if effect.engagement_id != request.engagement_id()? {
            return Err(Error::Conflict);
        }
        let source = self
            .sources
            .get(&request.target_project_id)
            .cloned()
            .ok_or(Error::NotFound)?;
        let provision = canonical::transport_digest(&json!([effect, registration]))?;
        let binding = canonical::transport_digest(
            &json!({"kind":"native-agent-home-v1","effect":effect,"registration":registration,
            "root":self.root.path,"source":source.root.path,"mode":source.mode,"binary":self.binary.path,
            "binary_length":self.binary.length,"binary_modified":self.binary.modified}),
        )?;
        let job = {
            let mut jobs = self.jobs.lock().map_err(|_| Error::OutcomeUnknown)?;
            if let Some(job) = jobs.get(&effect.id) {
                if job.binding != binding {
                    return Err(Error::Conflict);
                }
                match job
                    .result
                    .lock()
                    .map_err(|_| Error::OutcomeUnknown)?
                    .as_ref()
                {
                    Some(Ok(_)) => {}
                    Some(Err(error)) => return Err(error.error()),
                    None => return Err(Error::Busy),
                }
                job.clone()
            } else {
                if jobs.len() >= JOBS {
                    return Err(Error::Capacity);
                }
                let job = Arc::new(Job {
                    binding: binding.clone(),
                    busy: Arc::new(Semaphore::new(1)),
                    result: Mutex::new(None),
                    home: Mutex::new(None),
                });
                jobs.insert(effect.id.clone(), job.clone());
                job
            }
        };
        let permit = job
            .busy
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Busy)?;
        let root = self.root.clone();
        let binary = self.binary.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let result = async {
                domain
                    .validate_provision_account(effect.clone(), registration.clone())
                    .await?;
                let resource_id = request.agent_definition.resource_id.clone();
                let resource = domain.resource_configuration(resource_id.clone()).await?;
                if !resource.qualifies(&request.role) {
                    return Err(Error::Unqualified);
                }
                domain
                    .validate_provision_account(effect.clone(), registration.clone())
                    .await?;
                checkpoint(deadline, &cancel)?;
                let known = job
                    .result
                    .lock()
                    .map_err(|_| Error::OutcomeUnknown)?
                    .as_ref()
                    .and_then(|r| r.as_ref().ok())
                    .cloned();
                let binding = job.binding.clone();
                let physical_cancel = cancel.clone();
                let physical = tokio::task::spawn_blocking(move || {
                    if let Some(known) = known {
                        known.check()?;
                        if known.resource != canonical::transport_digest(&json!(&resource))? {
                            return Err(Error::Conflict);
                        }
                        checkpoint(deadline, &physical_cancel)?;
                        return Ok(known);
                    }
                    create(Creation {
                        root,
                        source,
                        binary,
                        request,
                        resource,
                        binding,
                        provision,
                        deadline,
                        cancel: physical_cancel,
                    })
                })
                .await
                .map_err(|_| Error::OutcomeUnknown)??;
                *job.home.lock().map_err(|_| Error::OutcomeUnknown)? = Some(physical.clone());
                let current = domain.resource_configuration(resource_id).await?;
                if physical.resource != canonical::transport_digest(&json!(current))? {
                    return Err(Error::Conflict);
                }
                domain
                    .validate_provision_account(effect.clone(), registration)
                    .await?;
                checkpoint(deadline, &cancel)?;
                Ok(physical)
            }
            .await;
            let result = result.map_err(|error| ErrorKind::from(&error));
            if result.is_err() {
                let _ = domain
                    .observe_effect(effect.id, effect.fence, EffectOutcome::Unknown)
                    .await;
            }
            *job.result.lock().map_err(|_| Error::OutcomeUnknown)? = Some(result.clone());
            result.map_err(ErrorKind::error)
        })
        .await
        .map_err(|_| Error::OutcomeUnknown)?
    }
}
struct Creation {
    root: Arc<Root>,
    source: Arc<Source>,
    binary: Arc<Binary>,
    request: ProjectRequest,
    resource: project::Resource,
    binding: String,
    provision: String,
    deadline: Instant,
    cancel: Arc<AtomicBool>,
}
fn create(creation: Creation) -> Result<Arc<ManagedAgentHome>, Error> {
    let Creation {
        root,
        source,
        binary,
        request,
        resource,
        binding,
        provision,
        deadline,
        cancel,
    } = creation;
    let request = &request;
    let resource = &resource;
    let binding = binding.as_str();
    let cancel = cancel.as_ref();
    checkpoint(deadline, cancel)?;
    root.check()?;
    source.root.check()?;
    binary.check()?;
    let agents = root.path.join("agents");
    private::directory(&agents)?;
    let custody = root.path.join("custody");
    private::directory(&custody)?;
    let custody = custody.join(format!("home-{}", request.engagement_id()?));
    private::create_directory_new(&custody)?;
    private::write_new(&custody.join("possible"), binding.as_bytes())?;
    checkpoint(deadline, cancel)?;
    let id = format!("agent_{}", request.engagement_id()?);
    let home = agents.join(&id);
    private::create_directory_new(&home)?;
    for path in [
        "state",
        "state/locks",
        "state/history",
        "state/tmp",
        "workdir",
        "workdir/docs",
        "workdir/data",
        "workdir/projects",
        "supervisor",
        "supervisor/docs",
    ] {
        checkpoint(deadline, cancel)?;
        private::create_directory_new(&home.join(path))?;
    }
    private::write_new(&home.join("state/home-binding"), binding.as_bytes())?;
    let workdir = home.join("workdir");
    let project_path = workdir.join("projects").join(&request.target_project_id);
    match source.mode {
        ProjectMode::Copy => {
            private::create_directory_new(&project_path)?;
            let target = Dir::open_ambient_dir(&project_path, ambient_authority())?;
            let mut remaining = CopyBudget {
                entries: ENTRIES,
                bytes: TOTAL_BYTES,
                deadline,
                cancel,
            };
            copy(&source.root.dir, &target, Path::new("."), 0, &mut remaining)?;
        }
        ProjectMode::Symlink => link(&source.root.path, &project_path, true)?,
    }
    source.root.check()?;
    root.check()?;
    checkpoint(deadline, cancel)?;
    let mode = match source.mode {
        ProjectMode::Copy => "copy",
        ProjectMode::Symlink => "symlink",
    };
    let mapping = json!({"name":request.target_project_id,"path":project_path,"source":mode,"originPath":source.root.path});
    let manifest = json!({"id":id,"name":request.agent_definition.name,"type":resource.framework,"agentModelVersion":"1.1",
        "layoutVersion":1,"homeDir":home,"workdir":workdir,"stateDir":home.join("state"),"managedProjects":[mapping],
        "human":{"owner":request.owner_mxid},"task":null,"runtimeProfile":{"primary":{"framework":resource.framework,
            "provider":resource.provider,"model":resource.model,"reasoning":resource.reasoning}},"nativeProvisionBinding":binding});
    let bytes = serde_json::to_vec_pretty(&manifest)?;
    if bytes.len() > 32 * 1024 {
        return Err(Error::Capacity);
    }
    let docs = render(CLAUDE, request, &id);
    let agent_docs = render(AGENTS, request, &id);
    for (path,content) in [("workdir/CLAUDE.md",docs),("workdir/AGENTS.md",agent_docs),
        ("supervisor/CLAUDE.md",render(SUPERVISOR_CLAUDE,request,&id)),("supervisor/AGENTS.md",render(SUPERVISOR_AGENTS,request,&id)),
        ("workdir/docs/agent-knowledge.md","# Agent Knowledge\n\nRecord durable reusable knowledge, not canonical task state.\n".into()),
        ("workdir/docs/plan.md","## Current\nRead the assigned canonical task through the native task/MCP tools.\n".into()),
        ("workdir/docs/progress.md",String::new()),("supervisor/docs/plan.md","## Current\nObserve shared canonical task state; keep only supervisor-local notes here.\n".into()),
        ("supervisor/docs/progress.md",String::new())] {checkpoint(deadline,cancel)?;private::write_new(&home.join(path),content.as_bytes())?;}
    let projection = json!([{"name":request.target_project_id,"workdirPath":format!("projects/{}",request.target_project_id),
        "path":project_path,"source":mode,"originPath":source.root.path}]);
    let projects = format!(
        "# Projects\n\n<!-- hagency-managed-projects:start -->\n## Provisioned project mappings\n\nProjects are relative to workdir/. Copy edits stay in the managed copy; symlink edits affect originPath.\nBindings come from agent.json, not manual task state.\n\n```json\n{}\n```\n<!-- hagency-managed-projects:end -->\n",
        serde_json::to_string_pretty(&projection)?
    );
    private::write_new(&workdir.join("docs/projects.md"), projects.as_bytes())?;
    link(
        Path::new("../CLAUDE.md"),
        &workdir.join("docs/CLAUDE.md"),
        false,
    )?;
    link(
        Path::new("../AGENTS.md"),
        &workdir.join("docs/AGENTS.md"),
        false,
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let wrapper = format!(
            "#!/bin/sh\n# hagency-task-writer-wrapper: native-v1\nexec '{}' task \"$@\"\n",
            binary.path.display()
        );
        let path = workdir.join("task-writer");
        private::write_new(&path, wrapper.as_bytes())?;
        let file = private::open(&path, false)?;
        file.set_permissions(std::fs::Permissions::from_mode(0o700))?;
        file.sync_all()?;
    }
    #[cfg(windows)]
    {
        let wrapper = format!(
            "@echo off\r\nrem hagency-task-writer-wrapper: native-v1\r\n\"{}\" task %*\r\n",
            binary.path.display()
        );
        private::write_new(&workdir.join("task-writer.cmd"), wrapper.as_bytes())?;
    }
    private::write_new(&home.join("agent.json"), &bytes)?;
    let project_root = match source.mode {
        ProjectMode::Copy => Some(Root::open(project_path.clone(), true)?),
        ProjectMode::Symlink => None,
    };
    let created = Arc::new(ManagedAgentHome {
        home: Root::open(home, true)?,
        workdir: Root::open(workdir, true)?,
        custody: Root::open(custody.clone(), true)?,
        root,
        source,
        binary,
        project_root,
        project_path,
        binding: binding.into(),
        manifest: project::hash(&bytes),
        resource: canonical::transport_digest(&json!(resource))?,
        provision,
    });
    checkpoint(deadline, cancel)?;
    private::write_new(&custody.join("complete"), project::hash(&bytes).as_bytes())?;
    created.check()?;
    checkpoint(deadline, cancel)?;
    Ok(created)
}
fn render(template: &str, request: &ProjectRequest, id: &str) -> String {
    template
        .replace("{{AGENT_NAME}}", request.agent_definition.name.as_str())
        .replace("{{AGENT_ID}}", id)
        .replace("{{LAYOUT_VERSION}}", "1")
        .replace(
            "(`agent.json` plus compatibility/backend sync)",
            "(native DomainStore and scoped task/MCP API)",
        )
        .replace(
            "start a new batch: `./task-writer start --id <task-id>`",
            "heartbeat the assigned task: `./task-writer start`",
        )
        + "\nNative authority: canonical tasks and effective runtime authority live in the native DomainStore/scoped API, not agent.json or these notes. The manifest is the physical home mapping and selected resource projection only.\n"
}
struct CopyBudget<'a> {
    entries: usize,
    bytes: usize,
    deadline: Instant,
    cancel: &'a AtomicBool,
}
fn copy(
    source: &Dir,
    target: &Dir,
    path: &Path,
    depth: usize,
    budget: &mut CopyBudget<'_>,
) -> Result<(), Error> {
    checkpoint(budget.deadline, budget.cancel)?;
    if depth > DEPTH {
        return Err(Error::Capacity);
    }
    for entry in source.read_dir(path)? {
        checkpoint(budget.deadline, budget.cancel)?;
        budget.entries = budget.entries.checked_sub(1).ok_or(Error::Capacity)?;
        let entry = entry?;
        let relative = path.join(entry.file_name());
        let kind = entry.file_type()?;
        if kind.is_dir() {
            let builder = cap_std::fs::DirBuilder::new();
            #[cfg(unix)]
            let builder = {
                use cap_std::fs::DirBuilderExt;
                let mut builder = builder;
                builder.mode(0o700);
                builder
            };
            // Create private, rather than trying fchmod on cap-std's Linux
            // O_PATH handle (EBADF) after briefly creating a public directory.
            target.create_dir_with(&relative, &builder)?;
            let file = target.open_dir(&relative)?.into_std_file();
            private::check_handle(&file)?;
            copy(source, target, &relative, depth + 1, budget)?;
        } else if kind.is_file() {
            let mut input = source.open(&relative)?;
            let size = usize::try_from(input.metadata()?.len()).map_err(|_| Error::Capacity)?;
            if size > FILE_BYTES || size > budget.bytes {
                return Err(Error::Capacity);
            }
            let mut bytes = Vec::with_capacity(size);
            Read::by_ref(&mut input)
                .take((FILE_BYTES + 1) as u64)
                .read_to_end(&mut bytes)?;
            if bytes.len() > FILE_BYTES {
                return Err(Error::Capacity);
            }
            budget.bytes = budget
                .bytes
                .checked_sub(bytes.len())
                .ok_or(Error::Capacity)?;
            checkpoint(budget.deadline, budget.cancel)?;
            let mut options = cap_std::fs::OpenOptions::new();
            options.read(true).write(true).create_new(true);
            #[cfg(unix)]
            {
                use cap_std::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut output = target.open_with(&relative, &options)?.into_std();
            private::seal_created_file_handle(&output)?;
            output.write_all(&bytes)?;
            output.sync_all()?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::{MetadataExt, PermissionsExt};
                if input.try_clone()?.into_std().metadata()?.mode() & 0o100 != 0 {
                    output.set_permissions(std::fs::Permissions::from_mode(0o700))?;
                    output.sync_all()?;
                }
            }
        } else if kind.is_symlink() {
            let link_target = source.read_link(&relative)?;
            if link_target.is_absolute() {
                return Err(Error::Private);
            }
            // Resolving through the retained source capability refuses external
            // links. Keep only the original safe relative link in the copy.
            source.canonicalize(&relative)?;
            let parent = relative.parent().ok_or(Error::Private)?;
            let target_root = target.try_clone()?.into_std_file();
            private::check_handle(&target_root)?;
            #[cfg(unix)]
            {
                target.symlink(&link_target, &relative)?;
            }
            #[cfg(windows)]
            {
                if source.metadata(parent.join(&link_target))?.is_dir() {
                    target.symlink_dir(&link_target, &relative)?;
                } else {
                    target.symlink_file(&link_target, &relative)?;
                }
            }
            #[cfg(unix)]
            let _ = parent;
        } else {
            return Err(Error::Private);
        }
    }
    // Each recursive directory owns new entries. Syncing only the copy root
    // cannot establish durability of nested names, even after file fsync.
    private::sync_directory(&target.open_dir(path)?)?;
    checkpoint(budget.deadline, budget.cancel)
}
fn link(source: &Path, target: &Path, is_dir: bool) -> Result<(), Error> {
    #[cfg(unix)]
    {
        let _ = is_dir;
        std::os::unix::fs::symlink(source, target)?;
    }
    #[cfg(windows)]
    {
        if is_dir {
            std::os::windows::fs::symlink_dir(source, target)?;
        } else {
            std::os::windows::fs::symlink_file(source, target)?;
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (source, target, is_dir);
        return Err(Error::PlatformUnavailable);
    }
    Ok(())
}
fn bounded(path: &Path, cap: usize) -> Result<Vec<u8>, Error> {
    let file = private::open(path, false)?;
    if file.metadata()?.len() > cap as u64 {
        return Err(Error::Capacity);
    }
    let mut bytes = Vec::new();
    file.take((cap + 1) as u64).read_to_end(&mut bytes)?;
    if bytes.len() > cap {
        return Err(Error::Capacity);
    }
    Ok(bytes)
}
fn path_valid(path: &Path) -> Result<(), Error> {
    if !path.is_absolute()
        || path.as_os_str().as_encoded_bytes().len() > 4096
        || path
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
    {
        return Err(Error::Private);
    }
    Ok(())
}
fn disjoint(first: &Path, second: &Path) -> Result<(), Error> {
    if first.starts_with(second) || second.starts_with(first) {
        return Err(Error::Private);
    }
    Ok(())
}
fn checkpoint(deadline: Instant, cancel: &AtomicBool) -> Result<(), Error> {
    if cancel.load(Ordering::Acquire) || Instant::now() >= deadline {
        return Err(Error::OutcomeUnknown);
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::MetadataExt;
    #[cfg(target_os = "linux")]
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn native_agent_home_nested_copy_private() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        let target = root.path().join("target");
        private::directory(&source).unwrap();
        private::directory(&target).unwrap();
        private::directory(&source.join("nested")).unwrap();
        private::directory(&source.join("nested/deeper")).unwrap();
        private::write_new(
            &source.join("nested/deeper/文件.md"),
            b"original nested bytes",
        )
        .unwrap();
        let source = Dir::open_ambient_dir(&source, ambient_authority()).unwrap();
        let target = Dir::open_ambient_dir(&target, ambient_authority()).unwrap();
        // Physical Linux evidence of the old failure; never turn EBADF into a
        // successful permission operation or adopt a public replacement path.
        #[cfg(target_os = "linux")]
        assert_eq!(
            target
                .try_clone()
                .unwrap()
                .into_std_file()
                .set_permissions(std::fs::Permissions::from_mode(0o700))
                .unwrap_err()
                .raw_os_error(),
            Some(9)
        );
        let cancel = AtomicBool::new(false);
        let mut budget = CopyBudget {
            entries: ENTRIES,
            bytes: TOTAL_BYTES,
            deadline: Instant::now() + std::time::Duration::from_secs(3),
            cancel: &cancel,
        };
        copy(&source, &target, Path::new("."), 0, &mut budget).unwrap();
        assert_eq!(
            target.read("nested/deeper/文件.md").unwrap(),
            b"original nested bytes"
        );
        for path in ["nested", "nested/deeper"] {
            let directory = target.open_dir(path).unwrap();
            let held = directory.try_clone().unwrap().into_std_file();
            private::check_handle(&held).unwrap();
            assert_eq!(held.metadata().unwrap().mode() & 0o777, 0o700);
            private::sync_directory(&directory).unwrap();
        }
        assert_eq!(budget.entries, ENTRIES - 3);
        assert_eq!(budget.bytes, TOTAL_BYTES - b"original nested bytes".len());
    }
}
