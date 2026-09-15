//! One bootstrap-owned binding, deliberately not a public source reader yet.
use super::Failure;
use hagency_core::tasks::RunnerCapability;
use hagency_execution::StartedWorkspace;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

struct Entry {
    capability: RunnerCapability,
    binding: StartedWorkspace,
}
#[derive(Clone)]
pub(crate) struct WorkspaceAccess {
    entry: Arc<Mutex<Option<Arc<Entry>>>>,
    retired: Arc<AtomicBool>,
}
pub(super) struct Rejected {
    pub capability: RunnerCapability,
    pub binding: StartedWorkspace,
}
impl WorkspaceAccess {
    pub fn new() -> Self {
        Self {
            entry: Arc::new(Mutex::new(None)),
            retired: Arc::new(AtomicBool::new(false)),
        }
    }
    pub(super) async fn register(
        &self,
        capability: RunnerCapability,
        binding: StartedWorkspace,
    ) -> Result<(), Rejected> {
        if self.retired.load(Ordering::Acquire)
            || binding.validate_current(&capability).await.is_err()
        {
            return Err(Rejected {
                capability,
                binding,
            });
        }
        let Ok(mut entry) = self.entry.lock() else {
            return Err(Rejected {
                capability,
                binding,
            });
        };
        if self.retired.load(Ordering::Acquire) || entry.is_some() {
            return Err(Rejected {
                capability,
                binding,
            });
        }
        *entry = Some(Arc::new(Entry {
            capability,
            binding,
        }));
        Ok(())
    }
    #[cfg(test)]
    pub(crate) async fn register_for_service_test(
        &self,
        capability: RunnerCapability,
        binding: StartedWorkspace,
    ) -> Result<(), Failure> {
        // Forward the actual one-shot post-Started handoff; no entry setter.
        self.register(capability, binding)
            .await
            .map_err(|_| Failure::Registration)
    }
    pub fn retire(&self) {
        self.retired.store(true, Ordering::Release);
    }
    pub async fn check(&self, capability: &RunnerCapability) -> Result<(), Failure> {
        if self.retired.load(Ordering::Acquire) {
            return Err(Failure::Registration);
        }
        let entry = self
            .entry
            .lock()
            .map_err(|_| Failure::Registration)?
            .clone()
            .ok_or(Failure::Registration)?;
        if entry.capability.dispatch_id != capability.dispatch_id
            || entry.capability.runner_id != capability.runner_id
            || entry.capability.fence != capability.fence
            || entry.capability.secret != capability.secret
        {
            return Err(Failure::Registration);
        }
        entry
            .binding
            .validate_current(capability)
            .await
            .map_err(|_| Failure::Registration)?;
        if self.retired.load(Ordering::Acquire) {
            return Err(Failure::Registration);
        }
        Ok(())
    }
}

/// This guard retains the exact original writer/capability/root; no raw root is exposed.
pub(crate) struct WorkspaceGuard {
    entry: Arc<Entry>,
    retired: Arc<AtomicBool>,
}
impl WorkspaceAccess {
    pub(crate) async fn acquire(
        &self,
        capability: &RunnerCapability,
    ) -> Result<WorkspaceGuard, Failure> {
        self.check(capability).await?;
        let entry = self
            .entry
            .lock()
            .map_err(|_| Failure::Registration)?
            .clone()
            .ok_or(Failure::Registration)?;
        Ok(WorkspaceGuard {
            entry,
            retired: self.retired.clone(),
        })
    }
}
impl WorkspaceGuard {
    pub(crate) fn prepare_receive(
        &self,
        write: hagency_store::ReceiveWrite,
        deadline: std::time::Instant,
    ) -> Result<hagency_execution::WorkspaceReceive, Failure> {
        if self.retired.load(Ordering::Acquire) {
            return Err(Failure::Registration);
        }
        self.entry
            .binding
            .prepare_receive(&self.entry.capability, write, deadline)
            .map_err(|_| Failure::Registration)
    }
    pub(crate) async fn validate_current(&self) -> Result<(), Failure> {
        if self.retired.load(Ordering::Acquire) {
            return Err(Failure::Registration);
        }
        self.entry
            .binding
            .validate_current(&self.entry.capability)
            .await
            .map_err(|_| Failure::Registration)?;
        if self.retired.load(Ordering::Acquire) {
            return Err(Failure::Registration);
        }
        Ok(())
    }
    pub(crate) fn snapshot(
        &self,
        selection: &hagency_files::RelativeFile,
        limit: usize,
    ) -> Result<hagency_files::Snapshot, Failure> {
        if self.retired.load(Ordering::Acquire) {
            return Err(Failure::Registration);
        }
        let value = self
            .entry
            .binding
            .snapshot(&self.entry.capability, selection, limit)
            .map_err(|_| Failure::Registration)?;
        if self.retired.load(Ordering::Acquire) {
            return Err(Failure::Registration);
        }
        Ok(value)
    }
}
