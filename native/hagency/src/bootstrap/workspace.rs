//! Bounded original Started bindings, never a public source-root registry.
use super::Failure;
use hagency_core::tasks::RunnerCapability;
use hagency_execution::StartedWorkspace;
use std::collections::BTreeMap;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

struct Entry {
    capability: RunnerCapability,
    binding: StartedWorkspace,
    retired: AtomicBool,
}
impl Entry {
    fn matches(&self, capability: &RunnerCapability) -> bool {
        self.capability.dispatch_id == capability.dispatch_id
            && self.capability.runner_id == capability.runner_id
            && self.capability.fence == capability.fence
            && self.capability.secret == capability.secret
    }
}
#[derive(Clone)]
pub(crate) struct WorkspaceAccess {
    entries: Arc<Mutex<BTreeMap<String, Arc<Entry>>>>,
    retired: Arc<AtomicBool>,
}
pub(super) struct Rejected {
    pub capability: RunnerCapability,
    pub binding: StartedWorkspace,
}
impl WorkspaceAccess {
    pub fn new() -> Self {
        Self {
            entries: Arc::new(Mutex::new(BTreeMap::new())),
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
        let Ok(mut entries) = self.entries.lock() else {
            return Err(Rejected {
                capability,
                binding,
            });
        };
        if self.retired.load(Ordering::Acquire)
            || entries.len() >= 16
            || entries.contains_key(&capability.dispatch_id)
        {
            return Err(Rejected {
                capability,
                binding,
            });
        }
        entries.insert(
            capability.dispatch_id.clone(),
            Arc::new(Entry {
                capability,
                binding,
                retired: AtomicBool::new(false),
            }),
        );
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
    /// Remove only the binding installed for this exact runner capability.
    /// A clean sequential driver calls this after its operation has stopped;
    /// a stale or foreign capability cannot clear the current handoff.
    pub(super) fn release(&self, capability: &RunnerCapability) -> Result<(), Failure> {
        if self.retired.load(Ordering::Acquire) {
            return Err(Failure::Registration);
        }
        let mut entries = self.entries.lock().map_err(|_| Failure::Registration)?;
        if self.retired.load(Ordering::Acquire) {
            return Err(Failure::Registration);
        }
        let entry = entries
            .get(&capability.dispatch_id)
            .filter(|entry| entry.matches(capability))
            .ok_or(Failure::Registration)?;
        entry.retired.store(true, Ordering::Release);
        entries.remove(&capability.dispatch_id);
        Ok(())
    }
    pub async fn check(&self, capability: &RunnerCapability) -> Result<(), Failure> {
        self.acquire(capability).await.map(|_| ())
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
        if self.retired.load(Ordering::Acquire) {
            return Err(Failure::Registration);
        }
        // Select and retain exactly once. Releasing/replacing an entry while
        // the writer check waits cannot redirect this guard to another root.
        let entry = self
            .entries
            .lock()
            .map_err(|_| Failure::Registration)?
            .get(&capability.dispatch_id)
            .filter(|entry| entry.matches(capability))
            .cloned()
            .ok_or(Failure::Registration)?;
        let guard = WorkspaceGuard {
            entry,
            retired: self.retired.clone(),
        };
        guard.validate_current().await?;
        Ok(guard)
    }
}
impl WorkspaceGuard {
    fn is_retired(&self) -> bool {
        self.retired.load(Ordering::Acquire) || self.entry.retired.load(Ordering::Acquire)
    }
    pub(crate) fn prepare_receive(
        &self,
        write: hagency_store::ReceiveWrite,
        deadline: std::time::Instant,
    ) -> Result<hagency_execution::WorkspaceReceive, Failure> {
        if self.is_retired() {
            return Err(Failure::Registration);
        }
        self.entry
            .binding
            .prepare_receive(&self.entry.capability, write, deadline)
            .map_err(|_| Failure::Registration)
    }
    pub(crate) async fn validate_current(&self) -> Result<(), Failure> {
        if self.is_retired() {
            return Err(Failure::Registration);
        }
        self.entry
            .binding
            .validate_current(&self.entry.capability)
            .await
            .map_err(|_| Failure::Registration)?;
        if self.is_retired() {
            return Err(Failure::Registration);
        }
        Ok(())
    }
    pub(crate) fn snapshot(
        &self,
        selection: &hagency_files::RelativeFile,
        limit: usize,
    ) -> Result<hagency_files::Snapshot, Failure> {
        if self.is_retired() {
            return Err(Failure::Registration);
        }
        let value = self
            .entry
            .binding
            .snapshot(&self.entry.capability, selection, limit)
            .map_err(|_| Failure::Registration)?;
        if self.is_retired() {
            return Err(Failure::Registration);
        }
        Ok(value)
    }
}
