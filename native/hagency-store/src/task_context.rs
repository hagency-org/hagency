//! One create-only operational dispatch context, never runtime readiness.
use crate::{DomainStore, Error, OwnedDispatchScope, private};
use cap_std::{ambient_authority, fs::Dir};
use hagency_core::{
    canonical,
    tasks::{RunnerCapability, TaskState},
};
use serde_json::json;
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

const BYTES: usize = 4096;
pub struct RetainedTaskContext {
    root: PathBuf,
    dir: Dir,
    path: PathBuf,
    id: String,
    job: Mutex<Option<Arc<Job>>>,
}
struct Job {
    scope: String,
    capability: String,
    deadline: Instant,
    cancel: Arc<AtomicBool>,
    record: Mutex<Option<(File, String)>>,
    result: Mutex<Option<Result<(), Kind>>>,
}
#[derive(Clone, Copy)]
enum Kind {
    Unknown,
    Authority,
    Private,
    Conflict,
}
impl Kind {
    fn error(self) -> Error {
        match self {
            Self::Unknown => Error::OutcomeUnknown,
            Self::Authority => Error::RunnerAuthority,
            Self::Private => Error::Private,
            Self::Conflict => Error::Conflict,
        }
    }
    fn from(error: &Error) -> Self {
        match error {
            Error::RunnerAuthority | Error::State | Error::Generation | Error::Quarantined => {
                Self::Authority
            }
            Error::Private => Self::Private,
            Error::Conflict => Self::Conflict,
            _ => Self::Unknown,
        }
    }
}
/// Read-only structural root check used by the native helper. It grants no scope.
pub fn validate_root(path: &Path) -> Result<(), Error> {
    if !path.is_absolute()
        || path.to_str().is_none()
        || path.as_os_str().as_encoded_bytes().len() > 3500
        || path.canonicalize()? != path
    {
        return Err(Error::Private);
    }
    let dir = Dir::open_ambient_dir(path, ambient_authority())?;
    private::check_handle(&dir.into_std_file())
}
impl RetainedTaskContext {
    pub fn new(root: PathBuf, context_id: &str) -> Result<Arc<Self>, Error> {
        validate_root(&root)?;
        if context_id.len() != 64
            || !context_id
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(Error::Conflict);
        }
        let path = root.join(format!("context-{context_id}.json"));
        match std::fs::symlink_metadata(&path) {
            Ok(_) => return Err(Error::Conflict),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        let dir = Dir::open_ambient_dir(&root, ambient_authority())?;
        let context = Arc::new(Self {
            root,
            dir,
            path,
            id: context_id.into(),
            job: Mutex::new(None),
        });
        context.check_root()?;
        Ok(context)
    }
    fn check_root(&self) -> Result<(), Error> {
        validate_root(&self.root)?;
        let held = self.dir.try_clone()?.into_std_file();
        private::check_handle(&held)?;
        let current = Dir::open_ambient_dir(&self.root, ambient_authority())?.into_std_file();
        if !hagency_platform::same_directory(&held, &current)? {
            return Err(Error::Private);
        }
        Ok(())
    }
    pub fn separate_from(&self, path: &Path) -> Result<(), Error> {
        self.check_root()?;
        let path = path.canonicalize()?;
        if path.starts_with(&self.root) || self.root.starts_with(&path) {
            return Err(Error::Private);
        }
        Ok(())
    }
    /// Only the fixed reference/marker, not a capability value, enters the parent.
    pub fn apply_environment(
        &self,
        environment: &mut BTreeMap<OsString, OsString>,
    ) -> Result<(), Error> {
        self.check_root()?;
        let reference = serde_json::to_string(
            &json!({"profile":"retained_task_context_v1","path":self.path,"context_id":self.id}),
        )?;
        if reference.len() > BYTES {
            return Err(Error::Private);
        }
        environment.insert("HAGENCY_RUNNER_CAPABILITY".into(), reference.into());
        environment.insert(
            "HAGENCY_TASK_ID".into(),
            format!("context_{}", self.id).into(),
        );
        Ok(())
    }
    fn check_record(&self, job: &Job) -> Result<(), Error> {
        self.check_root()?;
        let held = job.record.lock().map_err(|_| Error::OutcomeUnknown)?;
        let (file, hash) = held.as_ref().ok_or(Error::OutcomeUnknown)?;
        private::check_handle(file)?;
        let current = private::open(&self.path, false)?;
        if !hagency_platform::same_file(file, &current)? || current.metadata()?.len() > BYTES as u64
        {
            return Err(Error::Private);
        }
        let mut bytes = Vec::new();
        current.take(BYTES as u64 + 1).read_to_end(&mut bytes)?;
        if bytes.len() > BYTES || canonical::transport_digest(&json!(bytes))? != *hash {
            return Err(Error::Conflict);
        }
        Ok(())
    }
    /// One retained admitted task; receiving or storing this acknowledgement
    /// creates no runtime, task, approval, completion or route authority.
    pub async fn bind(
        self: &Arc<Self>,
        domain: DomainStore,
        cap: RunnerCapability,
        started: OwnedDispatchScope,
        deadline: Instant,
        cancel: Arc<AtomicBool>,
    ) -> Result<(), Error> {
        checkpoint(deadline, &cancel)?;
        let hash = canonical::transport_digest(&json!(cap))?;
        let (job, known) = {
            let mut slot = self.job.lock().map_err(|_| Error::OutcomeUnknown)?;
            if let Some(job) = slot.as_ref() {
                if job.scope != started.fingerprint()
                    || job.capability != hash
                    || job.deadline != deadline
                    || !Arc::ptr_eq(&job.cancel, &cancel)
                {
                    return Err(Error::Conflict);
                }
                match *job.result.lock().map_err(|_| Error::OutcomeUnknown)? {
                    None => return Err(Error::Busy),
                    Some(Err(error)) => return Err(error.error()),
                    Some(Ok(())) => {}
                }
                (job.clone(), true)
            } else {
                let job = Arc::new(Job {
                    scope: started.fingerprint().into(),
                    capability: hash,
                    deadline,
                    cancel: cancel.clone(),
                    record: Mutex::new(None),
                    result: Mutex::new(None),
                });
                *slot = Some(job.clone());
                (job, false)
            }
        };
        let context = self.clone();
        tokio::spawn(async move {
            let result = async {
                started.check_started(&cap)?;
                if started.task().status != TaskState::InProgress
                    || started.input().id != cap.dispatch_id
                {
                    return Err(Error::RunnerAuthority);
                }
                current(&domain, &cap, &job.scope, deadline, &cancel).await?;
                let body = serde_json::to_vec(
                    &json!({"version":1,"context_id":context.id,"scope":job.scope,
                    "task_id":started.task().id,"capability":cap}),
                )?;
                if body.len() > BYTES {
                    return Err(Error::Capacity);
                }
                let physical = context.clone();
                let observed = job.clone();
                let physical_cancel = cancel.clone();
                tokio::task::spawn_blocking(move || {
                    checkpoint(deadline, &physical_cancel)?;
                    physical.check_root()?;
                    if !known {
                        let file = private::open(&physical.path, true)?;
                        let mut writer = file.try_clone()?;
                        let digest = canonical::transport_digest(&json!(body))?;
                        // Keep the actual create-only descriptor before any write.
                        // A partial write remains original custody, never adoption.
                        *observed.record.lock().map_err(|_| Error::OutcomeUnknown)? =
                            Some((file, digest));
                        writer.write_all(&body)?;
                        writer.sync_all()?;
                        private::sync_directory(&physical.dir)?;
                    }
                    physical.check_record(&observed)
                })
                .await
                .map_err(|_| Error::OutcomeUnknown)??;
                current(&domain, &cap, &job.scope, deadline, &cancel).await
            }
            .await;
            let result = result.map_err(|error| Kind::from(&error));
            *job.result.lock().map_err(|_| Error::OutcomeUnknown)? = Some(result);
            result.map_err(Kind::error)
        })
        .await
        .map_err(|_| Error::OutcomeUnknown)?
    }
}
fn checkpoint(deadline: Instant, cancel: &AtomicBool) -> Result<(), Error> {
    if cancel.load(Ordering::Acquire) || Instant::now() >= deadline {
        return Err(Error::OutcomeUnknown);
    }
    Ok(())
}
async fn current(
    domain: &DomainStore,
    cap: &RunnerCapability,
    scope: &str,
    deadline: Instant,
    cancel: &AtomicBool,
) -> Result<(), Error> {
    checkpoint(deadline, cancel)?;
    tokio::time::timeout_at(
        tokio::time::Instant::from_std(deadline),
        domain.check_owned_dispatch(cap.clone(), scope.into()),
    )
    .await
    .map_err(|_| Error::OutcomeUnknown)??;
    checkpoint(deadline, cancel)
}
