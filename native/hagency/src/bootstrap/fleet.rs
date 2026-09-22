//! Original inline agents, finite service ownership and per-agent file routing.
use super::{
    DriverMode, Failure, Shared, Status, StatusHandle, driver::Driver, workspace::WorkspaceAccess,
};
use crate::{
    file_service::{FileHandle, FileOwner},
    receive_service::{ReceiveHandle, ReceiveOwner},
};
use hagency_core::tasks::RunnerCapability;
use hagency_matrix::{CancellationToken, Collector, ProvisionedAgent};
use hagency_store::DomainStore;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

/// Fixed private Host composition, not a complete deployment profile.
pub struct Setup {
    pub state: PathBuf,
    pub limit: usize,
    pub send: bool,
    pub receive: bool,
    pub limits: hagency_execution::Limits,
}
#[derive(Clone)]
pub(crate) struct Backends {
    pub files: Option<FileHandle>,
    pub receives: Option<ReceiveHandle>,
    status: StatusHandle,
}
#[derive(Clone)]
pub(crate) struct Routes {
    domain: DomainStore,
    entries: Arc<Mutex<BTreeMap<String, Backends>>>,
    running: Arc<AtomicBool>,
    failed: Arc<AtomicBool>,
    closed: Arc<AtomicBool>,
}

/// An agent whose attempt failed is lost to the fleet unless it says it is alive
/// and waiting for the operator to resolve that attempt. Its own status keeps
/// reporting the failure either way.
fn lost(status: &super::Status) -> bool {
    status.error.is_some() && !status.awaiting_operator
}
impl Routes {
    pub(crate) async fn select(
        &self,
        cap: RunnerCapability,
    ) -> Result<Backends, hagency_store::Error> {
        // Full original credential authentication is a bounded writer read,
        // including for historical GET after workspace release. This lookup
        // itself grants no execution/IO; each backend still checks its scope.
        let engagement = self.domain.runner_service_engagement(cap).await?;
        self.entries
            .lock()
            .map_err(|_| hagency_store::Error::OutcomeUnknown)?
            .get(&engagement)
            .cloned()
            .ok_or(hagency_store::Error::NotFound)
    }
    fn insert(&self, engagement: String, backends: Backends) -> Result<(), Failure> {
        let mut entries = self.entries.lock().map_err(|_| Failure::OutcomeUnknown)?;
        if self.closed.load(Ordering::Acquire)
            || entries.len() >= 17
            || entries.contains_key(&engagement)
        {
            return Err(Failure::Registration);
        }
        entries.insert(engagement, backends);
        Ok(())
    }
    pub(crate) fn state(&self) -> &'static str {
        if self.closed.load(Ordering::Acquire) {
            return "stopped";
        }
        let entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        if self.failed.load(Ordering::Acquire)
            || entries.values().any(|entry| lost(&entry.status.get()))
        {
            return "outcome_unknown";
        }
        if self.running.load(Ordering::Acquire) {
            "running"
        } else {
            "not_started"
        }
    }
    pub(crate) fn snapshot(&self) -> serde_json::Value {
        let entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        let failed = self.failed.load(Ordering::Acquire)
            || entries.values().any(|entry| lost(&entry.status.get()));
        let agents: Vec<_>=entries.iter().map(|(engagement,entry)|serde_json::json!({"engagement_id":engagement,"status":entry.status.get()})).collect();
        serde_json::json!({"profile":"inline_factory_service_checkpoint_v1","running":self.running.load(Ordering::Acquire),
            "closed":self.closed.load(Ordering::Acquire),"failed":failed,"registered_backends":entries.len(),"agents":agents})
    }
}
struct AgentOwner {
    shared: Shared,
    status: StatusHandle,
    driver: Option<Driver>,
    files: Option<FileOwner>,
    receives: Option<ReceiveOwner>,
}
impl AgentOwner {
    fn quiesce(&self) {
        if let Some(driver) = &self.driver {
            driver.cancel();
        }
        if let Some(files) = &self.files {
            files.quiesce();
        }
        if let Some(receives) = &self.receives {
            receives.quiesce();
        }
        self.shared.workspace.retire();
    }
    async fn close(&mut self) -> Result<(), Failure> {
        self.quiesce();
        let mut failed = false;
        if let Some(files) = &mut self.files {
            failed |= files.close().await.is_err();
        }
        if let Some(receives) = &mut self.receives {
            failed |= receives.close().await.is_err();
        }
        if let Some(driver) = &mut self.driver {
            failed |= driver.close().await.is_err();
        }
        if failed {
            self.status.fail(Failure::OutcomeUnknown);
            Err(Failure::OutcomeUnknown)
        } else {
            self.status.phase("closed");
            Ok(())
        }
    }
}
/// Host-only original fleet service. Bootstrap owns it; no HTTP/runtime input
/// can install a backend, reconstruct a factory owner or assert readiness.
pub struct Service {
    setup: Setup,
    coordinator: Arc<Collector>,
    domain: DomainStore,
    routes: Routes,
    agents: Vec<AgentOwner>,
    factory_close: Option<tokio::task::JoinHandle<Result<(), hagency_matrix::Error>>>,
    factory_closed: Option<Result<(), Failure>>,
    started: bool,
}
impl Service {
    pub fn new(
        domain: DomainStore,
        coordinator: Arc<Collector>,
        setup: Setup,
    ) -> Result<Self, Failure> {
        if !setup.state.is_absolute()
            || setup.limit == 0
            || setup.limit > hagency_core::file_delivery::MAX_FILE_BYTES as usize
            || (setup.receive && hagency_core::received_files::receive_limit(setup.limit).is_err())
            || !setup.limits.validate()
        {
            return Err(Failure::Config);
        }
        hagency_store::private::directory(&setup.state).map_err(|_| Failure::Config)?;
        if setup.send {
            hagency_store::private::directory(&setup.state.join("factory-file-media"))
                .map_err(|_| Failure::Config)?;
        }
        let routes = Routes {
            domain: domain.clone(),
            entries: Arc::new(Mutex::new(BTreeMap::new())),
            running: Arc::new(AtomicBool::new(false)),
            failed: Arc::new(AtomicBool::new(false)),
            closed: Arc::new(AtomicBool::new(false)),
        };
        Ok(Self {
            setup,
            coordinator,
            domain,
            routes,
            agents: vec![],
            factory_close: None,
            factory_closed: None,
            started: false,
        })
    }
    pub(crate) fn routes(&self) -> Routes {
        self.routes.clone()
    }
    pub(crate) fn register_root(
        &self,
        engagement: String,
        files: Option<FileHandle>,
        receives: Option<ReceiveHandle>,
        status: StatusHandle,
    ) -> Result<(), Failure> {
        self.routes.insert(
            engagement,
            Backends {
                files,
                receives,
                status,
            },
        )
    }
    pub fn statuses(&self) -> Vec<Status> {
        self.agents.iter().map(|agent| agent.status.get()).collect()
    }
    async fn admit(
        &mut self,
        agent: ProvisionedAgent,
        notices: tokio::sync::mpsc::Sender<hagency_execution::ApprovalRequests>,
        reattached: bool,
    ) -> Result<(), Failure> {
        if self.agents.len() >= 16 || self.routes.closed.load(Ordering::Acquire) {
            return Err(Failure::Registration);
        }
        let engagement = agent.session().engagement_id.clone();
        let shared = Shared {
            domain: self.domain.clone(),
            collector: agent.shared_collector(),
            workspace: WorkspaceAccess::new(),
        };
        // Retain the partially constructed owner before any failing startup or
        // await. Its original factory is also still held by the coordinator.
        self.agents.push(AgentOwner {
            shared,
            status: StatusHandle::for_mode(DriverMode::Continuous),
            driver: None,
            files: None,
            receives: None,
        });
        let owner = self.agents.last_mut().ok_or(Failure::Startup)?;
        let start = async {
            if self.setup.send {
                let namespace = hagency_core::canonical::digest(&serde_json::json!([
                    "native_factory_file_storage_v1",
                    engagement
                ]))
                .map_err(|_| Failure::Config)?;
                owner.files = Some(
                    FileOwner::start(
                        owner.shared.clone(),
                        crate::file_service::Setup {
                            directory: self.setup.state.join("factory-file-media").join(&namespace),
                            namespace,
                            limit: self.setup.limit,
                        },
                    )
                    .map_err(|_| Failure::Startup)?,
                );
            }
            if self.setup.receive {
                owner.receives = Some(
                    ReceiveOwner::start(
                        owner.shared.clone(),
                        crate::receive_service::Setup {
                            limit: self.setup.limit,
                        },
                    )
                    .map_err(|_| Failure::Startup)?,
                );
            }
            self.routes.insert(
                engagement,
                Backends {
                    files: owner.files.as_ref().map(FileOwner::handle),
                    receives: owner.receives.as_ref().map(ReceiveOwner::handle),
                    status: owner.status.clone(),
                },
            )?;
            owner.driver = Some(
                Driver::start_agent(
                    agent,
                    owner.shared.clone(),
                    owner.files.as_ref().map(FileOwner::handle),
                    owner.status.clone(),
                    notices,
                    self.setup.limits,
                )
                .await?,
            );
            Ok(())
        }
        .await;
        if let Err(error) = start {
            owner.quiesce();
            if reattached {
                // An agent a restart could not bring back is skipped and shown;
                // it fails neither the fleet nor the agents that did come back.
                owner.status.not_attached(None);
            } else {
                owner.status.fail(error);
                self.routes.failed.store(true, Ordering::Release);
            }
        }
        start
    }
    /// After a restart: bring back every agent this service's inline factory
    /// already completed, from what their provision left on disk. One agent that
    /// cannot come back is skipped and published as `not_attached`; it never
    /// stops the others, the coordinator or readiness.
    async fn reattach_known_agents(
        &mut self,
        notices: &tokio::sync::mpsc::Sender<hagency_execution::ApprovalRequests>,
        cancel: &CancellationToken,
    ) {
        let known = match self.coordinator.provisioned_engagements().await {
            Ok(known) => known,
            Err(error) => {
                tracing::warn!(?error, "factory agents could not be listed for re-attach");
                return;
            }
        };
        for engagement in known {
            if cancel.is_cancelled() || self.routes.closed.load(Ordering::Acquire) {
                return;
            }
            let attached = match self
                .coordinator
                .reattach_provisioned_agent(&engagement, cancel)
                .await
            {
                Ok(()) => self.coordinator.take_provisioned_agent(&engagement),
                Err(error) => Err(error),
            };
            match attached {
                Ok(agent) => {
                    if self.admit(agent, notices.clone(), true).await.is_ok() {
                        tracing::info!(%engagement, "factory agent re-attached");
                    } else {
                        tracing::warn!(%engagement, "re-attached factory agent did not start");
                    }
                }
                Err(hagency_matrix::Error::Busy) => {
                    // This process already owns the provision (the coordinator
                    // completed it before the fleet started): ordinary
                    // discovery takes it, exactly once, as it always did.
                    tracing::debug!(%engagement, "factory agent already owned here; left to discovery");
                }
                Err(error) => {
                    tracing::warn!(%engagement, ?error, "factory agent not re-attached");
                    let status = StatusHandle::for_mode(DriverMode::Continuous);
                    status.not_attached(Some(&error));
                    let _ = self.register_root(engagement, None, None, status);
                }
            }
        }
    }
    /// The actual service keeps discovery independent of the coordinator's
    /// potentially long inline intake. No job is replayed or reconstructed.
    pub async fn run(
        &mut self,
        notices: tokio::sync::mpsc::Sender<hagency_execution::ApprovalRequests>,
        cancel: &CancellationToken,
    ) -> Result<(), Failure> {
        if self.routes.closed.load(Ordering::Acquire) || self.started {
            return Err(Failure::Startup);
        }
        self.started = true;
        self.routes.running.store(true, Ordering::Release);
        struct Running(Arc<AtomicBool>);
        impl Drop for Running {
            fn drop(&mut self) {
                self.0.store(false, Ordering::Release);
            }
        }
        let _running = Running(self.routes.running.clone());
        self.reattach_known_agents(&notices, cancel).await;
        let mut tick = tokio::time::interval(Duration::from_millis(100));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {biased;_ = cancel.cancelled()=>return Ok(()),_ = tick.tick()=>{}}
            let next = self
                .coordinator
                .take_next_provisioned_agent()
                .map_err(|_| Failure::OutcomeUnknown);
            match next {
                Ok(Some(agent)) => {
                    self.admit(agent, notices.clone(), false).await?;
                }
                Ok(None) => {}
                Err(error) => {
                    self.routes.failed.store(true, Ordering::Release);
                    return Err(error);
                }
            }
        }
    }
    pub fn quiesce(&self) {
        self.routes.closed.store(true, Ordering::Release);
        for agent in &self.agents {
            agent.quiesce();
        }
    }
    pub(crate) async fn drain_agents(&mut self) -> Result<(), Failure> {
        self.quiesce();
        let mut failed = false;
        for agent in &mut self.agents {
            failed |= agent.close().await.is_err();
        }
        // Do not close an enrolled SDK while its original file/driver owners
        // are still unknown. All other admitted owners were drained above.
        if failed {
            self.routes.failed.store(true, Ordering::Release);
            return Err(Failure::OutcomeUnknown);
        }
        Ok(())
    }
    pub async fn close(&mut self) -> Result<(), Failure> {
        self.drain_agents().await?;
        if let Some(result) = self.factory_closed {
            return result;
        }
        if self.factory_close.is_none() {
            let coordinator = self.coordinator.clone();
            self.factory_close = Some(tokio::spawn(async move {
                coordinator.close_provisioned_agents().await
            }));
        }
        let result = match tokio::time::timeout(
            Duration::from_secs(2),
            self.factory_close.as_mut().ok_or(Failure::OutcomeUnknown)?,
        )
        .await
        {
            Ok(Ok(Ok(()))) => Ok(()),
            Ok(_) => Err(Failure::OutcomeUnknown),
            Err(_) => return Err(Failure::OutcomeUnknown),
        };
        self.factory_close = None;
        self.factory_closed = Some(result);
        result
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        self.quiesce();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file_service::{FileError, test_common};
    #[tokio::test]
    async fn native_factory_failure_diagnostics() {
        let f = test_common::Fixture::new();
        let collector =
            Arc::new(Collector::new(f.config("https://127.0.0.1:1/"), f.store.clone()).unwrap());
        let fleet = Service::new(
            f.store.clone(),
            collector.clone(),
            Setup {
                state: f.root.path().join("fleet-diagnostics"),
                limit: 128,
                send: false,
                receive: false,
                limits: hagency_execution::Limits {
                    operation_ms: 5000,
                    response_ms: 1000,
                },
            },
        )
        .unwrap();
        let failed = StatusHandle::new(true);
        let healthy = StatusHandle::new(true);
        failed.fail(Failure::OutcomeUnknown);
        healthy.phase("idle");
        fleet
            .register_root("en_failed".into(), None, None, failed.clone())
            .unwrap();
        fleet
            .register_root("en_healthy".into(), None, None, healthy.clone())
            .unwrap();
        let value = fleet.routes.snapshot();
        assert_eq!(value["failed"], true);
        assert_eq!(value["registered_backends"], 2);
        assert_eq!(value["agents"][0]["engagement_id"], "en_failed");
        assert_eq!(value["agents"][0]["status"]["error"], "outcome_unknown");
        assert_eq!(value["agents"][1]["engagement_id"], "en_healthy");
        assert_eq!(value["agents"][1]["status"]["state"], "idle");
        assert_eq!(failed.state(), "outcome_unknown");
        assert_eq!(healthy.state(), "idle");
        assert!(!fleet.routes.closed.load(Ordering::Acquire));
        healthy.phase("receiving");
        assert_eq!(
            fleet.routes.snapshot()["agents"][1]["status"]["state"],
            "receiving"
        );
        assert!(serde_json::to_vec(&value).unwrap().len() < 2048);
        // An agent whose failed attempt is recoverable is alive and waiting for
        // the operator. It keeps reporting its failure, says that it waits, and
        // is not what makes a fleet failed; the agent that is gone still is.
        failed.awaiting_operator();
        let value = fleet.routes.snapshot();
        assert_eq!(value["agents"][0]["status"]["error"], "outcome_unknown");
        assert_eq!(value["agents"][0]["status"]["awaiting_operator"], true);
        assert!(
            value["agents"][1]["status"]
                .get("awaiting_operator")
                .is_none()
        );
        assert_eq!(value["failed"], false);
        assert_eq!(fleet.routes.state(), "not_started");
        let gone = StatusHandle::new(true);
        gone.fail(Failure::OutcomeUnknown);
        fleet
            .register_root("en_gone".into(), None, None, gone.clone())
            .unwrap();
        assert_eq!(fleet.routes.snapshot()["failed"], true);
        // The next attempt clears the wait with everything else.
        failed.begin_attempt();
        assert!(
            fleet.routes.snapshot()["agents"][0]["status"]
                .get("awaiting_operator")
                .is_none()
        );
        drop(fleet);
        collector.close().await.unwrap();
        test_common::shutdown_domain(&f.store, "factory diagnostics").await;
    }
    #[tokio::test]
    async fn native_configured_fleet_shutdown_isolation() {
        let f = test_common::Fixture::new();
        let (a, mut first, ra) =
            super::super::driver::tests::workspace_operation(&f, "service_first").await;
        let (b, mut second, rb) =
            super::super::driver::tests::workspace_operation(&f, "service_second").await;
        let collector =
            Arc::new(Collector::new(f.config("https://127.0.0.1:1/"), f.store.clone()).unwrap());
        let one = Shared {
            domain: f.store.clone(),
            collector: collector.clone(),
            workspace: WorkspaceAccess::new(),
        };
        let two = Shared {
            domain: f.store.clone(),
            collector: collector.clone(),
            workspace: WorkspaceAccess::new(),
        };
        let (binding, ack_a) = ra.into_parts();
        assert!(one.workspace.register(a.clone(), binding).await.is_ok());
        let (binding, ack_b) = rb.into_parts();
        assert!(two.workspace.register(b.clone(), binding).await.is_ok());
        let path = hagency_files::RelativeFile::new("same.txt").unwrap();
        let first_guard = one.workspace.acquire(&a).await.unwrap();
        let second_guard = two.workspace.acquire(&b).await.unwrap();
        let existing = f.root.path().join("original-missing-media");
        hagency_store::private::directory(&existing).unwrap();
        let failed = FileOwner::start(
            one.clone(),
            crate::file_service::Setup {
                directory: existing.clone(),
                namespace: "first_service".into(),
                limit: 128,
            },
        )
        .unwrap();
        // Actual existing incomplete media directory: original recovery must
        // refuse without manufacturing a replacement journal or close ACK.
        assert_eq!(failed.handle().initialize().await, Err(FileError::Unknown));
        let healthy = FileOwner::start(
            two.clone(),
            crate::file_service::Setup {
                directory: f.root.path().join("second-media"),
                namespace: "second_service".into(),
                limit: 128,
            },
        )
        .unwrap();
        let first_owner = AgentOwner {
            shared: one,
            status: StatusHandle::new(true),
            driver: None,
            files: Some(failed),
            receives: None,
        };
        let second_owner = AgentOwner {
            shared: two,
            status: StatusHandle::new(true),
            driver: None,
            files: Some(healthy),
            receives: None,
        };
        first_owner.quiesce();
        assert!(first_guard.validate_current().await.is_err());
        second_guard.validate_current().await.unwrap();
        assert_eq!(
            second_guard
                .snapshot(&path, 4 * 1024 * 1024)
                .unwrap()
                .bytes(),
            b"service_second"
        );
        let mut fleet = Service::new(
            f.store.clone(),
            collector.clone(),
            Setup {
                state: f.root.path().join("fleet-state"),
                limit: 128,
                send: false,
                receive: false,
                limits: hagency_execution::Limits {
                    operation_ms: 5000,
                    response_ms: 1000,
                },
            },
        )
        .unwrap();
        fleet.agents = vec![first_owner, second_owner];
        assert_eq!(fleet.close().await, Err(Failure::OutcomeUnknown));
        assert_eq!(fleet.agents[0].status.state(), "outcome_unknown");
        assert_eq!(
            fleet.agents[1].status.state(),
            "closed",
            "second actual worker was acknowledged and joined despite first failure"
        );
        assert!(second_guard.validate_current().await.is_err());
        assert!(
            fleet.factory_close.is_none() && fleet.factory_closed.is_none(),
            "failed child drain cannot close original SDKs"
        );
        assert_eq!(fleet.routes.state(), "stopped");
        assert_eq!(std::fs::read_dir(existing).unwrap().count(), 0);
        drop(ack_a);
        drop(ack_b);
        for operation in [&mut first, &mut second] {
            assert_eq!(
                operation.wait().await.unwrap().protocol,
                hagency_execution::Protocol::NotStarted
            );
        }
        drop(fleet);
        collector.close().await.unwrap();
        test_common::shutdown_domain(&f.store, "original fleet drain isolation").await;
    }
}
