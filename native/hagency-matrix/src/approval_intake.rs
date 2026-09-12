use crate::{
    CancellationToken, Error, HostConfig,
    approval_batch::{Batch, Command, Outcome, Phase, Room, Target},
    collector::{Inner, observe},
    sdk::Owner,
};
use hagency_core::{approvals::*, project::identifier, replies::RoomPrivacy};
use hagency_store::DomainStore;
use serde_json::json;
use std::{collections::BTreeSet, sync::Arc, time::Duration};
/// Host configuration only. The shared identity tuple names the approval bot,
/// never an Agent Matrix transport. Its persisted SDK purpose is independent.
pub struct HostApprovalConfig {
    config: HostConfig,
    engagements: Vec<String>,
}
impl HostApprovalConfig {
    pub fn new(mut config: HostConfig, engagements: Vec<String>) -> Result<Self, Error> {
        if config.approval
            || config.enrollment.is_some()
            || engagements.is_empty()
            || engagements.len() > 64
            || !engagements.contains(&config.identity.transport.engagement_id)
            || config
                .rooms
                .iter()
                .any(|r| !matches!(r.privacy, RoomPrivacy::Direct { .. }))
        {
            return Err(Error::Config);
        }
        let mut seen = BTreeSet::new();
        for id in &engagements {
            identifier(id, 128).map_err(|_| Error::Config)?;
            if !seen.insert(id) {
                return Err(Error::Config);
            }
        }
        config.approval = true;
        Ok(Self {
            config,
            engagements,
        })
    }
    /// Explicit approval-purpose fresh ordinary account. Peer masters come from
    /// the host outside the Matrix query channel, never a request or event.
    pub fn with_fresh_account_enrollment(
        mut self,
        anchors: Vec<(String, String)>,
    ) -> Result<Self, Error> {
        if self.config.endpoint.scheme() != "https" {
            return Err(Error::Config);
        }
        self.config.enrollment = Some(crate::enrollment::state::Profile::new(
            anchors,
            &self.config.identity.transport.sender_mxid,
            &self.config.identity.server_name,
        )?);
        Ok(self)
    }
}
pub struct HostApprovalPlan {
    requests: Vec<String>,
}
impl HostApprovalPlan {
    pub fn new(requests: Vec<String>) -> Result<Self, Error> {
        if requests.len() > 64 {
            return Err(Error::Config);
        }
        let mut seen = BTreeSet::new();
        for id in &requests {
            identifier(id, 128).map_err(|_| Error::Config)?;
            if !seen.insert(id) {
                return Err(Error::Config);
            }
        }
        Ok(Self { requests })
    }
}
#[derive(Debug, PartialEq, Eq)]
pub struct ApprovalIntakeSummary {
    pub accepted: usize,
    pub replayed: usize,
    pub rejected: usize,
    pub pending: usize,
}
/// Bounded host inspection; excludes room IDs, account IDs, keys and event bodies.
#[derive(Debug, PartialEq, Eq)]
pub enum ApprovalCustodyStage {
    Idle,
    Prepared,
    Applying,
    Derived,
    Quarantined,
}
#[derive(Debug, PartialEq, Eq)]
pub struct ApprovalCustodyStatus {
    pub stage: ApprovalCustodyStage,
    pub completed_batches: usize,
    pub terminal_sources: usize,
    pub frozen_targets: usize,
    pub events: usize,
    pub acknowledged: usize,
    pub retained_response_bytes: usize,
}
pub struct ApprovalCollector {
    pub(crate) inner: Arc<Inner>,
    pub(crate) engagements: Vec<String>,
    pub(crate) jobs: crate::approval_delivery::jobs::Jobs,
}
impl ApprovalCollector {
    pub fn new(config: HostApprovalConfig, domain: DomainStore) -> Result<Self, Error> {
        Ok(Self {
            inner: Inner::new(config.config, domain)?,
            engagements: config.engagements,
            jobs: Default::default(),
        })
    }
    /// Refreshes authenticated private binding snapshots without taking a sync.
    pub async fn observe(&self, cancel: &CancellationToken) -> Result<(), Error> {
        let permit = self.delivery_permit(false)?;
        let inner = self.inner.clone();
        let engagements = self.engagements.clone();
        let cancel = cancel.clone();
        #[cfg(test)]
        let observation = crate::collector::observation::current();
        tokio::spawn(async move {
            let _permit = permit;
            let work = async {
                observe!(RoomPrior);
                let rooms = inner.approval_rooms(&engagements).await?;
                observe!(Whoami);
                inner.refresh_approval_rooms(&rooms, &cancel).await?;
                Ok(())
            };
            #[cfg(test)]
            let work = crate::collector::observation::owned(observation, work);
            work.await
        })
        .await
        .map_err(|_| Error::OutcomeUnknown)?
    }
    pub async fn intake(
        &self,
        plan: HostApprovalPlan,
        cancel: &CancellationToken,
    ) -> Result<ApprovalIntakeSummary, Error> {
        self.job(Some(plan), cancel).await
    }
    /// Network-free historical receipt settlement. Never creates a new decision.
    pub async fn resume_custody(
        &self,
        cancel: &CancellationToken,
    ) -> Result<ApprovalIntakeSummary, Error> {
        self.job(None, cancel).await
    }
    async fn job(
        &self,
        plan: Option<HostApprovalPlan>,
        cancel: &CancellationToken,
    ) -> Result<ApprovalIntakeSummary, Error> {
        let permit = self.delivery_permit(plan.is_none())?;
        let inner = self.inner.clone();
        let engagements = self.engagements.clone();
        let cancel = cancel.child_token();
        tokio::spawn(async move {let _permit=permit;let work=inner.approval_intake(engagements,plan,&cancel);tokio::pin!(work);
            tokio::select!{r=&mut work=>r,_=tokio::time::sleep(Duration::from_secs(45))=>{cancel.cancel();work.await}}
        }).await.map_err(|_|Error::OutcomeUnknown)?
    }
    pub async fn custody_status(&self) -> Result<ApprovalCustodyStatus, Error> {
        let permit = self.delivery_permit(true)?;
        let inner = self.inner.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let mut guard = inner.owner.lock().await;
            if guard.is_none() {
                *guard = Some(Owner::open_existing(&inner.config).await?);
            }
            let view = guard
                .as_ref()
                .ok_or(Error::Storage)?
                .approval(Command::Read)
                .await?;
            let (stage, frozen_targets, events, acknowledged, retained_response_bytes) =
                match view.batch {
                    None => (ApprovalCustodyStage::Idle, 0, 0, 0, 0),
                    Some(b) => (
                        match b.phase {
                            Phase::Prepared => ApprovalCustodyStage::Prepared,
                            Phase::Applying => ApprovalCustodyStage::Applying,
                            Phase::Derived => ApprovalCustodyStage::Derived,
                            Phase::Quarantined => ApprovalCustodyStage::Quarantined,
                        },
                        b.targets.len(),
                        b.events.len(),
                        b.acknowledgements.len(),
                        serde_json::to_vec(&b.raw)
                            .map_err(|_| Error::Storage)?
                            .len(),
                    ),
                };
            Ok(ApprovalCustodyStatus {
                stage,
                completed_batches: view.receipts.len(),
                terminal_sources: view.outcomes.len(),
                frozen_targets,
                events,
                acknowledged,
                retained_response_bytes,
            })
        })
        .await
        .map_err(|_| Error::OutcomeUnknown)?
    }
    pub async fn close(&self) -> Result<(), Error> {
        if let Some(job) = self.jobs.closing()? {
            return match job.wait().await? {
                crate::approval_delivery::jobs::Value::Unit => Ok(()),
                _ => Err(Error::Storage),
            };
        }
        let permit = self
            .inner
            .busy
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Busy)?;
        #[cfg(test)]
        let observation = crate::collector::observation::current();
        let missing_owner = self.jobs.missing_owner_result();
        let inner = self.inner.clone();
        let engagements = self.engagements.clone();
        let job = self.jobs.start(true, false, permit, async move {
            let work = async {
                let fence = async {
                    observe!(RoomPrior);
                    let rooms = inner.approval_rooms(&engagements).await?;
                    observe!(Fence);
                    inner.fence_approval_candidates(&rooms).await
                }
                .await;
                #[cfg(test)]
                crate::collector::observation::fence(fence.as_ref().err().copied());
                // Even retired domain authority must not skip actual SDK shutdown.
                observe!(CloseOwnerLock);
                let shutdown = if let Some(owner) = inner.owner.lock().await.take() {
                    observe!(CloseSdk);
                    owner.close().await
                } else {
                    missing_owner
                };
                shutdown?;
                fence?;
                Ok(())
            };
            #[cfg(test)]
            let work = crate::collector::observation::owned(observation, work);
            work.await
                .map(|_| crate::approval_delivery::jobs::Value::Unit)
        })?;
        match job.wait().await? {
            crate::approval_delivery::jobs::Value::Unit => Ok(()),
            _ => Err(Error::Storage),
        }
    }
}
impl Inner {
    pub(crate) async fn approval_rooms(&self, engagements: &[String]) -> Result<Vec<Room>, Error> {
        let primary = self
            .domain
            .approval_room_authority(self.config.identity.transport.engagement_id.clone())
            .await?;
        let mut rooms = vec![];
        for id in engagements {
            let a = self.domain.approval_room_authority(id.clone()).await?;
            let configured = self
                .config
                .rooms
                .iter()
                .find(|r| r.room_id == a.room_id)
                .ok_or(Error::Generation)?;
            let identity = &self.config.identity;
            if a.fleet_id != primary.fleet_id
                || a.server_name != identity.server_name
                || a.registration_generation != identity.transport.registration_generation
                || a.bot_mxid != identity.transport.sender_mxid
                || configured.privacy
                    != (RoomPrivacy::Direct {
                        human_mxid: a.owner_mxid.clone(),
                    })
            {
                return Err(Error::Generation);
            }
            rooms.push(Room {
                authority: a,
                device: identity.transport.device_id.clone(),
                generation: configured.generation,
            });
        }
        Ok(rooms)
    }
    pub(crate) async fn refresh_approval_rooms(
        &self,
        rooms: &[Room],
        cancel: &CancellationToken,
    ) -> Result<(), Error> {
        let mut prior = vec![];
        for r in rooms {
            prior.push(
                self.domain
                    .approval_room_capture(r.authority.clone())
                    .await?,
            );
        }
        let result = async {
            self.whoami(cancel).await?;
            // A raw private snapshot may serve multiple Agent bindings. It is
            // authenticated once, but every target binding is checked in-store.
            let mut snapshots: std::collections::BTreeMap<
                String,
                hagency_core::replies::MatrixRoomObservation,
            > = std::collections::BTreeMap::new();
            for r in rooms {
                let configured = self
                    .config
                    .rooms
                    .iter()
                    .find(|t| t.room_id == r.authority.room_id)
                    .ok_or(Error::Generation)?;
                let observation = if let Some(v) = snapshots.get(&configured.room_id) {
                    v.clone()
                } else {
                    let raw = self
                        .http
                        .request(
                            &[
                                "_matrix",
                                "client",
                                "v3",
                                "rooms",
                                &configured.room_id,
                                "state",
                            ],
                            None,
                            cancel,
                        )
                        .await?
                        .success()?;
                    let observation = self.room(configured, raw)?;
                    snapshots.insert(configured.room_id.clone(), observation.clone());
                    observation
                };
                self.domain
                    .observe_approval_room(ApprovalRoomObservation {
                        engagement_id: r.authority.engagement_id.clone(),
                        registration_generation: r.authority.registration_generation,
                        generation: r.generation,
                        room_id: r.authority.room_id.clone(),
                        device_id: r.device.clone(),
                        joined: observation.joined,
                        invite_only: observation.invite_only,
                        encrypted: observation.encrypted,
                        available: true,
                    })
                    .await?;
                let after = self
                    .domain
                    .approval_room_capture(r.authority.clone())
                    .await?
                    .ok_or(Error::Generation)?;
                if !after.available
                    || after.device_id != r.device
                    || after.generation != r.generation
                {
                    return Err(Error::Generation);
                }
            }
            Ok(())
        }
        .await;
        if let Err(error) = result {
            let mut failed = false;
            for (r, old) in rooms.iter().zip(prior) {
                if self
                    .domain
                    .fence_approval_room(r.authority.clone(), r.device.clone(), r.generation, old)
                    .await
                    .is_err()
                {
                    failed = true;
                }
            }
            return Err(if failed { Error::OutcomeUnknown } else { error });
        }
        Ok(())
    }
    pub(crate) async fn fence_approval_candidates(&self, rooms: &[Room]) -> Result<(), Error> {
        for r in rooms {
            self.domain
                .fence_approval_room(r.authority.clone(), r.device.clone(), r.generation, None)
                .await?;
        }
        Ok(())
    }
    async fn approval_intake(
        &self,
        engagements: Vec<String>,
        plan: Option<HostApprovalPlan>,
        cancel: &CancellationToken,
    ) -> Result<ApprovalIntakeSummary, Error> {
        let mut guard = self.owner.lock().await;
        if guard.is_none() {
            if plan.is_none() {
                *guard = Some(Owner::open_existing(&self.config).await?);
            } else {
                let rooms = self.approval_rooms(&engagements).await?;
                self.refresh_approval_rooms(&rooms, cancel).await?;
                *guard = Some(Owner::open(&self.config).await?);
            }
        }
        let owner = guard.as_ref().ok_or(Error::Storage)?;
        let mut view = owner.approval(Command::Read).await?;
        let allow_new = plan.is_some();
        if view.batch.is_none()
            && let Some(plan) = plan
        {
            if view.receipts.len() >= crate::approval_batch::MAX_BATCHES
                || view.outcomes.len() >= crate::approval_batch::MAX_OUTCOMES
            {
                return Err(Error::Capacity);
            }
            let rooms = self.approval_rooms(&engagements).await?;
            self.refresh_approval_rooms(&rooms, cancel).await?;
            let mut targets = vec![];
            for id in plan.requests {
                let target = self.domain.approval_intake_target(id).await?;
                if !rooms.iter().any(|r| {
                    r.authority == target.authority
                        && r.generation == target.room_generation
                        && r.device == target.device_id
                }) {
                    return Err(Error::Generation);
                }
                targets.push(Target(target));
            }
            let users: BTreeSet<_> = rooms
                .iter()
                .flat_map(|r| [r.authority.owner_mxid.clone(), r.authority.bot_mxid.clone()])
                .collect();
            let (query_id, body) = owner.approval_query(users.into_iter().collect()).await?;
            let keys = self
                .http
                .post(&["_matrix", "client", "v3", "keys", "query"], body, cancel)
                .await?
                .success()?;
            crate::outgoing::state::encode(&keys, 256 * 1024)?;
            let filter=json!({"room":{"rooms":self.config.rooms.iter().map(|r|&r.room_id).collect::<Vec<_>>(),"timeline":{"limit":100},"ephemeral":{"types":[]},"account_data":{"types":[]},"state":{"lazy_load_members":false}},"presence":{"types":[]},"account_data":{"types":[]}}).to_string();
            let mut query = vec![
                ("timeout", "0"),
                ("full_state", "true"),
                ("filter", filter.as_str()),
            ];
            if let Some(last) = view.receipts.last() {
                query.push(("since", last.token.as_str()));
            }
            let raw = self
                .http
                .request(&["_matrix", "client", "v3", "sync"], Some(&query), cancel)
                .await?
                .success()?;
            self.sync_bounds(&raw)?;
            // Complete response freezes before any SDK crypto or cursor mutation.
            view = owner
                .approval(Command::Start(Box::new(Batch::new(
                    raw,
                    keys,
                    targets,
                    rooms,
                    query_id,
                    String::new(),
                )?)))
                .await?;
            if view.batch.is_some() {
                view = match owner.approval(Command::Apply).await {
                    Ok(view) => view,
                    Err(error @ (Error::Recipients | Error::Identity | Error::Wire)) => {
                        if let Some(batch) = &view.batch {
                            self.fence_approval_candidates(&batch.rooms).await?;
                        }
                        return Err(error);
                    }
                    Err(error) => return Err(error),
                };
            }
        }
        let Some(batch) = view.batch else {
            return Ok(ApprovalIntakeSummary {
                accepted: 0,
                replayed: 0,
                rejected: 0,
                pending: 0,
            });
        };
        if batch.phase != Phase::Derived {
            return Err(Error::OutcomeUnknown);
        }
        #[cfg(test)]
        let fault = self
            .handoff_fault
            .swap(0, std::sync::atomic::Ordering::SeqCst);
        #[cfg(test)]
        if fault == 1 {
            return Err(Error::Busy);
        }
        let mut summary = ApprovalIntakeSummary {
            accepted: 0,
            replayed: batch.acknowledgements.len(),
            rejected: 0,
            pending: batch.events.len() - batch.acknowledgements.len(),
        };
        for (index, event) in batch
            .events
            .iter()
            .enumerate()
            .skip(batch.acknowledgements.len())
        {
            if cancel.is_cancelled() {
                return Err(Error::Cancelled);
            }
            let outcome = if let Some(old) = view.outcomes.iter().find(|r| r.source == event.source)
            {
                if old.digest != event.digest {
                    owner.approval(Command::Quarantine).await?;
                    return Err(Error::Conflict);
                }
                summary.replayed += 1;
                old.outcome.clone()
            } else if let Some(input) = event.input() {
                match self.domain.approval_verdict_receipt(input.clone()).await {
                    Ok(Some(_)) => {
                        summary.replayed += 1;
                        event.outcome()
                    }
                    Ok(None) => {
                        let current = match self
                            .domain
                            .approval_intake_target(input.target.request_id.clone())
                            .await
                        {
                            Ok(target) => target == input.target,
                            Err(
                                hagency_store::Error::RunnerAuthority
                                | hagency_store::Error::Generation
                                | hagency_store::Error::Conflict
                                | hagency_store::Error::State
                                | hagency_store::Error::NotFound,
                            ) => false,
                            Err(error) => return Err(error.into()),
                        };
                        if !current {
                            // The initial historical read can race an exact decision
                            // that made this target non-pending before the current read.
                            match self.domain.approval_verdict_receipt(input).await {
                                Ok(Some(_)) => {
                                    summary.replayed += 1;
                                    event.outcome()
                                }
                                Ok(None) => {
                                    summary.rejected += 1;
                                    Outcome::Rejected
                                }
                                Err(
                                    hagency_store::Error::Conflict
                                    | hagency_store::Error::RunnerAuthority,
                                ) => {
                                    owner.approval(Command::Quarantine).await?;
                                    return Err(Error::Conflict);
                                }
                                Err(error) => return Err(error.into()),
                            }
                        } else {
                            if !allow_new {
                                return Ok(summary);
                            }

                            // Fresh proof is a prerequisite to each possible grant;
                            // writer scope/lease/expiry checks run again after queueing.
                            self.refresh_approval_rooms(&batch.rooms, cancel).await?;
                            let users: BTreeSet<_> = batch
                                .rooms
                                .iter()
                                .flat_map(|r| {
                                    [r.authority.owner_mxid.clone(), r.authority.bot_mxid.clone()]
                                })
                                .collect();
                            let body = crate::outgoing::state::encode(
                                &json!({"device_keys":users.into_iter().map(|u|(u,json!([]))).collect::<serde_json::Map<_,_>>()}),
                                256 * 1024,
                            )?;
                            let current = self
                                .http
                                .post(&["_matrix", "client", "v3", "keys", "query"], body, cancel)
                                .await?
                                .success()?;
                            if crate::approval_batch::hash(&current)?
                                != crate::approval_batch::hash(&batch.keys)?
                            {
                                for room in &batch.rooms {
                                    self.domain
                                        .fence_approval_room(
                                            room.authority.clone(),
                                            room.device.clone(),
                                            room.generation,
                                            None,
                                        )
                                        .await?;
                                }
                                return Err(Error::Recipients);
                            }
                            if cancel.is_cancelled() {
                                return Err(Error::Cancelled);
                            }
                            #[cfg(test)]
                            if fault == 3 {
                                self.handoff_reached.notify_one();
                                self.handoff_continue.notified().await;
                            }
                            if cancel.is_cancelled() {
                                return Err(Error::Cancelled);
                            }
                            match self.domain.admit_approval_verdict(input.clone()).await {
                                Ok(_) => {
                                    #[cfg(test)]
                                    if fault == 2 {
                                        return Err(Error::OutcomeUnknown);
                                    }
                                    summary.accepted += 1;
                                    event.outcome()
                                }
                                Err(
                                    hagency_store::Error::RunnerAuthority
                                    | hagency_store::Error::Generation
                                    | hagency_store::Error::Conflict
                                    | hagency_store::Error::State
                                    | hagency_store::Error::NotFound,
                                ) => {
                                    // Another exact host may have committed between the
                                    // historical read and this rejected current attempt.
                                    match self.domain.approval_verdict_receipt(input).await {
                                        Ok(Some(_)) => {
                                            summary.replayed += 1;
                                            event.outcome()
                                        }
                                        Ok(None) => {
                                            summary.rejected += 1;
                                            Outcome::Rejected
                                        }
                                        Err(
                                            hagency_store::Error::Conflict
                                            | hagency_store::Error::RunnerAuthority,
                                        ) => {
                                            owner.approval(Command::Quarantine).await?;
                                            return Err(Error::Conflict);
                                        }
                                        Err(error) => return Err(error.into()),
                                    }
                                }
                                Err(e) => return Err(e.into()),
                            }
                        }
                    }
                    Err(hagency_store::Error::Conflict | hagency_store::Error::RunnerAuthority) => {
                        owner.approval(Command::Quarantine).await?;
                        return Err(Error::Conflict);
                    }
                    Err(e) => return Err(e.into()),
                }
            } else {
                summary.rejected += 1;
                Outcome::Rejected
            };
            owner
                .approval(Command::Ack(index, outcome))
                .await
                .map_err(|_| Error::OutcomeUnknown)?;
            summary.pending -= 1;
        }
        owner
            .approval(Command::Finish)
            .await
            .map_err(|_| Error::OutcomeUnknown)?;
        Ok(summary)
    }
}

#[cfg(test)]
#[path = "../tests/approval_intake/mod.rs"]
mod tests;
