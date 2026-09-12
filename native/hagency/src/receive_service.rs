//! Bounded development receive custody through the same bootstrap account owner.
mod job;
mod pipeline;
mod recovery;
mod types;
mod worker;
use crate::bootstrap::Shared;
use hagency_core::{
    attachments::AttachmentPage, received_files::MAX_RECEIVED_FILES, tasks::RunnerCapability,
};
use hagency_matrix::CancellationToken;
use job::{Disposition, Job};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::Duration,
};
use tokio::{
    sync::{mpsc, oneshot, watch},
    time::Instant,
};
pub(crate) use types::{ReceiveError, ReceiveFile, ReceiveView};

const LIVE_JOBS: usize = 2;
const BUDGET: Duration = Duration::from_secs(5);
pub(crate) struct Setup {
    pub limit: usize,
}
#[derive(Default)]
struct Entries {
    live: BTreeMap<String, Arc<Job>>,
    ready: BTreeMap<String, Arc<Job>>,
}
pub(super) struct Registry {
    entries: Mutex<Entries>,
    closed: AtomicBool,
    cancel: CancellationToken,
}
#[derive(Clone)]
pub(crate) struct ReceiveHandle {
    shared: Shared,
    registry: Arc<Registry>,
    commands: mpsc::Sender<worker::Command>,
}
pub(crate) struct ReceiveWait {
    handle: ReceiveHandle,
    job: Arc<Job>,
    replayed: bool,
    deadline: Instant,
}
impl ReceiveWait {
    pub async fn wait(self) -> Result<ReceiveView, ReceiveError> {
        let mut result = self.job.finished.subscribe();
        loop {
            if let Some(value) = result.borrow_and_update().clone() {
                value?;
                break;
            }
            tokio::time::timeout_at(self.deadline, result.changed())
                .await
                .map_err(|_| ReceiveError::Unknown)?
                .map_err(|_| ReceiveError::Unknown)?;
        }
        // No saved path is returned directly, even if the original requester
        // waited before polling or another caller joined while the job was live.
        let (reply, wait) = oneshot::channel();
        self.handle
            .commands
            .try_send(worker::Command::Read {
                job: self.job,
                deadline: self.deadline,
                replayed: self.replayed,
                reply,
            })
            .map_err(|_| ReceiveError::Busy)?;
        tokio::time::timeout_at(self.deadline, wait)
            .await
            .map_err(|_| ReceiveError::Unknown)?
            .map_err(|_| ReceiveError::Unknown)?
    }
}
pub(crate) struct ReceiveOwner {
    handle: ReceiveHandle,
    thread: Option<JoinHandle<()>>,
    pending_close: Option<oneshot::Receiver<Result<(), ReceiveError>>>,
    close_result: Option<Result<(), ReceiveError>>,
}
impl ReceiveOwner {
    pub(crate) fn start(shared: Shared, setup: Setup) -> Result<Self, ReceiveError> {
        hagency_core::received_files::receive_limit(setup.limit)
            .map_err(|_| ReceiveError::Invalid)?;
        let registry = Arc::new(Registry {
            entries: Mutex::new(Entries::default()),
            closed: AtomicBool::new(false),
            cancel: CancellationToken::new(),
        });
        let (commands, receive) = mpsc::channel(2);
        let handle = ReceiveHandle {
            shared: shared.clone(),
            registry: registry.clone(),
            commands,
        };
        // Exactly one OS worker owns synchronous filesystem execution. Its
        // local async tasks permit at most two original download jobs.
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| ReceiveError::Unavailable)?;
        let thread = std::thread::Builder::new()
            .name("hagency-receive-service".into())
            .spawn(move || worker::run(runtime, shared, registry, setup, receive))
            .map_err(|_| ReceiveError::Unavailable)?;
        Ok(Self {
            handle,
            thread: Some(thread),
            pending_close: None,
            close_result: None,
        })
    }
    pub(crate) fn handle(&self) -> ReceiveHandle {
        self.handle.clone()
    }
    pub(crate) fn quiesce(&self) {
        self.handle.registry.closed.store(true, Ordering::Release);
        self.handle.registry.cancel.cancel();
    }
    pub(crate) async fn close(&mut self) -> Result<(), ReceiveError> {
        self.quiesce();
        if let Some(result) = self.close_result {
            return result;
        }
        if self.pending_close.is_none() {
            let (reply, wait) = oneshot::channel();
            if self
                .handle
                .commands
                .try_send(worker::Command::Close(reply))
                .is_err()
            {
                self.close_result = Some(Err(ReceiveError::Unknown));
                return Err(ReceiveError::Unknown);
            }
            self.pending_close = Some(wait);
        }
        let result = match tokio::time::timeout(
            Duration::from_secs(2),
            self.pending_close.as_mut().ok_or(ReceiveError::Unknown)?,
        )
        .await
        {
            Ok(Ok(value)) => value,
            Ok(Err(_)) => Err(ReceiveError::Unknown),
            Err(_) => return Err(ReceiveError::Unknown),
        };
        self.pending_close = None;
        if result.is_err() {
            self.close_result = Some(result);
            return result;
        }
        if self
            .thread
            .take()
            .is_some_and(|thread| thread.join().is_err())
        {
            self.close_result = Some(Err(ReceiveError::Unknown));
            return Err(ReceiveError::Unknown);
        }
        self.close_result = Some(Ok(()));
        Ok(())
    }
}
impl Drop for ReceiveOwner {
    fn drop(&mut self) {
        self.quiesce();
    }
}
impl ReceiveHandle {
    pub fn submit(
        &self,
        cap: RunnerCapability,
        input: ReceiveFile,
    ) -> Result<ReceiveWait, ReceiveError> {
        let deadline = Instant::now() + BUDGET;
        input.validate()?;
        let key = types::key(&cap, &input.event_id)?;
        let mut entries = self
            .registry
            .entries
            .lock()
            .map_err(|_| ReceiveError::Unknown)?;
        if self.registry.closed.load(Ordering::Acquire) {
            return Err(ReceiveError::Unavailable);
        }
        if let Some(job) = entries.live.get(&key).or_else(|| entries.ready.get(&key)) {
            return Ok(ReceiveWait {
                handle: self.clone(),
                job: job.clone(),
                replayed: true,
                deadline,
            });
        }
        if entries.live.len() >= LIVE_JOBS || entries.ready.len() >= MAX_RECEIVED_FILES {
            return Err(ReceiveError::Busy);
        }
        let (finished, _) = watch::channel(None);
        let job = Arc::new(Job {
            key: key.clone(),
            cap,
            input,
            deadline,
            info: Mutex::new(Default::default()),
            original: tokio::sync::Mutex::new(Default::default()),
            finished,
        });
        entries.live.insert(key.clone(), job.clone());
        if self
            .commands
            .try_send(worker::Command::Submit(job.clone()))
            .is_err()
        {
            entries.live.remove(&key);
            return Err(ReceiveError::Busy);
        }
        Ok(ReceiveWait {
            handle: self.clone(),
            job,
            replayed: false,
            deadline,
        })
    }
    pub async fn list(
        &self,
        cap: RunnerCapability,
        after: u64,
        limit: usize,
    ) -> Result<AttachmentPage, ReceiveError> {
        let deadline = Instant::now() + BUDGET;
        if after > hagency_core::JSON_SAFE_MAX || !(1..=16).contains(&limit) {
            return Err(ReceiveError::Invalid);
        }
        pipeline::checkpoint(&self.registry, deadline)?;
        self.shared
            .workspace
            .check(&cap)
            .await
            .map_err(|_| ReceiveError::Unauthorized)?;
        let page = self
            .shared
            .domain
            .visible_attachments(cap.clone(), after, limit)
            .await
            .map_err(ReceiveError::from)?;
        self.shared
            .workspace
            .check(&cap)
            .await
            .map_err(|_| ReceiveError::Unauthorized)?;
        pipeline::checkpoint(&self.registry, deadline)?;
        types::page(&page, after, limit)?;
        Ok(page)
    }
}
impl Registry {
    fn joined(&self, job: &Arc<Job>) -> Result<(), ReceiveError> {
        let mut entries = self.entries.lock().map_err(|_| ReceiveError::Unknown)?;
        if !entries
            .live
            .get(&job.key)
            .is_some_and(|old| Arc::ptr_eq(old, job))
        {
            return Err(ReceiveError::Unknown);
        }
        match job
            .info
            .lock()
            .map_err(|_| ReceiveError::Unknown)?
            .disposition
        {
            Disposition::Retain => {}
            Disposition::Release => {
                entries.live.remove(&job.key);
            }
            Disposition::Ready => {
                if entries.ready.len() >= MAX_RECEIVED_FILES {
                    return Err(ReceiveError::Unknown);
                }
                entries.ready.insert(job.key.clone(), job.clone());
                entries.live.remove(&job.key);
            }
        }
        Ok(())
    }
    fn unknown(&self) {
        self.closed.store(true, Ordering::Release);
        self.cancel.cancel();
        let entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        for job in entries.live.values() {
            job.unknown();
            job.acknowledge();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn cap() -> RunnerCapability {
        RunnerCapability {
            dispatch_id: "dispatch".into(),
            runner_id: "runner".into(),
            fence: 1,
            secret: "a".repeat(64),
        }
    }
    fn job(key: &str) -> Arc<Job> {
        let (finished, _) = watch::channel(None);
        Arc::new(Job {
            key: key.into(),
            cap: cap(),
            input: ReceiveFile {
                event_id: "$original".into(),
            },
            deadline: Instant::now() + BUDGET,
            info: Mutex::new(Default::default()),
            original: tokio::sync::Mutex::new(Default::default()),
            finished,
        })
    }
    #[tokio::test]
    async fn native_receive_service_owned_jobs() {
        let original = job("same");
        let registry = Registry {
            entries: Mutex::new(Entries::default()),
            closed: AtomicBool::new(false),
            cancel: CancellationToken::new(),
        };
        registry
            .entries
            .lock()
            .unwrap()
            .live
            .insert(original.key.clone(), original.clone());
        let substitute = job("same");
        substitute.finish(Err(ReceiveError::Unauthorized), Disposition::Release);
        assert_eq!(registry.joined(&substitute), Err(ReceiveError::Unknown));
        assert!(Arc::ptr_eq(
            registry.entries.lock().unwrap().live.get("same").unwrap(),
            &original
        ));
        original.unknown();
        registry.joined(&original).unwrap();
        assert_eq!(registry.entries.lock().unwrap().live.len(), 1);

        // Actual fixed worker, real canonical writer and uninitialized Collector.
        // There is no Started binding and no setter manufactures one; admission
        // must refuse without network IO and release only after its task joins.
        let fixture = crate::file_service::test_common::Fixture::new();
        let shared = Shared {
            domain: fixture.store.clone(),
            collector: Arc::new(
                hagency_matrix::Collector::new(
                    fixture.config("https://127.0.0.1:1/"),
                    fixture.store.clone(),
                )
                .unwrap(),
            ),
            workspace: crate::bootstrap::workspace::WorkspaceAccess::new(),
        };
        // Exercise bounded admission without a consumer. Queuing metadata here
        // grants no Started/workspace/SDK authority and performs no effects.
        let (commands, mut unpolled) = mpsc::channel(2);
        let queued = ReceiveHandle {
            shared: shared.clone(),
            registry: Arc::new(Registry {
                entries: Mutex::new(Entries::default()),
                closed: AtomicBool::new(false),
                cancel: CancellationToken::new(),
            }),
            commands,
        };
        let first = queued
            .submit(
                cap(),
                ReceiveFile {
                    event_id: "$first".into(),
                },
            )
            .unwrap();
        let held = first.job.clone();
        let original_deadline = held.deadline;
        drop(first);
        let replay = queued
            .submit(
                cap(),
                ReceiveFile {
                    event_id: "$first".into(),
                },
            )
            .unwrap();
        assert!(Arc::ptr_eq(&held, &replay.job));
        assert!(replay.replayed);
        assert_eq!(replay.job.deadline, original_deadline);
        let _second = queued
            .submit(
                cap(),
                ReceiveFile {
                    event_id: "$second".into(),
                },
            )
            .unwrap();
        assert!(matches!(
            queued.submit(
                cap(),
                ReceiveFile {
                    event_id: "$third".into()
                }
            ),
            Err(ReceiveError::Busy)
        ));
        assert_eq!(queued.registry.entries.lock().unwrap().live.len(), 2);
        let mut unaccepted_close = ReceiveOwner {
            handle: queued,
            thread: None,
            pending_close: None,
            close_result: None,
        };
        assert_eq!(unaccepted_close.close().await, Err(ReceiveError::Unknown));
        drop(unpolled.try_recv().unwrap());
        // Newly available queue capacity cannot turn the original failed close
        // into another close attempt or a reported success.
        assert_eq!(unaccepted_close.close().await, Err(ReceiveError::Unknown));
        assert!(unaccepted_close.pending_close.is_none());
        let mut owner = ReceiveOwner::start(shared, Setup { limit: 1024 }).unwrap();
        let wait = owner
            .handle()
            .submit(
                cap(),
                ReceiveFile {
                    event_id: "$original".into(),
                },
            )
            .unwrap();
        let retained = wait.job.clone();
        let mut completion = retained.finished.subscribe();
        drop(wait);
        tokio::time::timeout(BUDGET, async {
            while completion.borrow_and_update().is_none() {
                completion.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
        assert_eq!(
            completion.borrow().as_ref(),
            Some(&Err(ReceiveError::Unauthorized))
        );
        assert!(
            owner
                .handle
                .registry
                .entries
                .lock()
                .unwrap()
                .live
                .is_empty()
        );
        owner.close().await.unwrap();
        owner.close().await.unwrap();
        assert!(owner.thread.is_none());
        assert!(matches!(
            owner.handle().submit(
                cap(),
                ReceiveFile {
                    event_id: "$original".into()
                }
            ),
            Err(ReceiveError::Unavailable)
        ));
        fixture.store.shutdown().await.unwrap();
    }
}
