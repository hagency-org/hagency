//! Retained roots under the host-exclusive, stable-ancestor provisioning
//! contract. The fixed runtime path is not safe against hostile same-UID
//! namespace manipulation. Comparison detects changes; handles own objects.
mod approval_path;
mod received;
pub use approval_path::ordinary_launch_path;
pub use received::{WorkspaceReceive, WorkspaceReceiveError};

use crate::Failure;
use cap_std::{ambient_authority, fs::Dir};
use hagency_core::{canonical, project, tasks::RunnerCapability};
use hagency_files::{RelativeFile, Snapshot, Workspace};
use hagency_store::{DomainStore, OwnedDispatchScope, private};
use std::{
    collections::BTreeMap,
    fs::File,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

pub(crate) const MAX_FILE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum WorkspaceError {
    #[error("workspace belongs to another capability")]
    Authority,
    #[error("workspace access is retired")]
    Retired,
    #[error("host workspace configuration is unavailable or changed")]
    Root,
    #[error("workspace copy profile exceeds the requested bound")]
    Limit,
    #[error("current workspace authority could not be confirmed")]
    Current,
    #[error("workspace snapshot refused: {0}")]
    Snapshot(hagency_files::Error),
}

pub(crate) struct Root {
    file: File,
    path: PathBuf,
    files: Workspace,
    limit: usize,
}
impl Root {
    fn open(path: PathBuf) -> Result<Self, Failure> {
        if path.as_os_str().as_encoded_bytes().len() > 4096
            || !path.is_absolute()
            || path.canonicalize().ok().as_ref() != Some(&path)
        {
            return Err(Failure::Admission);
        }
        let dir =
            Dir::open_ambient_dir(&path, ambient_authority()).map_err(|_| Failure::Admission)?;
        Self::from_file(path, dir.into_std_file(), MAX_FILE_BYTES)
    }
    fn from_file(path: PathBuf, file: File, limit: usize) -> Result<Self, Failure> {
        private::check_handle(&file).map_err(|_| Failure::Admission)?;
        let limits = hagency_files::Limits::new(limit, 4).map_err(|_| Failure::Admission)?;
        let files = Workspace::from_directory(
            Dir::from_std_file(file.try_clone().map_err(|_| Failure::Admission)?),
            limits,
        )
        .map_err(|_| Failure::Admission)?;
        let root = Self {
            file,
            path,
            files,
            limit,
        };
        root.check().map_err(|_| Failure::Admission)?;
        Ok(root)
    }
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
    pub(crate) fn check(&self) -> Result<(), WorkspaceError> {
        private::check_handle(&self.file).map_err(|_| WorkspaceError::Root)?;
        if self.path.canonicalize().ok().as_ref() != Some(&self.path) {
            return Err(WorkspaceError::Root);
        }
        // Reopening here is ONLY a consistency check; source selection always
        // uses self.files, built from a duplicate of the original held object.
        let current = Dir::open_ambient_dir(&self.path, ambient_authority())
            .map_err(|_| WorkspaceError::Root)?
            .into_std_file();
        private::check_handle(&current).map_err(|_| WorkspaceError::Root)?;
        if !hagency_platform::same_directory(&self.file, &current)
            .map_err(|_| WorkspaceError::Root)?
        {
            return Err(WorkspaceError::Root);
        }
        Ok(())
    }
}

pub(crate) struct Workspaces(BTreeMap<String, Arc<Root>>);
impl Workspaces {
    pub(crate) fn open(paths: BTreeMap<String, PathBuf>) -> Result<Self, Failure> {
        if paths.is_empty() || paths.len() > 16 {
            return Err(Failure::Admission);
        }
        let mut roots: BTreeMap<String, Arc<Root>> = BTreeMap::new();
        for (id, path) in paths {
            project::identifier(&id, 128).map_err(|_| Failure::Admission)?;
            let root = Root::open(path)?;
            for prior in roots.values() {
                // Alias comparison observes both live objects; canonical path
                // nesting is a separate fixed-host configuration refusal. This
                // is not global mount-topology or cross-Host isolation proof.
                if hagency_platform::same_directory(&root.file, &prior.file)
                    .map_err(|_| Failure::Admission)?
                    || root.path.starts_with(&prior.path)
                    || prior.path.starts_with(&root.path)
                {
                    return Err(Failure::Admission);
                }
            }
            roots.insert(id, Arc::new(root));
        }
        Ok(Self(roots))
    }
    pub(crate) fn first_path(&self) -> Result<&Path, Failure> {
        self.0
            .values()
            .next()
            .map(|root| root.path())
            .ok_or(Failure::Admission)
    }
    pub(crate) fn get(&self, id: &str) -> Result<Arc<Root>, Failure> {
        self.0.get(id).cloned().ok_or(Failure::Admission)
    }
    pub(crate) fn limit(self, limit: usize) -> Result<Self, Failure> {
        if limit == 0 || limit > MAX_FILE_BYTES {
            return Err(Failure::Admission);
        }
        let mut roots = BTreeMap::new();
        for (id, root) in self.0 {
            let file = root.file.try_clone().map_err(|_| Failure::Admission)?;
            roots.insert(
                id,
                Arc::new(Root::from_file(root.path.clone(), file, limit)?),
            );
        }
        Ok(Self(roots))
    }
}

fn capability_digest(cap: &RunnerCapability) -> Result<String, WorkspaceError> {
    if project::identifier(&cap.dispatch_id, 128).is_err()
        || project::identifier(&cap.runner_id, 128).is_err()
        || cap.fence == 0
        || cap.secret.len() != 64
        || !cap
            .secret
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(WorkspaceError::Authority);
    }
    canonical::digest(&serde_json::json!(cap)).map_err(|_| WorkspaceError::Authority)
}

pub(crate) struct Binding {
    root: Arc<Root>,
    domain: DomainStore,
    capability: String,
    fingerprint: String,
    cancel: Arc<AtomicBool>,
    retired: AtomicBool,
}
impl Binding {
    pub(crate) fn start(
        root: Arc<Root>,
        domain: DomainStore,
        cap: &RunnerCapability,
        scope: &OwnedDispatchScope,
        cancel: Arc<AtomicBool>,
    ) -> Result<Arc<Self>, Failure> {
        // Only operation::execute calls this, immediately after its successful
        // original Started receipt. No public scope/JSON constructor exists.
        root.check().map_err(|_| Failure::Admission)?;
        Ok(Arc::new(Self {
            root,
            domain,
            capability: capability_digest(cap).map_err(|_| Failure::Admission)?,
            fingerprint: scope.fingerprint().into(),
            cancel,
            retired: AtomicBool::new(false),
        }))
    }
    fn live(&self) -> Result<(), WorkspaceError> {
        if self.retired.load(Ordering::Acquire) || self.cancel.load(Ordering::Acquire) {
            Err(WorkspaceError::Retired)
        } else {
            Ok(())
        }
    }
    fn check(&self, cap: &RunnerCapability) -> Result<(), WorkspaceError> {
        self.live()?;
        if capability_digest(cap)? != self.capability {
            return Err(WorkspaceError::Authority);
        }
        self.root.check()
    }
    pub(crate) fn check_root(&self) -> Result<(), WorkspaceError> {
        self.live()?;
        self.root.check()
    }
    pub(crate) fn retirement(self: &Arc<Self>) -> Retirement {
        Retirement(self.clone())
    }
    pub(crate) fn handoff(self: &Arc<Self>) -> StartedWorkspace {
        StartedWorkspace {
            binding: self.clone(),
        }
    }
}
pub(crate) struct Retirement(Arc<Binding>);
impl Drop for Retirement {
    fn drop(&mut self) {
        self.0.retired.store(true, Ordering::Release);
    }
}
pub(crate) type Handoff = Arc<Mutex<Option<StartedWorkspace>>>;

/// One original successful Started association and retained physical root.
/// No raw Workspace or handle escapes. This is not fresh domain authority:
/// the host file worker must validate_current immediately before snapshot and
/// again before publishing. No operation can restore a lost handoff.
pub struct StartedWorkspace {
    binding: Arc<Binding>,
}
impl StartedWorkspace {
    pub async fn validate_current(&self, cap: &RunnerCapability) -> Result<(), WorkspaceError> {
        self.binding.check(cap)?;
        self.binding
            .domain
            .check_owned_dispatch(cap.clone(), self.binding.fingerprint.clone())
            .await
            .map_err(|_| WorkspaceError::Current)?;
        self.binding.check(cap)
    }
    /// Synchronous bounded copy; filesystem calls are not cancellable. The
    /// accepted copy profile must already fit the caller's configured limit,
    /// so a smaller caller limit never reads up to a larger default first.
    /// This checks original association/liveness, not current domain leases.
    pub fn snapshot(
        &self,
        cap: &RunnerCapability,
        selection: &RelativeFile,
        requested_limit: usize,
    ) -> Result<Snapshot, WorkspaceError> {
        self.binding.check(cap)?;
        if requested_limit == 0
            || requested_limit > MAX_FILE_BYTES
            || self.binding.root.limit > requested_limit
        {
            return Err(WorkspaceError::Limit);
        }
        let snapshot = self
            .binding
            .root
            .files
            .snapshot(selection)
            .map_err(WorkspaceError::Snapshot)?;
        self.binding.check(cap)?;
        Ok(snapshot)
    }
}
