//! Approval-purpose card transmission. A packet is checked afresh, never a grant.
mod enrollment;
pub(crate) mod jobs;
mod public;
pub(crate) mod state;
use crate::{
    ApprovalCollector, CancellationToken, Error,
    collector::Inner,
    enrollment::checkpoint,
    outgoing::state as wire,
    sdk::{
        Owner,
        approval_delivery::{Command, Handle},
    },
};
use hagency_store::PrivateApprovalCard;
use jobs::Value;
use state::{Frozen, Phase, View};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{sync::OwnedSemaphorePermit, time::Instant};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivateApprovalDeliveryState {
    Idle,
    Accepted,
    Uncertain,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrivateApprovalDeliverySummary {
    pub request_id: Option<String>,
    pub state: PrivateApprovalDeliveryState,
    pub replayed: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivateApprovalDeliveryStage {
    Idle,
    Prepared,
    QueryPrepared,
    CryptoApplying,
    Ready,
    WritePossible,
    ResponseStored,
    Complete,
    Quarantined,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrivateApprovalDeliveryStatus {
    pub stage: PrivateApprovalDeliveryStage,
    pub receipts: usize,
    pub writes: usize,
    pub accepted: usize,
    pub retained_bytes: usize,
}
impl ApprovalCollector {
    pub(crate) fn delivery_permit(&self, historical: bool) -> Result<OwnedSemaphorePermit, Error> {
        let permit = self
            .inner
            .busy
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Busy)?;
        self.jobs.check(historical)?;
        Ok(permit)
    }
    /// Current private checks admit one original send; this never applies a verdict.
    pub async fn send_private_approval_card(
        &self,
        card: Arc<PrivateApprovalCard>,
        cancel: &CancellationToken,
    ) -> Result<PrivateApprovalDeliverySummary, Error> {
        let permit = self.delivery_permit(false)?;
        if self.inner.config.enrollment.is_none()
            || !self
                .engagements
                .contains(&card.target().authority.engagement_id)?
        {
            return Err(Error::Config);
        }
        let frozen = Frozen::new(&card)?;
        // ADR-149: the bound is a DELIVERY clock starting at the first send
        // attempt, not the owner's decision window. The owner expiry keeps
        // governing the owner through the store's verdict gate; the 45 s
        // literal is unchanged from the old expression's ceiling (Rule 4).
        let deadline = Instant::now() + Duration::from_secs(45);
        let inner = self.inner.clone();
        // ADR-149: an owned clone shares this token's cancellation node, so the
        // caller's shutdown cancel still reaches the work; what is gone is the
        // delivery job cancelling a token of its own.
        let cancel = cancel.clone();
        let blocks_after_error = Arc::new(AtomicBool::new(true));
        let original_classification = blocks_after_error.clone();
        let denial_request_id = card.target().request_id.clone();
        let job=self.jobs.start_classified(false,false,permit,blocks_after_error,async move{
            // ADR-149: a budget overrun is a Timeout, never a manufactured
            // cancellation — the arm returns, it does not cancel a token the
            // work itself holds, and it does not await the inner work (a stuck
            // inner hold must not hang the job). The token is the caller's own
            // parent, so shutdown propagation survives; the jobs registry owns
            // this task's lifetime, not the pump.
            let work=inner.deliver_private_card(card,frozen,&cancel,deadline,&original_classification);tokio::pin!(work);
            let result=tokio::select!{biased;_ = cancel.cancelled() => Err(Error::Cancelled),_ = tokio::time::sleep_until(deadline) => Err(Error::Timeout),r=&mut work=>r};
            result.map(Value::Delivery)
        })?;
        match job.wait().await {
            Ok(Value::Delivery(v)) => Ok(v),
            Ok(_) => Err(Error::Storage),
            Err(error) => {
                // Fail-closed (ADR-137, D-PC-FC): a delivery attempt that did
                // not reach `Accepted` denies the pending request with a named
                // reason, at most once — no retry, no packet reconstruction.
                // Errors before the attempt starts (config, busy, an invalid
                // card) are wiring refusals, not send failures, and deny
                // nothing. If the denial write itself fails, that failure is
                // returned instead: the row stays `pending` and the caller
                // sees one honest uncertainty, never a false `decided`.
                let mut reason = format!("private approval card send failed: {error}");
                let mut cut = reason.len().min(256);
                while !reason.is_char_boundary(cut) {
                    cut -= 1;
                }
                reason.truncate(cut);
                match self
                    .inner
                    .domain
                    .deny_for_failed_delivery(denial_request_id, reason)
                    .await
                {
                    Ok(_) => Err(error),
                    Err(denial) => Err(denial.into()),
                }
            }
        }
    }
    /// The redacted public status notice (ADR-137): a content-free status
    /// word in the project room through its own validator. It is not the
    /// request, carries none of its material, and confers no grant and no
    /// authority on anyone who reads it.
    pub async fn send_private_approval_notice(
        &self,
        card: Arc<PrivateApprovalCard>,
        cancel: &CancellationToken,
    ) -> Result<(), Error> {
        let permit = self.delivery_permit(false)?;
        if self.inner.config.enrollment.is_none()
            || !self
                .engagements
                .contains(&card.target().authority.engagement_id)?
        {
            return Err(Error::Config);
        }
        let notice = public::PublicFrozen::new(&card)?;
        let inner = self.inner.clone();
        let engagement = card.target().authority.engagement_id.clone();
        let cancel = cancel.child_token();
        let job = self.jobs.start(false, false, permit, async move {
            // Destination re-derivation from the live rows by the same
            // room_authority path the private card uses: a notice addressed
            // from stale or caller-influenced state is refused here, never
            // repaired.
            let authority = inner.domain.approval_room_authority(engagement).await?;
            if !notice.matches(&authority) {
                return Err(Error::Generation);
            }
            let content = notice.content()?;
            inner
                .http
                .put(
                    &[
                        "_matrix",
                        "client",
                        "v3",
                        "rooms",
                        &authority.project_room_id,
                        "send",
                        notice.msgtype(),
                        &notice.transaction(),
                    ],
                    content,
                    &cancel,
                )
                .await?
                .success()?;
            Ok(Value::Unit)
        })?;
        match job.wait().await? {
            Value::Unit => Ok(()),
            _ => Err(Error::Storage),
        }
    }
    /// Network-free history. It cannot recreate a packet or authorize another PUT.
    pub async fn resume_private_approval_delivery_custody(
        &self,
        _cancel: &CancellationToken,
    ) -> Result<PrivateApprovalDeliverySummary, Error> {
        let permit = self.delivery_permit(true)?;
        let inner = self.inner.clone();
        let job = self.jobs.start(false, true, permit, async move {
            let owner = inner.card_handle().await?;
            let view = owner.command(Command::Read).await?;
            inner.card_history(&owner, view).await.map(Value::Delivery)
        })?;
        match job.wait().await? {
            Value::Delivery(v) => Ok(v),
            _ => Err(Error::Storage),
        }
    }
    pub async fn private_approval_delivery_status(
        &self,
    ) -> Result<PrivateApprovalDeliveryStatus, Error> {
        let permit = self.delivery_permit(true)?;
        let inner = self.inner.clone();
        let job = self.jobs.start(false, true, permit, async move {
            let view = inner.card_handle().await?.command(Command::Read).await?;
            let (stage, writes, accepted, bytes) = match view.attempt {
                None => (PrivateApprovalDeliveryStage::Idle, 0, 0, 0),
                Some(a) => {
                    let stage = match a.phase {
                        Phase::Prepared => PrivateApprovalDeliveryStage::Prepared,
                        Phase::QueryPrepared => PrivateApprovalDeliveryStage::QueryPrepared,
                        Phase::CryptoApplying => PrivateApprovalDeliveryStage::CryptoApplying,
                        Phase::Ready => PrivateApprovalDeliveryStage::Ready,
                        Phase::WritePossible => PrivateApprovalDeliveryStage::WritePossible,
                        Phase::ResponseStored => PrivateApprovalDeliveryStage::ResponseStored,
                        Phase::Complete => PrivateApprovalDeliveryStage::Complete,
                        Phase::Quarantined => PrivateApprovalDeliveryStage::Quarantined,
                    };
                    let bytes = wire::encode(
                        &serde_json::to_value(&a).map_err(|_| Error::Storage)?,
                        wire::MAX_ATTEMPT,
                    )?
                    .len();
                    (
                        stage,
                        a.writes.len(),
                        a.writes.iter().filter(|w| w.response.is_some()).count(),
                        bytes,
                    )
                }
            };
            Ok(Value::Status(PrivateApprovalDeliveryStatus {
                stage,
                receipts: view.receipts.len(),
                writes,
                accepted,
                retained_bytes: bytes,
            }))
        })?;
        match job.wait().await? {
            Value::Status(v) => Ok(v),
            _ => Err(Error::Storage),
        }
    }
}
impl Inner {
    async fn card_handle(&self) -> Result<Handle, Error> {
        let mut guard = self.owner.lock().await;
        if guard.is_none() {
            *guard = Some(Owner::open_existing(&self.config).await?);
        }
        Ok(guard
            .as_ref()
            .ok_or(Error::Storage)?
            .approval_delivery_handle())
    }
    async fn card_history(
        &self,
        owner: &Handle,
        view: View,
    ) -> Result<PrivateApprovalDeliverySummary, Error> {
        if let Some(a) = view.attempt {
            let id = a.card.target.0.request_id;
            if a.phase == Phase::Complete {
                owner.command(Command::Settle).await?;
                return Ok(summary(
                    Some(id),
                    PrivateApprovalDeliveryState::Accepted,
                    true,
                ));
            }
            return Ok(summary(
                Some(id),
                PrivateApprovalDeliveryState::Uncertain,
                true,
            ));
        }
        Ok(view.receipts.last().map_or_else(
            || summary(None, PrivateApprovalDeliveryState::Idle, true),
            |r| {
                summary(
                    Some(r.request_id.clone()),
                    PrivateApprovalDeliveryState::Accepted,
                    true,
                )
            },
        ))
    }
    async fn deliver_private_card(
        &self,
        card: Arc<PrivateApprovalCard>,
        frozen: Frozen,
        cancel: &CancellationToken,
        deadline: Instant,
        blocks_after_error: &AtomicBool,
    ) -> Result<PrivateApprovalDeliverySummary, Error> {
        let owner = self.card_handle().await?;
        let view = owner.command(Command::Read).await?;
        // Only this positive original SDK read proves no pending card mutation.
        // A refusal before Start can then leave the collector usable. Unknown
        // opens/reads, retained attempts and all submitted mutations stay fenced.
        if view.attempt.is_none() {
            blocks_after_error.store(false, Ordering::Release);
        }
        if let Some(receipt) = view
            .receipts
            .iter()
            .find(|r| r.request_id == frozen.target.0.request_id)
        {
            if receipt.card_digest != frozen.digest {
                return Err(Error::Conflict);
            }
            return Ok(summary(
                Some(receipt.request_id.clone()),
                PrivateApprovalDeliveryState::Accepted,
                true,
            ));
        }
        if view.attempt.is_some() {
            return Err(Error::OutcomeUnknown);
        }
        if view.receipts.len() >= state::MAX_RECEIPTS {
            return Err(Error::Capacity);
        }
        checkpoint(cancel, deadline)?;
        self.card_current(&card, cancel, None).await?;
        // Set before even submitting the original SDK command: a lost queue or
        // result acknowledgment cannot be classified as a no-effect refusal.
        blocks_after_error.store(true, Ordering::Release);
        owner.command(Command::Start(Box::new(frozen))).await?;
        let view = owner.command(Command::Query).await?;
        let draft = view.attempt.ok_or(Error::Storage)?;
        checkpoint(cancel, deadline)?;
        let keys = self
            .http
            .post(
                &["_matrix", "client", "v3", "keys", "query"],
                draft.query_body.ok_or(Error::Storage)?,
                cancel,
            )
            .await?
            .success()?;
        wire::encode(&keys, wire::MAX_QUERY)?;
        let encrypted = owner.command(Command::Encrypt(keys)).await;
        let mut attempt = match encrypted {
            Ok(v) => v.attempt.ok_or(Error::Storage)?,
            Err(error) => {
                if matches!(error, Error::Identity | Error::Recipients | Error::Wire) {
                    let r = crate::approval_batch::Room {
                        authority: card.target().authority.clone(),
                        device: card.target().device_id.clone(),
                        generation: card.target().room_generation,
                    };
                    self.fence_approval_candidates(&[r]).await?;
                }
                return Err(error);
            }
        };
        loop {
            let index = attempt.index;
            if attempt.phase == Phase::Complete {
                break;
            }
            checkpoint(cancel, deadline)?;
            let write = attempt.writes.get(index).ok_or(Error::Storage)?;
            owner.command(Command::Possible(index)).await?;
            let acceptance = owner.acceptance()?;
            self.card_current(
                &card,
                cancel,
                Some((&attempt.query_body, &attempt.keys_digest)),
            )
            .await?;
            checkpoint(cancel, deadline)?;
            let response = if write.room {
                self.http
                    .put(
                        &[
                            "_matrix",
                            "client",
                            "v3",
                            "rooms",
                            &card.target().authority.room_id,
                            "send",
                            &write.event_type,
                            &write.transaction_id,
                        ],
                        write.body.clone(),
                        cancel,
                    )
                    .await?
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
                        write.body.clone(),
                        cancel,
                    )
                    .await?
            }
            .success()?;
            // The reserved original command takes actual response custody before
            // another cancellation observation. No retry or replacement frame.
            attempt = acceptance
                .accept(index, response)
                .await?
                .attempt
                .ok_or(Error::Storage)?;
        }
        let id = attempt.card.target.0.request_id;
        owner.command(Command::Settle).await?;
        Ok(summary(
            Some(id),
            PrivateApprovalDeliveryState::Accepted,
            false,
        ))
    }
    async fn card_current(
        &self,
        card: &Arc<PrivateApprovalCard>,
        cancel: &CancellationToken,
        keys: Option<(&Option<String>, &Option<String>)>,
    ) -> Result<(), Error> {
        let t = card.target();
        let rooms = self
            .approval_rooms(std::slice::from_ref(&t.authority.engagement_id))
            .await?;
        if !rooms.iter().any(|r| {
            r.authority == t.authority
                && r.device == t.device_id
                && r.generation == t.room_generation
        }) {
            return Err(Error::Generation);
        }
        self.refresh_approval_rooms(&rooms, cancel).await?;
        if let Some((body, digest)) = keys {
            let response = self
                .http
                .post(
                    &["_matrix", "client", "v3", "keys", "query"],
                    body.as_ref().ok_or(Error::Storage)?.clone(),
                    cancel,
                )
                .await?
                .success()?;
            if wire::hash(wire::encode(&response, wire::MAX_QUERY)?.as_bytes())
                != *digest.as_ref().ok_or(Error::Storage)?
            {
                self.fence_approval_candidates(&rooms).await?;
                return Err(Error::Recipients);
            }
        }
        self.domain
            .check_private_approval_card(card.clone())
            .await
            .map_err(Error::from)
    }
}
fn summary(
    request_id: Option<String>,
    state: PrivateApprovalDeliveryState,
    replayed: bool,
) -> PrivateApprovalDeliverySummary {
    PrivateApprovalDeliverySummary {
        request_id,
        state,
        replayed,
    }
}
#[cfg(test)]
#[path = "../tests/approval_delivery/mod.rs"]
mod tests;
