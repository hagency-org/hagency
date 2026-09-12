use crate::collector::observe;
use crate::{
    CancellationToken, Collector, Error,
    collector::Inner,
    event_batch::{Acknowledgement, Batch, MAX_TARGETS, MAX_TIMELINE, Phase},
    sdk::Owner,
};
use hagency_core::{project::identifier, replies::ReplyRoute};
use serde_json::json;
use std::collections::BTreeSet;

/// Inherited host configuration, not a browser/runtime request or crypto proof.
/// Targets must already be current native Matrix sessions; intake cannot create them.
pub struct HostIntakePlan {
    sessions: Vec<String>,
}
impl HostIntakePlan {
    pub fn new(sessions: Vec<String>) -> Result<Self, Error> {
        if sessions.is_empty() || sessions.len() > MAX_TARGETS {
            return Err(Error::Config);
        }
        let mut seen = BTreeSet::new();
        for id in &sessions {
            identifier(id, 128).map_err(|_| Error::Config)?;
            if !seen.insert(id) {
                return Err(Error::Config);
            }
        }
        Ok(Self { sessions })
    }
}
#[derive(Debug, PartialEq, Eq)]
pub struct IntakeSummary {
    pub admitted: usize,
    pub replayed: usize,
    pub filtered: usize,
    pub rejected: usize,
}
/// Safe inspection metadata. Raw events, private rooms, devices and frozen
/// credentials never appear here; exact data remain in the encrypted journal.
#[derive(Debug, PartialEq, Eq)]
pub struct IntakeStatus {
    pub stage: &'static str,
    pub batch_digest: Option<String>,
    pub targets: usize,
    pub events: usize,
    pub acknowledged: usize,
    pub rejected: usize,
    pub raw_bytes: usize,
}
impl IntakeStatus {
    fn of(batch: Option<Batch>) -> Result<Self, Error> {
        Ok(match batch {
            None => Self {
                stage: "idle",
                batch_digest: None,
                targets: 0,
                events: 0,
                acknowledged: 0,
                rejected: 0,
                raw_bytes: 0,
            },
            Some(b) => Self {
                stage: match b.phase {
                    Phase::Prepared => "prepared",
                    Phase::Applying => "sdk_outcome_unknown",
                    Phase::Derived => "domain_handoff",
                    Phase::Quarantined => "quarantined",
                },
                rejected: b.rejected(),
                batch_digest: Some(b.digest),
                targets: b.targets.len(),
                events: b.events.len(),
                acknowledged: b.acknowledgements.len(),
                raw_bytes: serde_json::to_vec(&b.raw)
                    .map_err(|_| Error::Storage)?
                    .len(),
            },
        })
    }
}
impl Collector {
    pub async fn intake(
        &self,
        plan: HostIntakePlan,
        cancel: &CancellationToken,
    ) -> Result<IntakeSummary, Error> {
        let permit = self
            .inner
            .busy
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Busy)?;
        let inner = self.inner.clone();
        let cancel = cancel.clone();
        #[cfg(test)]
        let observation = crate::collector::observation::current();
        let job = async move {
            let _permit = permit;
            inner.intake(plan, &cancel).await
        };
        #[cfg(test)]
        let job = crate::collector::observation::owned(observation, job);
        tokio::spawn(job).await.map_err(|_| Error::OutcomeUnknown)?
    }
    pub async fn intake_status(&self, cancel: &CancellationToken) -> Result<IntakeStatus, Error> {
        let permit = self
            .inner
            .busy
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Busy)?;
        let inner = self.inner.clone();
        let cancel = cancel.clone();
        #[cfg(test)]
        let observation = crate::collector::observation::current();
        let job = async move {
            let _permit = permit;
            // Inspection may read an old quarantined generation, but does not publish a
            // positive observation or expose a constructor for authenticated event data.
            let expected = inner.config.identity.transport.clone();
            if let Err(error) = inner.whoami(&cancel).await {
                return inner.fence_observation(expected, error).await;
            }
            let mut owner = inner.owner.lock().await;
            if owner.is_none() {
                *owner = Some(Owner::open(&inner.config).await?);
            }
            IntakeStatus::of(owner.as_ref().ok_or(Error::Storage)?.batch().await?)
        };
        #[cfg(test)]
        let job = crate::collector::observation::owned(observation, job);
        tokio::spawn(job).await.map_err(|_| Error::OutcomeUnknown)?
    }
}
impl Inner {
    async fn targets(&self, plan: &HostIntakePlan) -> Result<Vec<ReplyRoute>, Error> {
        let mut targets = vec![];
        let mut scopes = BTreeSet::new();
        let identity = &self.config.identity;
        for id in &plan.sessions {
            observe!(Targets);
            let route = self.domain.matrix_intake_route(id.clone()).await?;
            let room = self
                .config
                .rooms
                .iter()
                .find(|r| r.room_id == route.room_id)
                .ok_or(Error::Generation)?;
            if route.engagement_id != identity.transport.engagement_id
                || route.registration_generation != identity.transport.registration_generation
                || route.transport_generation != identity.transport.generation
                || route.sender_mxid != identity.transport.sender_mxid
                || route.device_id != identity.transport.device_id
                || route.server_name != identity.server_name
                || route.room_generation != room.generation
                || route.privacy != room.privacy
                || !scopes.insert((route.room_id.clone(), route.thread_root.clone()))
            {
                return Err(Error::Generation);
            }
            targets.push(route);
        }
        Ok(targets)
    }
    async fn intake(
        &self,
        plan: HostIntakePlan,
        cancel: &CancellationToken,
    ) -> Result<IntakeSummary, Error> {
        let expected = self.expected_transport().await?;
        // Capture current targets before acquiring a new remote response. A resumed
        // handoff uses only its original journal targets, regardless of a new plan.
        let staged = async {
            observe!(Whoami);
            self.whoami(cancel).await?;
            observe!(OwnerLock);
            let mut guard = self.owner.lock().await;
            if guard.is_none() {
                observe!(OpenOwner);
                *guard = Some(Owner::open(&self.config).await?);
            }
            let owner = guard.as_ref().ok_or(Error::Storage)?;
            observe!(Batch);
            let pending = owner.batch().await?;
            if pending.as_ref().is_some_and(|b| b.phase != Phase::Derived) {
                return Err(Error::OutcomeUnknown);
            }
            if pending.is_none() {
                let targets = self.targets(&plan).await?;
                observe!(Cursor);
                let cursor = owner.cursor().await?;
                let filter = json!({
                    "room": {
                        "rooms": self.config.rooms.iter().map(|r| &r.room_id).collect::<Vec<_>>(),
                        "timeline": {"limit": MAX_TIMELINE}, "ephemeral": {"types": []},
                        "account_data": {"types": []}, "state": {"lazy_load_members": false}
                    },
                    "presence": {"types": []}, "account_data": {"types": []}
                })
                .to_string();
                let mut query = vec![
                    ("timeout", "0"),
                    ("full_state", "true"),
                    ("filter", filter.as_str()),
                ];
                if let Some(cursor) = cursor.as_deref() {
                    query.push(("since", cursor));
                }
                observe!(SyncHttp);
                let value = self
                    .http
                    .request(&["_matrix", "client", "v3", "sync"], Some(&query), cancel)
                    .await?
                    .success()?;
                self.sync_bounds(&value)?;
                // Once a complete response is accepted, cancellation cannot discard it
                // before protected custody is written and the SDK owner takes over.
                observe!(IntakeStart);
                owner.intake_start(value, targets).await?;
            }
            observe!(PublishTransport);
            self.domain
                .observe_matrix_transport(self.config.identity.transport.clone())
                .await?;
            for room in &self.config.rooms {
                self.collect_room(room, cancel).await?;
            }
            observe!(Batch);
            owner.batch().await
        }
        .await;
        let batch = match staged {
            Ok(batch) => batch,
            Err(error) => {
                #[cfg(test)]
                crate::collector::observation::primary(error);
                return self.fence_observation(expected, error).await;
            }
        };
        let Some(batch) = batch else {
            return Ok(IntakeSummary {
                admitted: 0,
                replayed: 0,
                filtered: 0,
                rejected: 0,
            });
        };
        self.handoff(batch, cancel).await
    }
    async fn handoff(
        &self,
        batch: Batch,
        cancel: &CancellationToken,
    ) -> Result<IntakeSummary, Error> {
        observe!(HandoffLock);
        let guard = self.owner.lock().await;
        let owner = guard.as_ref().ok_or(Error::Storage)?;
        #[cfg(test)]
        let fault = self
            .handoff_fault
            .swap(0, std::sync::atomic::Ordering::SeqCst);
        #[cfg(test)]
        if fault == 1 {
            return Err(Error::Busy);
        }
        let mut admitted = 0;
        let mut replayed = batch.acknowledgements.len();
        for (index, event) in batch
            .events
            .iter()
            .enumerate()
            .skip(batch.acknowledgements.len())
        {
            if cancel.is_cancelled() {
                return Err(Error::Cancelled);
            }
            let observation = event.observation();
            let attachment = event.attachment_observation()?;
            // Historical read can only acknowledge an exact existing commit. It cannot
            // admit or project the event through stale or replacement authority.
            observe!(HistoricalReceipt, Some(index));
            let historical_result = if let Some(input) = &attachment {
                self.domain.matrix_attachment_receipt(input.clone()).await
            } else {
                self.domain
                    .matrix_ingress_receipt(observation.clone())
                    .await
            };
            let historical = match historical_result {
                Ok(receipt) => receipt,
                Err(hagency_store::Error::RunnerAuthority | hagency_store::Error::Conflict) => {
                    observe!(Quarantine, Some(index));
                    owner
                        .intake_quarantine(
                            "historical receipt conflicts with frozen scope or content".into(),
                        )
                        .await?;
                    return Err(Error::Generation);
                }
                Err(error) => return Err(error.into()),
            };
            let receipt = match historical {
                Some(receipt) => {
                    replayed += 1;
                    receipt
                }
                None => {
                    let current = &self.config.identity.transport;
                    if event.route.engagement_id != current.engagement_id
                        || event.route.registration_generation != current.registration_generation
                        || event.route.transport_generation != current.generation
                        || event.route.sender_mxid != current.sender_mxid
                        || event.route.device_id != current.device_id
                    {
                        observe!(Quarantine, Some(index));
                        owner
                            .intake_quarantine(
                                "unadmitted event has a retired source incarnation".into(),
                            )
                            .await?;
                        return Err(Error::Generation);
                    }
                    #[cfg(test)]
                    if fault == 3 {
                        self.handoff_reached.notify_one();
                        self.handoff_continue.notified().await;
                    }
                    observe!(Admission, Some(index));
                    let admitted_result = if let Some(input) = attachment {
                        self.domain.admit_matrix_attachment(input).await
                    } else {
                        self.domain.admit_matrix_event(observation).await
                    };
                    match admitted_result {
                        Ok(receipt) => {
                            if receipt.projected {
                                admitted += 1;
                            } else {
                                replayed += 1;
                            }
                            receipt
                        }
                        Err(
                            hagency_store::Error::RunnerAuthority
                            | hagency_store::Error::Generation
                            | hagency_store::Error::Conflict
                            | hagency_store::Error::State,
                        ) => {
                            observe!(Quarantine, Some(index));
                            owner
                                .intake_quarantine(
                                    "domain refused the frozen event scope or content".into(),
                                )
                                .await?;
                            return Err(Error::Generation);
                        }
                        Err(error) => return Err(error.into()),
                    }
                }
            };
            // A successful domain result is retained even if cancellation arrives now.
            // A lost writer response exits above, keeping this same event pending.
            #[cfg(test)]
            if fault == 5 {
                self.handoff_reached.notify_one();
                self.handoff_continue.notified().await;
                return Err(Error::OutcomeUnknown);
            }
            #[cfg(test)]
            if fault == 2 {
                return Err(Error::OutcomeUnknown);
            }
            observe!(Acknowledge, Some(index));
            owner
                .intake_ack(batch.digest.clone(), index, Acknowledgement::from(&receipt))
                .await?;
        }
        let rejected = batch.rejected();
        observe!(Finish);
        owner.intake_finish(batch.digest).await?;
        Ok(IntakeSummary {
            admitted,
            replayed,
            filtered: batch.filtered,
            rejected,
        })
    }
}

#[cfg(test)]
#[path = "../tests/intake/mod.rs"]
mod tests;
