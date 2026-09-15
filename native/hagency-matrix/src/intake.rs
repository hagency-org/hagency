use crate::collector::observe;
use crate::{
    CancellationToken, Collector, Error,
    collector::Inner,
    event_batch::{Acknowledgement, Batch, MAX_TARGETS, MAX_TIMELINE, Phase},
    sdk::Owner,
};
use hagency_core::{
    authority::{
        ProjectRequest, RequestObservation, RoomObservation, SourceObservation, verify_request,
    },
    project::identifier,
    replies::{ReplyRoute, RoomAuthorityFacts, RoomPrivacy},
};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

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
    /// ADR-095 provisioning ingress: assemble a `ProjectRequest` +
    /// `RequestObservation` from collector-observed facts only, verify with
    /// the existing `verify_request`, then `admit` exactly once. Returns the
    /// engagement and whether this was a fresh mint (vs an identical replay).
    /// Pre-project: the registration comes from the host identity's recorded
    /// engagement, never from a `ReplyRoute`.
    async fn provision(
        &self,
        msg: &hagency_core::messages::InboundMessage,
    ) -> Result<(hagency_core::project::Engagement, bool), Error> {
        let body: serde_json::Value = serde_json::from_str(&msg.body).map_err(|_| Error::Wire)?;
        let reg = self
            .domain
            .provisioning_registration_for_engagement(
                self.config.identity.transport.engagement_id.clone(),
            )
            .await
            .map_err(|_| Error::Wire)?;
        let owner_room = self
            .domain
            .provisioning_owner_room(
                body.get("requester")
                    .and_then(serde_json::Value::as_str)
                    .ok_or(Error::Wire)?
                    .to_owned(),
                reg.server_name.clone(),
            )
            .await
            .map_err(|_| Error::Wire)?;
        // The retained single-owner flow: the requester is the project owner.
        let requester = body
            .get("requester")
            .and_then(serde_json::Value::as_str)
            .ok_or(Error::Wire)?;
        let request_value = serde_json::json!({
            "v": 1,
            "fleetId": reg.fleet_id,
            "requestId": body.get("requestId").ok_or(Error::Wire)?,
            "requesterMxid": requester,
            "sourceRoomId": msg.room_id,
            "targetProjectId": body.get("project").ok_or(Error::Wire)?,
            "targetRoomId": body.get("projectRoomId").ok_or(Error::Wire)?,
            "ownerMxid": requester,
            "ownerDmRoomId": owner_room.room_id,
            "role": body.get("role").ok_or(Error::Wire)?,
            "requestedTokens": body.get("requestedTokens").ok_or(Error::Wire)?,
            "ratePerDay": body.get("ratePerDay").cloned().unwrap_or(serde_json::Value::Null),
            "authVersion": 1,
            "sourceEventId": msg.event_id,
            "agentDefinition": {
                "name": body.get("agent").ok_or(Error::Wire)?,
                "resourceId": body
                    .get("context")
                    .and_then(|c| c.get("agentDefinition"))
                    .and_then(|a| a.get("resourceId"))
                    .ok_or(Error::Wire)?
            }
        });
        let request: ProjectRequest =
            serde_json::from_value(request_value).map_err(|_| Error::Wire)?;
        // verify_request reconstructs the request from source.content (minus the
        // two private room/event keys) and requires its digest to match.
        let mut source_content = serde_json::to_value(&request).map_err(|_| Error::Wire)?;
        let source_content_obj = source_content.as_object_mut().ok_or(Error::Wire)?;
        source_content_obj.remove("ownerDmRoomId");
        source_content_obj.remove("sourceEventId");
        // The reception and target rooms are verify-only observations: neither
        // is published (observe_matrix_room refuses a room that is not the
        // engagement's own project room), so their authority facts are kept in
        // memory. A room the collector has not observed yet (the request's
        // target room is named by the event, not the host config) is fetched
        // from /state on demand; a missing or unverifiable snapshot is
        // refused, never substituted.
        let (reception_obs, reception_facts) = self
            .verify_room_facts(&msg.room_id, 1, RoomPrivacy::Group {})
            .await?;
        let (project_obs, project_facts) = self
            .verify_room_facts(&request.target_room_id, 1, RoomPrivacy::Group {})
            .await?;
        let room_observation = |obs: &hagency_core::replies::MatrixRoomObservation,
                                facts: &RoomAuthorityFacts| {
            RoomObservation {
                room_id: obs.room_id.clone(),
                joined: obs.joined.clone(),
                invite_only: obs.invite_only,
                encryption: obs.encrypted.then(|| "m.megolm.v1.aes-sha2".to_string()),
                powers: facts.powers.clone(),
                default_power: facts.default_power,
                invite_power: facts.invite_power,
                binding: facts.binding.clone(),
                name: facts.name.clone(),
            }
        };
        let reception = room_observation(&reception_obs, &reception_facts);
        let project = room_observation(&project_obs, &project_facts);
        let owner = RoomObservation {
            room_id: owner_room.room_id.clone(),
            joined: owner_room.joined.clone(),
            invite_only: owner_room.invite_only,
            encryption: owner_room
                .encrypted
                .then(|| "m.megolm.v1.aes-sha2".to_string()),
            powers: BTreeMap::new(),
            default_power: 0,
            invite_power: 0,
            binding: None,
            name: None,
        };
        let request_observation = RequestObservation {
            registration_generation: reg.generation,
            observed_at_ms: msg.origin_ts,
            source: SourceObservation {
                event_id: msg.event_id.clone(),
                room_id: msg.room_id.clone(),
                sender: msg.sender_mxid.clone(),
                event_type: "com.hagency.engagement.request.v1".into(),
                content: source_content,
            },
            reception,
            project,
            owner_room: owner,
        };
        let verified =
            verify_request(&reg, request, request_observation).map_err(|_| Error::Wire)?;
        let id = verified
            .request()
            .engagement_id()
            .map_err(|_| Error::Wire)?;
        let exists = self
            .domain
            .provisioning_engagement_exists(id.clone())
            .await?;
        let engagement = self.domain.admit(verified, msg.origin_ts).await?;
        Ok((engagement, !exists))
    }
    /// ADR-095 provider verdict: the approval event names the requester's
    /// idempotency key (`requestId`); the verified request is rebuilt from the
    /// admitted engagement's stored context + evidence, re-verified against
    /// current room authority (fresh room snapshots, fresh observed_at_ms —
    /// the same re-check the retained `decide()` performs), then `approve` is
    /// called exactly once. The verdict is idempotent on the request id, so a
    /// restored batch replays the prior decision instead of double-reserving.
    async fn approve_provision(
        &self,
        msg: &hagency_core::messages::InboundMessage,
    ) -> Result<(), Error> {
        let body: serde_json::Value = serde_json::from_str(&msg.body).map_err(|_| Error::Wire)?;
        if body.get("decision").and_then(serde_json::Value::as_str) != Some("approve") {
            return Err(Error::Wire);
        }
        let request_id = body
            .get("requestId")
            .and_then(serde_json::Value::as_str)
            .ok_or(Error::Wire)?;
        let reg = self
            .domain
            .provisioning_registration_for_engagement(
                self.config.identity.transport.engagement_id.clone(),
            )
            .await
            .map_err(|_| Error::Wire)?;
        // Only the fleet's representative may deliver the provider verdict.
        if msg.sender_mxid != reg.representative_mxid {
            return Err(Error::Wire);
        }
        let (context, evidence) = self
            .domain
            .provisioning_request_evidence(reg.fleet_id.clone(), request_id.to_owned())
            .await
            .map_err(|_| Error::Wire)?
            .ok_or(Error::Wire)?;
        let request: ProjectRequest =
            serde_json::from_str(&context).map_err(|_| Error::Wire)?;
        let audit: serde_json::Value = serde_json::from_str(&evidence).map_err(|_| Error::Wire)?;
        // The original request event's source observation is the evidence; the
        // approval event's own ids can never satisfy the request-type gate.
        let source_value = audit.get("source").ok_or(Error::Wire)?;
        let owner_room = self
            .domain
            .provisioning_owner_room(request.requester_mxid.clone(), reg.server_name.clone())
            .await
            .map_err(|_| Error::Wire)?;
        let source_room = source_value
            .get("room_id")
            .and_then(serde_json::Value::as_str)
            .ok_or(Error::Wire)?;
        let (reception_obs, reception_facts) = self
            .verify_room_facts(source_room, 1, RoomPrivacy::Group {})
            .await?;
        let (project_obs, project_facts) = self
            .verify_room_facts(&request.target_room_id, 1, RoomPrivacy::Group {})
            .await?;
        let room_observation = |obs: &hagency_core::replies::MatrixRoomObservation,
                                facts: &RoomAuthorityFacts| {
            RoomObservation {
                room_id: obs.room_id.clone(),
                joined: obs.joined.clone(),
                invite_only: obs.invite_only,
                encryption: obs.encrypted.then(|| "m.megolm.v1.aes-sha2".to_string()),
                powers: facts.powers.clone(),
                default_power: facts.default_power,
                invite_power: facts.invite_power,
                binding: facts.binding.clone(),
                name: facts.name.clone(),
            }
        };
        let reception = room_observation(&reception_obs, &reception_facts);
        let project = room_observation(&project_obs, &project_facts);
        let owner = RoomObservation {
            room_id: owner_room.room_id.clone(),
            joined: owner_room.joined.clone(),
            invite_only: owner_room.invite_only,
            encryption: owner_room
                .encrypted
                .then(|| "m.megolm.v1.aes-sha2".to_string()),
            powers: BTreeMap::new(),
            default_power: 0,
            invite_power: 0,
            binding: None,
            name: None,
        };
        let request_observation = RequestObservation {
            registration_generation: reg.generation,
            observed_at_ms: msg.origin_ts,
            source: SourceObservation {
                event_id: source_value
                    .get("event_id")
                    .and_then(serde_json::Value::as_str)
                    .ok_or(Error::Wire)?
                    .to_owned(),
                room_id: source_room.to_owned(),
                sender: source_value
                    .get("sender")
                    .and_then(serde_json::Value::as_str)
                    .ok_or(Error::Wire)?
                    .to_owned(),
                event_type: "com.hagency.engagement.request.v1".into(),
                content: source_value.get("content").cloned().ok_or(Error::Wire)?,
            },
            reception,
            project,
            owner_room: owner,
        };
        let verified =
            verify_request(&reg, request, request_observation).map_err(|_| Error::Wire)?;
        // The request id keys the decision: an identical re-delivery replays
        // the recorded verdict (replay_decision) instead of re-reserving.
        let command_id = format!("approve_{request_id}");
        self.domain
            .approve(command_id, verified, msg.origin_ts)
            .await?;
        // The retained product runs the provision synchronously in its decide
        // handler (ADR-022 "provisions agents on approval"): no async external
        // worker exists in this slice, so the intake claims the just-recorded
        // provision effect and observes it complete in the same handoff. The
        // claim is idempotent on a replayed verdict (claim_effect sees no
        // pending row for an already-completed effect and returns None).
        if let Some(effect) = self.domain.claim_effect().await? {
            self.domain
                .observe_effect(
                    effect.id,
                    effect.fence,
                    hagency_store::EffectOutcome::Applied {
                        receipt: format!("provisioned {request_id}"),
                    },
                )
                .await?;
        }
        Ok(())
    }
    /// The in-memory authority facts for a verify-only room, fetched from
    /// /state on demand when the collector has not observed the room yet.
    async fn verify_room_facts(
        &self,
        room_id: &str,
        generation: u64,
        privacy: RoomPrivacy,
    ) -> Result<
        (
            hagency_core::replies::MatrixRoomObservation,
            RoomAuthorityFacts,
        ),
        Error,
    > {
        if let Some(found) = self.room_facts.lock().await.get(room_id).cloned() {
            return Ok(found);
        }
        let target = crate::HostRoom {
            room_id: room_id.to_owned(),
            generation,
            privacy,
        };
        let state = self
            .http
            .request(
                &["_matrix", "client", "v3", "rooms", room_id, "state"],
                None,
                &crate::CancellationToken::new(),
            )
            .await?
            .success()?;
        let found = self.room(&target, state)?;
        self.room_facts
            .lock()
            .await
            .insert(room_id.to_owned(), found.clone());
        Ok(found)
    }
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
                        "rooms": self.config.observed_rooms().map(|r| &r.room_id).collect::<Vec<_>>(),
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
            for room in self.config.observed_rooms() {
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
                crate::collector::observation::primary(error.clone());
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
        // ADR-095: pre-project provisioning events are admitted before target
        // resolution and carry no route, ack or disposition. Reprocessing is
        // safe: provision is idempotent on `request_id`, so a restored batch
        // replays the prior admission instead of double-counting.
        for msg in batch.pre_project.iter().map(|e| e.observation()) {
            if cancel.is_cancelled() {
                return Err(Error::Cancelled);
            }
            observe!(Provision);
            // ADR-095: the request event mints via admit; the provider's
            // approval event is the separate verdict that reserves via approve.
            let result = if msg.kind == "com.hagency.engagement.approval.v1" {
                self.approve_provision(&msg).await.map(|_| true)
            } else {
                self.provision(&msg).await.map(|(_e, created)| created)
            };
            match result {
                Ok(created) => {
                    if created {
                        admitted += 1;
                    } else {
                        replayed += 1;
                    }
                }
                Err(Error::Conflict) => {
                    observe!(Quarantine, None);
                    owner
                        .intake_quarantine(
                            "provisioning request reused request_id with different content".into(),
                        )
                        .await?;
                    return Err(Error::Generation);
                }
                Err(Error::Wire) => {
                    observe!(Quarantine, None);
                    owner
                        .intake_quarantine(
                            "provisioning request failed verification before admission".into(),
                        )
                        .await?;
                    return Err(Error::Generation);
                }
                Err(error) => return Err(error),
            }
        }
        // A successful provision is retained even if the writer response is
        // lost now: the batch stays pending and the restored handoff replays
        // the admission instead of double-minting the engagement.
        #[cfg(test)]
        if fault == 6 && batch.events.len() == batch.acknowledgements.len() {
            return Err(Error::OutcomeUnknown);
        }
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
