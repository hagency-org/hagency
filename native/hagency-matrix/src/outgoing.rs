pub(crate) mod state;
use crate::collector::observe;
use crate::{CancellationToken, Collector, Error, collector::Inner, sdk::Owner};
use hagency_core::{ingress::VerifiedNoticeClaim, replies::*};
use hagency_matrix_format::MatrixContent;
use serde_json::json;
use state::{Attempt, Command, Kind, Phase};
use std::{collections::BTreeSet, time::Duration};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutgoingState {
    Idle,
    Delivered,
    Uncertain,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutgoingSummary {
    pub id: Option<String>,
    pub state: OutgoingState,
    pub replayed: bool,
}
pub(crate) enum Source {
    Final(ReplyClaim),
    Notice(Box<VerifiedNoticeClaim>),
    File(Box<crate::upload::publication::FileSource>),
    Resume,
}
impl Collector {
    /// Existing host claim only. No caller-selected Matrix path or content.
    pub async fn send_final(
        &self,
        claim: ReplyClaim,
        cancel: &CancellationToken,
    ) -> Result<OutgoingSummary, Error> {
        self.outgoing_job(Source::Final(claim), cancel).await
    }
    pub async fn send_notice(
        &self,
        claim: VerifiedNoticeClaim,
        cancel: &CancellationToken,
    ) -> Result<OutgoingSummary, Error> {
        self.outgoing_job(Source::Notice(Box::new(claim)), cancel)
            .await
    }
    /// Settles journaled acceptance. Uncertain/prepared work is inspect-only;
    /// reopening this method never starts another HTTP write.
    pub async fn resume_outgoing_custody(
        &self,
        cancel: &CancellationToken,
    ) -> Result<OutgoingSummary, Error> {
        self.outgoing_job(Source::Resume, cancel).await
    }
    async fn outgoing_job(
        &self,
        source: Source,
        cancel: &CancellationToken,
    ) -> Result<OutgoingSummary, Error> {
        let permit = self
            .inner
            .busy
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Busy)?;
        let inner = self.inner.clone();
        let cancel = cancel.child_token();
        #[cfg(test)]
        let observation = crate::collector::observation::current();
        let job = async move {
            let _permit = permit;
            let work = inner.outgoing(source, &cancel);
            tokio::pin!(work);
            tokio::select! {
                result = &mut work => result,
                _ = tokio::time::sleep(Duration::from_secs(45)) => {
                    cancel.cancel();
                    // Accepted results still finish bounded SDK journal custody.
                    work.await
                },
            }
        };
        #[cfg(test)]
        let job = crate::collector::observation::owned(observation, job);
        tokio::spawn(job).await.map_err(|_| Error::OutcomeUnknown)?
    }
}
impl Inner {
    pub(crate) async fn outgoing(
        &self,
        mut source: Source,
        cancel: &CancellationToken,
    ) -> Result<OutgoingSummary, Error> {
        observe!(OwnerLock);
        let mut guard = self.owner.lock().await;
        if guard.is_none() {
            // Resume uses protected old identity/journal solely for receipt
            // recovery; it cannot restore transport observations or send.
            if matches!(source, Source::Resume) {
                observe!(OpenOwner);
                *guard = Some(Owner::open_existing(&self.config).await?);
            } else {
                let expected = self.expected_transport().await?;
                observe!(Whoami);
                if let Err(error) = self.whoami(cancel).await {
                    #[cfg(test)]
                    crate::collector::observation::primary(error);
                    return self.fence_observation(expected, error).await;
                }
                observe!(OpenOwner);
                *guard = Some(Owner::open(&self.config).await?);
            }
        }
        let owner = guard.as_ref().ok_or(Error::Storage)?;
        let view = owner.outgoing(Command::Read).await?;
        if let Source::Resume = source {
            return match view.attempt {
                Some(attempt) if attempt.phase == Phase::Complete => {
                    self.settle_outgoing(owner, attempt, true).await
                }
                Some(attempt) => Ok(OutgoingSummary {
                    id: Some(attempt.id),
                    state: OutgoingState::Uncertain,
                    replayed: true,
                }),
                None => self.resume_settled_file_publication(&view.receipts).await,
            };
        }
        if view.attempt.is_some() {
            return Err(Error::OutcomeUnknown);
        }
        let (kind, id, fence, domain_digest, route, transaction_id, body) =
            match &source {
                Source::Final(claim) => {
                    if view.receipts.iter().any(|r| {
                        r.kind == Kind::Final && r.id == claim.id && r.fence == claim.fence
                    }) {
                        return Ok(OutgoingSummary {
                            id: Some(claim.id.clone()),
                            state: OutgoingState::Delivered,
                            replayed: true,
                        });
                    }
                    observe!(OutgoingPreview);
                    let send = self.domain.preview_final_reply(claim.clone()).await?;
                    (
                        Kind::Final,
                        send.id,
                        claim.fence,
                        send.digest,
                        send.route,
                        send.transaction_id,
                        send.body,
                    )
                }
                Source::Notice(claim) => {
                    observe!(OutgoingPreview);
                    let receipt = self
                        .domain
                        .verified_notice_receipt(claim.claim.notice.id.clone())
                        .await?;
                    if view.receipts.iter().any(|r| {
                        r.kind == Kind::Notice && r.id == receipt.id && r.fence == receipt.fence
                    }) {
                        return Ok(OutgoingSummary {
                            id: Some(receipt.id),
                            state: OutgoingState::Delivered,
                            replayed: true,
                        });
                    }
                    if receipt.state != "claimed" {
                        return Err(Error::Domain);
                    }
                    (
                        Kind::Notice,
                        claim.claim.notice.id.clone(),
                        receipt.fence,
                        claim.digest.clone(),
                        claim.route.clone(),
                        claim.claim.notice.transaction_id.clone(),
                        claim.claim.notice.body.clone(),
                    )
                }
                Source::File(file) => {
                    let l = &file.locator;
                    self.domain
                        .validate_file_publication(file.cap.clone(), file.claim.clone())
                        .await?;
                    (
                        Kind::File,
                        l.delivery_id.clone(),
                        l.fence,
                        l.content_digest.clone(),
                        l.route.clone(),
                        l.transaction_id.clone(),
                        String::new(),
                    )
                }
                Source::Resume => unreachable!(),
            };
        if view.receipts.len() >= state::MAX_RECEIPTS {
            return Err(Error::Capacity);
        }
        let joined = self.outgoing_preflight(&route, cancel).await?;
        let draft = if let Source::File(file) = &mut source {
            let mut start = file.start.take().ok_or(Error::Conflict)?;
            start.joined = joined;
            owner
                .outgoing(Command::StartFile(Box::new(start)))
                .await?
                .attempt
                .ok_or(Error::Storage)?
        } else {
            let mut content =
                json!({"msgtype":if kind==Kind::Notice {"m.notice"}else{"m.text"},"body":body});
            if let Some(root) = &route.thread_root {
                content["m.relates_to"] = json!({"rel_type":"m.thread","event_id":root,"is_falling_back":true,"m.in_reply_to":{"event_id":root}});
            }
            let content = MatrixContent::new(content)
                .and_then(|c| c.formatted())
                .map_err(|_| Error::Capacity)?
                .into_value();
            let content_digest = state::hash(state::encode(&content, state::MAX_EVENT)?.as_bytes());
            let draft = Attempt {
                kind,
                id: id.clone(),
                fence,
                domain_digest,
                route,
                transaction_id,
                content,
                content_digest,
                identity: String::new(),
                joined,
                phase: Phase::BeforeBegin,
                query_id: None,
                query_body: None,
                query_response: None,
                keys_digest: None,
                writes: vec![],
                index: 0,
                file: None,
            };
            owner
                .outgoing(Command::Start(Box::new(draft.clone())))
                .await?;
            draft
        };
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        observe!(OutgoingBegin);
        match &source {
            Source::Final(claim) => {
                let begun = self.domain.begin_final_reply_send(claim.clone()).await?;
                if begun.id != id
                    || begun.digest != draft.domain_digest
                    || begun.route != draft.route
                    || begun.transaction_id != draft.transaction_id
                    || begun.body != body
                {
                    return Err(Error::Conflict);
                }
            }
            Source::Notice(claim) => {
                let begun = self
                    .domain
                    .begin_verified_task_notice_send(id.clone(), claim.claim.token.clone())
                    .await?;
                if begun.fence != fence
                    || begun.digest != draft.domain_digest
                    || begun.route != draft.route
                    || begun.notice.transaction_id != draft.transaction_id
                    || begun.notice.body != body
                {
                    return Err(Error::Conflict);
                }
            }
            Source::File(file) => {
                self.domain
                    .validate_file_publication(file.cap.clone(), file.claim.clone())
                    .await?;
            }
            Source::Resume => unreachable!(),
        }
        #[cfg(test)]
        if self
            .outgoing_fault
            .compare_exchange(
                4,
                0,
                std::sync::atomic::Ordering::SeqCst,
                std::sync::atomic::Ordering::SeqCst,
            )
            .is_ok()
        {
            return Err(Error::OutcomeUnknown);
        }
        let mut attempt = owner
            .outgoing(Command::Begun)
            .await?
            .attempt
            .ok_or(Error::Storage)?;
        if attempt.route.encrypted {
            attempt = owner
                .outgoing(Command::Query)
                .await?
                .attempt
                .ok_or(Error::Storage)?;
            observe!(OutgoingQueryHttp);
            let response = self
                .http
                .post(
                    &["_matrix", "client", "v3", "keys", "query"],
                    attempt.query_body.clone().ok_or(Error::Storage)?,
                    cancel,
                )
                .await?
                .success()?;
            attempt = match owner.outgoing(Command::Encrypt(response)).await {
                Ok(view) => view.attempt.ok_or(Error::Storage)?,
                Err(Error::Recipients) => {
                    self.retire_outgoing_room(&attempt.route).await?;
                    return Err(Error::Recipients);
                }
                Err(Error::Identity) => {
                    #[cfg(test)]
                    crate::collector::observation::primary(Error::Identity);
                    return self
                        .fence_observation(self.config.identity.transport.clone(), Error::Identity)
                        .await;
                }
                Err(error) => return Err(error),
            };
        }
        while attempt.index < attempt.writes.len() {
            let joined = self.outgoing_preflight(&attempt.route, cancel).await?;
            if joined != attempt.joined {
                return Err(Error::Generation);
            }
            if attempt.route.encrypted {
                observe!(OutgoingQueryHttp);
                let keys = self
                    .http
                    .post(
                        &["_matrix", "client", "v3", "keys", "query"],
                        attempt.query_body.clone().ok_or(Error::Storage)?,
                        cancel,
                    )
                    .await?
                    .success()?;
                if Some(state::hash(
                    state::encode(&keys, state::MAX_QUERY)?.as_bytes(),
                )) != attempt.keys_digest
                {
                    self.retire_outgoing_room(&attempt.route).await?;
                    return Err(Error::Recipients);
                }
            }
            self.validate_outgoing(&source, attempt.fence).await?;
            if cancel.is_cancelled() {
                return Err(Error::Cancelled);
            }
            let index = attempt.index;
            let write = attempt.writes[index].clone();
            // From this durable point onward, even connect errors/timeouts are
            // uncertain. A stable transaction ID alone is not retry authority.
            owner.outgoing(Command::Possible(index)).await?;
            #[cfg(test)]
            if self
                .outgoing_fault
                .compare_exchange(
                    5,
                    0,
                    std::sync::atomic::Ordering::SeqCst,
                    std::sync::atomic::Ordering::SeqCst,
                )
                .is_ok()
            {
                self.outgoing_reached.notify_one();
                self.outgoing_continue.notified().await;
            }
            // SDK persistence may have queued while a host retired the scope or
            // the lease expired. Recheck after that await, immediately before IO.
            self.validate_outgoing(&source, attempt.fence).await?;
            observe!(OutgoingWriteHttp, Some(index));
            let value = if write.room {
                self.http
                    .put(
                        &[
                            "_matrix",
                            "client",
                            "v3",
                            "rooms",
                            &attempt.route.room_id,
                            "send",
                            &write.event_type,
                            &write.transaction_id,
                        ],
                        write.body,
                        cancel,
                    )
                    .await?
                    .success()?
            } else {
                self.http
                    .put(
                        &[
                            "_matrix",
                            "client",
                            "v3",
                            "sendToDevice",
                            &write.event_type,
                            &write.transaction_id,
                        ],
                        write.body,
                        cancel,
                    )
                    .await?
                    .success()?
            };
            // No cancellation gate between actual accepted response and custody.
            attempt = owner
                .outgoing(Command::Accept(index, value))
                .await?
                .attempt
                .ok_or(Error::Storage)?;
            if attempt.phase == Phase::Complete {
                break;
            }
        }
        self.settle_outgoing(owner, attempt, false).await
    }
    async fn validate_outgoing(&self, source: &Source, fence: u64) -> Result<(), Error> {
        observe!(OutgoingValidate);
        match source {
            Source::Final(claim) => self.domain.validate_final_reply_send(claim.clone()).await?,
            Source::Notice(claim) => {
                self.domain
                    .validate_verified_task_notice_send(
                        claim.claim.notice.id.clone(),
                        claim.claim.token.clone(),
                        fence,
                    )
                    .await?
            }
            Source::File(file) => {
                self.domain
                    .validate_file_publication(file.cap.clone(), file.claim.clone())
                    .await?;
            }
            Source::Resume => return Err(Error::OutcomeUnknown),
        }
        Ok(())
    }
    async fn outgoing_preflight(
        &self,
        route: &ReplyRoute,
        cancel: &CancellationToken,
    ) -> Result<BTreeSet<String>, Error> {
        let t = &self.config.identity.transport;
        if route.engagement_id != t.engagement_id
            || route.registration_generation != t.registration_generation
            || route.transport_generation != t.generation
            || route.sender_mxid != t.sender_mxid
            || route.device_id != t.device_id
            || route.server_name != self.config.identity.server_name
        {
            return Err(Error::Generation);
        }
        let target = self
            .config
            .rooms
            .iter()
            .find(|r| {
                r.room_id == route.room_id
                    && r.generation == route.room_generation
                    && r.privacy == route.privacy
            })
            .ok_or(Error::Generation)?;
        let expected = self.expected_transport().await?;
        observe!(Whoami);
        if let Err(error) = self.whoami(cancel).await {
            #[cfg(test)]
            crate::collector::observation::primary(error);
            return self.fence_observation(expected, error).await;
        }
        let observation = self.collect_room_observation(target, cancel).await?;
        if observation.encrypted != route.encrypted
            || observation.joined.len() > 16
            || (matches!(route.privacy, RoomPrivacy::Direct { .. }) && !route.encrypted)
        {
            return Err(Error::Unsupported);
        }
        Ok(observation.joined)
    }
    async fn retire_outgoing_room(&self, route: &ReplyRoute) -> Result<(), Error> {
        observe!(OutgoingRetireRoom);
        self.domain
            .invalidate_matrix_room(MatrixRoomInvalidation {
                engagement_id: route.engagement_id.clone(),
                registration_generation: route.registration_generation,
                transport_generation: route.transport_generation,
                room_id: route.room_id.clone(),
                generation: route
                    .room_generation
                    .checked_add(1)
                    .ok_or(Error::Capacity)?,
                reason: "Matrix current recipient proof changed".into(),
            })
            .await
            .map_err(Error::from)
    }
    async fn settle_outgoing(
        &self,
        owner: &Owner,
        attempt: Attempt,
        replayed: bool,
    ) -> Result<OutgoingSummary, Error> {
        #[cfg(test)]
        let fault = self
            .outgoing_fault
            .swap(0, std::sync::atomic::Ordering::SeqCst);
        #[cfg(test)]
        if fault == 1 {
            return Err(Error::Busy);
        }
        #[cfg(test)]
        if fault == 3 {
            self.outgoing_reached.notify_one();
            self.outgoing_continue.notified().await;
        }
        observe!(OutgoingSettle);
        let observation = ReplyReconciliation::Delivered(attempt.observation()?);
        match attempt.kind {
            Kind::Final => {
                self.domain
                    .reconcile_final_reply(attempt.id.clone(), attempt.fence, observation)
                    .await?;
            }
            Kind::Notice => {
                self.domain
                    .reconcile_verified_task_notice(attempt.id.clone(), attempt.fence, observation)
                    .await?;
            }
            Kind::File => {
                let binding = attempt.file.as_ref().ok_or(Error::Storage)?;
                let settlement = self
                    .domain
                    .restore_file_delivery_settlement_for_content(
                        binding.locator.clone(),
                        binding.metadata().clone(),
                        binding.captured().clone(),
                    )
                    .await?
                    .ok_or(Error::OutcomeUnknown)?;
                let receipt = attempt.receipt()?;
                self.domain
                    .record_file_delivery_settlement(
                        std::sync::Arc::new(settlement),
                        hagency_core::file_delivery::FileDeliveryAcceptance {
                            transaction_id: attempt.transaction_id.clone(),
                            content_digest: attempt.domain_digest.clone(),
                            event_id: attempt.observation()?.event_id,
                            receipt_id: format!("file_receipt_{}", receipt.attempt_digest),
                            receipt_digest: receipt.attempt_digest,
                        },
                    )
                    .await?;
            }
        }
        #[cfg(test)]
        if fault == 2 {
            return Err(Error::OutcomeUnknown);
        }
        owner.outgoing(Command::Settle).await?;
        let summary = OutgoingSummary {
            id: Some(attempt.id.clone()),
            state: OutgoingState::Delivered,
            replayed,
        };
        if attempt.kind == Kind::File {
            let binding = attempt.file.as_ref().ok_or(Error::Storage)?;
            self.complete_file_publication(
                &binding.locator,
                binding.metadata(),
                binding.captured(),
                &summary,
            )?;
        }
        Ok(summary)
    }
}

#[cfg(test)]
#[path = "../tests/outgoing/mod.rs"]
mod tests;
