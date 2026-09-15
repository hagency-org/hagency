//! One development owner for source capture and exact encrypted delivery custody.
mod job;
mod pipeline;
mod recovery;
mod types;
mod worker;
use crate::bootstrap::Shared;
use hagency_core::tasks::RunnerCapability;
use hagency_matrix::CancellationToken;
use job::Job;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::Duration,
};
use tokio::sync::{mpsc, oneshot};
pub(crate) use types::{FileError, FileStatus, FileView, SendFile};

pub(crate) struct Setup {
    pub directory: PathBuf,
    pub namespace: String,
    pub limit: usize,
}
pub(super) struct Registry {
    jobs: Mutex<BTreeMap<String, Arc<Job>>>,
    ready: AtomicBool,
    closed: AtomicBool,
    cancel: CancellationToken,
    #[cfg(test)]
    tests: admission_tests::Hooks,
}
#[derive(Clone)]
pub(crate) struct FileHandle {
    shared: Shared,
    registry: Arc<Registry>,
    commands: mpsc::Sender<worker::Command>,
}
pub(crate) struct AdmissionWait {
    result: Wait,
}
enum Wait {
    Ready(Result<FileView, FileError>),
    Pending(oneshot::Receiver<Result<FileView, FileError>>),
}
impl AdmissionWait {
    pub async fn wait(self) -> Result<FileView, FileError> {
        match self.result {
            Wait::Ready(value) => value,
            Wait::Pending(wait) => wait.await.unwrap_or(Err(FileError::Unknown)),
        }
    }
}
pub(crate) struct FileOwner {
    handle: FileHandle,
    thread: Option<JoinHandle<()>>,
    pending_close: Option<oneshot::Receiver<Result<(), FileError>>>,
    close_result: Option<Result<(), FileError>>,
}
impl FileOwner {
    pub(crate) fn start(shared: Shared, setup: Setup) -> Result<Self, FileError> {
        if setup.limit == 0
            || setup.limit > hagency_core::file_delivery::MAX_FILE_BYTES as usize
            || !setup.directory.is_absolute()
            || hagency_media_store::HostNamespace::new(&setup.namespace).is_err()
        {
            return Err(FileError::Invalid);
        }
        let registry = Arc::new(Registry {
            jobs: Mutex::new(BTreeMap::new()),
            ready: AtomicBool::new(false),
            closed: AtomicBool::new(false),
            cancel: CancellationToken::new(),
            #[cfg(test)]
            tests: admission_tests::Hooks::default(),
        });
        let (commands, receive) = mpsc::channel(2);
        let handle = FileHandle {
            shared: shared.clone(),
            registry: registry.clone(),
            commands,
        };
        let thread = std::thread::Builder::new()
            .name("hagency-file-service".into())
            .spawn(move || worker::run(shared, registry, setup, receive))
            .map_err(|_| FileError::Unavailable)?;
        Ok(Self {
            handle,
            thread: Some(thread),
            pending_close: None,
            close_result: None,
        })
    }
    pub(crate) fn handle(&self) -> FileHandle {
        self.handle.clone()
    }
    pub(crate) fn quiesce(&self) {
        self.handle.registry.closed.store(true, Ordering::Release);
        self.handle.registry.cancel.cancel();
    }
    pub(crate) async fn close(&mut self) -> Result<(), FileError> {
        self.quiesce();
        if let Some(result) = self.close_result {
            return result;
        }
        if self.thread.is_none() {
            return Ok(());
        }
        if self.pending_close.is_none() {
            let (tx, rx) = oneshot::channel();
            self.handle
                .commands
                .try_send(worker::Command::Close(tx))
                .map_err(|_| FileError::Unknown)?;
            self.pending_close = Some(rx);
        }
        let result = match tokio::time::timeout(
            Duration::from_secs(2),
            self.pending_close.as_mut().ok_or(FileError::Unknown)?,
        )
        .await
        {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => {
                self.pending_close = None;
                self.close_result = Some(Err(FileError::Unknown));
                return Err(FileError::Unknown);
            }
            Err(_) => return Err(FileError::Unknown),
        };
        self.pending_close = None;
        if result.is_err() {
            self.close_result = Some(result);
            return result;
        }
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            self.close_result = Some(Err(FileError::Unknown));
            return Err(FileError::Unknown);
        }
        self.close_result = Some(Ok(()));
        Ok(())
    }
}
impl Drop for FileOwner {
    fn drop(&mut self) {
        self.quiesce();
    }
}
impl FileHandle {
    /// Internal startup receipt only; current authentication runs first in Driver.
    pub(crate) async fn initialize(&self) -> Result<(), FileError> {
        let (tx, rx) = oneshot::channel();
        self.commands
            .try_send(worker::Command::Initialize(tx))
            .map_err(|_| FileError::Unknown)?;
        tokio::time::timeout(Duration::from_secs(2), rx)
            .await
            .map_err(|_| FileError::Unknown)?
            .map_err(|_| FileError::Unknown)?
    }
    pub fn submit(
        &self,
        cap: RunnerCapability,
        input: SendFile,
    ) -> Result<AdmissionWait, FileError> {
        let request = input.request()?;
        let key = types::key(&cap, &input.call_id)?;
        let mut jobs = self.registry.jobs.lock().map_err(|_| FileError::Unknown)?;
        if let Some(job) = jobs.get(&key) {
            if job.request != request {
                return Err(FileError::Conflict);
            }
            let result = job.result().map(|mut view| {
                view.replayed = true;
                view
            });
            return Ok(AdmissionWait {
                result: Wait::Ready(result),
            });
        }
        if self.registry.closed.load(Ordering::Acquire)
            || !self.registry.ready.load(Ordering::Acquire)
        {
            return Err(FileError::Unavailable);
        }
        if jobs.len() >= 2 {
            return Err(FileError::Busy);
        }
        let job = Arc::new(Job {
            key: key.clone(),
            cap,
            input,
            request,
            info: Mutex::new(job::Info {
                live: true,
                ..Default::default()
            }),
            original: tokio::sync::Mutex::new(job::Original::default()),
        });
        let (tx, rx) = oneshot::channel();
        jobs.insert(key.clone(), job.clone());
        if self
            .commands
            .try_send(worker::Command::Submit(job, tx))
            .is_err()
        {
            jobs.remove(&key);
            return Err(FileError::Busy);
        }
        Ok(AdmissionWait {
            result: Wait::Pending(rx),
        })
    }
    pub async fn inspect(&self, cap: RunnerCapability, id: String) -> Result<FileView, FileError> {
        types::capability(&cap)?;
        let receipt = self
            .shared
            .domain
            .inspect_file_delivery(cap, id.clone())
            .await
            .map_err(FileError::from)?;
        let live = self
            .registry
            .jobs
            .lock()
            .map_err(|_| FileError::Unknown)?
            .values()
            .any(|job| {
                job.info
                    .lock()
                    .ok()
                    .is_some_and(|info| info.id.as_deref() == Some(&id) && info.live)
            });
        let value = FileView::from_receipt(receipt, live);
        value.validate()?;
        Ok(value)
    }
}
impl Registry {
    fn remove(&self, job: &Arc<Job>) -> Result<(), FileError> {
        let mut jobs = self.jobs.lock().map_err(|_| FileError::Unknown)?;
        if jobs.get(&job.key).is_some_and(|old| Arc::ptr_eq(old, job)) {
            jobs.remove(&job.key);
            Ok(())
        } else {
            Err(FileError::Unknown)
        }
    }
    fn mark_unknown(&self) {
        if let Ok(jobs) = self.jobs.lock() {
            for job in jobs.values() {
                job.mark_unknown();
            }
        }
    }
    fn empty(&self) -> bool {
        self.jobs.lock().is_ok_and(|jobs| jobs.is_empty())
    }
}

#[cfg(test)]
#[path = "../tests/file_service/admission.rs"]
mod admission_tests;
#[cfg(test)]
#[path = "../tests/file_service/scope.rs"]
mod scope_tests;
#[cfg(test)]
#[path = "../tests/file_service/shutdown.rs"]
mod shutdown_tests;
#[cfg(test)]
#[path = "../../hagency-matrix/tests/common/mod.rs"]
pub(crate) mod test_common;
