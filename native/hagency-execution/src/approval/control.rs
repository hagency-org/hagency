use super::{
    observations::Drive,
    state::{self, ApprovalRun, Sending},
};
use crate::{Failure, SettlementCause, operation::Deadline};
use hagency_core::{
    approvals::{ApprovalChoice, HostApprovalContext},
    tasks::RunnerCapability,
};
use hagency_runtime::{codex::session::PreparedUpdate, owned::OwnedSession};
use hagency_store::{ApprovalResponseObservation, DomainStore};
use std::time::Duration;
use tokio::time::Instant;

/// A frame that never accepted a byte was never transmitted: a peer-side
/// transport refusal there is the peer being gone — **non-uncertain by
/// construction** (nothing was sent, so there is no lost response to be
/// uncertain about and no idempotency question). Any byte accepted means the
/// frame **was** transmitted: its fate is unknown (the peer may hold the
/// bytes), so the uncertain `SettlementUnknown` — never a silent completion,
/// never a protocol fault. `Protocol` stays for genuine malformed-frame
/// refusals. `Error::Closed` (the host-side parse-removed-id sentinel) and
/// `Error::HostClosed` (the host's own action) are never "peer gone" and
/// stay on the non-`PeerUnavailable` arms regardless of offset. The
/// `zero_accepted` input must be **observed** — a snapshot that reports
/// `accepted_bytes == 0` — never defaulted from an absent snapshot
/// (absence means no writer was installed or the transport was already torn
/// down: "we do not know", not "we know nothing left").
fn send_failure(
    error: hagency_runtime::codex::session::Error,
    zero_accepted: bool,
    armed: bool,
) -> Failure {
    match error {
        hagency_runtime::codex::session::Error::Transport(
            hagency_runtime::codex::transport::Error::Io(_)
            | hagency_runtime::codex::transport::Error::PeerEof,
        ) if zero_accepted || !armed => Failure::PeerUnavailable,
        hagency_runtime::codex::session::Error::Transport(
            hagency_runtime::codex::transport::Error::Io(_)
            | hagency_runtime::codex::transport::Error::PeerEof
            | hagency_runtime::codex::transport::Error::HostClosed
            | hagency_runtime::codex::transport::Error::Closed,
        ) => Failure::SettlementUnknown,
        _ => Failure::Protocol,
    }
}

/// H3 totality: every observer of a dead transport routes through the same
/// classifier, with the same evidence rule. The `zero_accepted` fact is
/// read from the runner's termination snapshot here — an OBSERVED
/// `accepted_bytes == 0` (the frame never left), never a default from an
/// absent snapshot. The `armed` fact (Q3) comes from the coordinator's
/// entry state — a frame is armed when an entry holds its prepared frame or
/// is in flight — NEVER from the absence of a write observation: a
/// never-armed entry with a peer-side cause (`PeerEof` with no writer ever
/// installed) is in the zero-byte class too, while an armed entry with no
/// snapshot (the host's `stop()` took the custody) stays uncertain.
pub(super) fn send_failure_with_termination(
    runner: &hagency_runtime::owned::OwnedSession,
    armed: bool,
    error: hagency_runtime::codex::session::Error,
) -> Failure {
    // The post-turn-end window (ADR-046's who-closed-first): after the peer
    // ends its turn the session phase is `Ended`, so `send_prepared_approval`
    // refuses with `Error::State` — NOT `Transport(HostClosed)` — while the
    // termination snapshot carries the real cause. A non-transport session
    // error with a termination present is therefore classified by the
    // termination's own cause, or the verdict silently degrades to
    // `Protocol` (the macOS failure shape). Host-side causes keep the
    // ADR-046 rule below; genuine session errors without a termination
    // stay `Protocol`.
    let error = match (&error, runner.transport_termination()) {
        (hagency_runtime::codex::session::Error::State, Some(termination)) => {
            hagency_runtime::codex::session::Error::Transport(termination.cause)
        }
        _ => error,
    };
    let zero_accepted = runner
        .transport_termination()
        .and_then(|termination| termination.unconfirmed_write.as_ref())
        .is_some_and(|write| write.accepted_bytes == 0);
    send_failure(error, zero_accepted, armed)
}

impl ApprovalRun {
    pub(crate) async fn bind(
        &mut self,
        domain: &DomainStore,
        cap: &RunnerCapability,
        expected: &str,
        context: HostApprovalContext,
        deadline: Deadline,
        runner: &mut OwnedSession,
    ) -> Result<(), Failure> {
        let Deadline { until, expires_at } = deadline;
        self.callbacks.context = Some(context.clone());
        // Retain the sole returned scope before enabling callback control.
        self.scope = Some(
            domain
                .bind_owned_approval_context(
                    cap.clone(),
                    expected.into(),
                    context,
                    until.into_std(),
                    expires_at,
                )
                .await
                .map_err(|error| Failure::lost(crate::AuthoritySite::ApprovalBind, &error))?,
        );
        runner
            .enable_approval_control(self.callbacks.host.policy())
            .map_err(|_| Failure::Admission)?;
        // This coordinator answers an unanswered callback at its owner bound.
        runner
            .enable_owner_wait_expiry()
            .map_err(|_| Failure::Admission)
    }

    pub(crate) async fn drive(
        &mut self,
        mut drive: Drive<'_>,
        runner: &mut OwnedSession,
    ) -> Result<(), Failure> {
        let (domain, cap, cancel, until) = (drive.domain, drive.cap, drive.cancel, drive.until);
        loop {
            // Owner-wait expiry (ADR046 amendment). An approval the owner never
            // answered must not end the agent: the host declines it so the turn
            // can report that approval was not granted in time.
            //  1. `expire_approval` moves the runtime callback to its expired
            //     stage. It writes nothing and grants nothing.
            //  2. `deny_for_owner_wait_expiry` is the durable decision, committed
            //     before any response byte. It is the same decided/deny the
            //     owner's own Deny records.
            //  3. Nothing else here. The decline is prepared, authorized, begun,
            //     rechecked and written by the loop below from the persisted
            //     choice, never from this site's knowledge that it expired.
            // A refused deny sends nothing; an unknown one is LostAuthority, like
            // every other unknown store call in this loop.
            for key in self.callbacks.entries.keys().cloned().collect::<Vec<_>>() {
                let entry = self
                    .callbacks
                    .entries
                    .get_mut(&key)
                    .ok_or(Failure::Protocol)?;
                // A resolved entry has no runtime callback left to expire; the
                // resolution rules own it. An entry already chosen, prepared or
                // on the wire is past the owner's part.
                if entry.expired
                    || entry.resolved
                    || entry.selected.is_some()
                    || entry.prepared.is_some()
                    || entry.write.is_some()
                    || entry.admitted
                    || entry.in_flight
                    || Instant::now() < entry.owner_deadline
                {
                    continue;
                }
                // No durable request yet: nothing to decide against.
                let Some(id) = entry.id.clone() else {
                    continue;
                };
                let owner_expires_at = entry.owner_expires_at;
                runner
                    .expire_approval(&key)
                    .map_err(|_| Failure::Deadline)?;
                entry.expired = true;
                #[cfg(any(test, feature = "test-diagnostics"))]
                entry.mark("owner-wait-expired");
                let denied = drive
                    .pump(
                        &mut self.callbacks,
                        runner,
                        domain.deny_for_owner_wait_expiry(id, owner_expires_at),
                    )
                    .await;
                #[cfg(test)]
                let outcome = if self.callbacks.fault == Some(super::Fault::ExpireAck) {
                    // Test seam: a successful deny converted to the reply-loss
                    // verdict, so the decision is committed while this outcome
                    // is unknown. It can only convert a success into an error.
                    Err(hagency_store::Error::OutcomeUnknown)
                } else {
                    denied.output
                };
                #[cfg(not(test))]
                let outcome = denied.output;
                // `State`: another path decided first. That decision is read
                // below like any other, and an allow first seen after the cutoff
                // is still refused there.
                match outcome {
                    Ok(_) | Err(hagency_store::Error::State) => {}
                    Err(error) => {
                        return Err(Failure::lost(crate::AuthoritySite::ApprovalExpiry, &error));
                    }
                }
                if denied.terminal? {
                    return Ok(());
                }
            }
            let scope = self.scope.as_ref().ok_or(Failure::Admission)?;
            let maintenance = domain.maintain_owned_approval(scope);
            #[cfg(test)]
            let maintenance = {
                let gate = if self.callbacks.fault == Some(super::Fault::MaintainGate) {
                    self.callbacks.gate.take()
                } else {
                    None
                };
                async move {
                    let result = maintenance.await;
                    if result.is_ok()
                        && let Some(gate) = gate
                    {
                        gate.wait().await;
                    }
                    result
                }
            };
            let observed = drive.pump(&mut self.callbacks, runner, maintenance).await;
            let current = observed
                .output
                .map_err(|error| Failure::lost(crate::AuthoritySite::ApprovalMaintain, &error))?;
            *drive.status = Some(current.task.status);
            if observed.terminal? {
                return Ok(());
            }
            // Freeze a typed response only from an actual persisted choice,
            // before the original owner deadline. Encoding grants no authority.
            let keys: Vec<_> = self.callbacks.entries.keys().cloned().collect();
            for key in &keys {
                let entry = self
                    .callbacks
                    .entries
                    .get_mut(key)
                    .ok_or(Failure::Protocol)?;
                if entry.write.is_some() || entry.prepared.is_some() || entry.admitted {
                    continue;
                }
                let Some(summary) = current
                    .approvals
                    .iter()
                    .find(|v| Some(v.id.as_str()) == entry.id.as_deref())
                else {
                    continue;
                };
                let Some(choice) = summary.choice else {
                    continue;
                };
                let allow = choice != ApprovalChoice::Deny;
                // Past the owner cutoff only a deny may still be answered, and
                // only on a callback the host has expired. An allow first seen
                // after the cutoff keeps its refusal: the response reserve never
                // revives an expired decision. The runtime enforces the same
                // rule on its own, so a fault here cannot put an accept on the wire.
                if (allow || !entry.expired) && Instant::now() >= entry.owner_deadline {
                    return Err(Failure::Deadline);
                }
                entry.prepared = Some(
                    runner
                        .prepare_approval(entry.request.response(allow))
                        .map_err(|_| Failure::Protocol)?,
                );
                entry.selected = Some(allow);
                #[cfg(any(test, feature = "test-diagnostics"))]
                entry.mark("prepared");
                let id = entry.id.clone().ok_or(Failure::Protocol)?;
                let authorized = drive
                    .pump(
                        &mut self.callbacks,
                        runner,
                        domain.authorize_approval_response(cap.clone(), id),
                    )
                    .await;
                // Positive consumption is retained even if a cancellation was
                // observed while the original writer receipt was pending.
                let grant = authorized.output.map_err(|error| {
                    Failure::lost(crate::AuthoritySite::ApprovalResponse, &error)
                })?;
                #[cfg(test)]
                if self.callbacks.fault == Some(super::Fault::ConsumeAck) {
                    drop(grant);
                    return Err(Failure::LostAuthority {
                        site: crate::AuthoritySite::ApprovalResponse,
                        cause: crate::AuthorityCause::Other,
                    });
                }
                let entry = self
                    .callbacks
                    .entries
                    .get_mut(key)
                    .ok_or(Failure::Protocol)?;
                entry.grant = Some(grant);
                state::matches(
                    entry.grant.as_ref().ok_or(Failure::Protocol)?,
                    entry,
                    self.callbacks.context.as_ref().ok_or(Failure::Admission)?,
                )?;
                if authorized.terminal? {
                    // The turn-end rule is the authority on quiet ends: a
                    // host-side close with zero accepted bytes already
                    // classified this turn as completed; the pump must not
                    // re-map its own arrival into a cancellation.
                    return Ok(());
                }
            }
            let ready = !self.callbacks.entries.is_empty()
                && self.callbacks.entries.values().all(|e| {
                    e.write.is_some() || e.admitted || (e.prepared.is_some() && e.grant.is_some())
                });
            if ready {
                let deadline = self
                    .callbacks
                    .entries
                    .values()
                    .filter(|e| e.write.is_none())
                    .map(|e| e.response_deadline)
                    .min()
                    .unwrap_or(until)
                    .min(until);
                for (key, entry) in &mut self.callbacks.entries {
                    if entry.write.is_none() && !entry.admitted {
                        self.batch.ids.push(key.clone());
                        self.batch
                            .grants
                            .push(entry.grant.take().ok_or(Failure::Protocol)?);
                        #[cfg(any(test, feature = "test-diagnostics"))]
                        entry.mark("begun");
                    }
                }
                if !self.batch.grants.is_empty() {
                    let begin = domain.begin_approval_responses(
                        cap.clone(),
                        &mut self.batch.grants,
                        deadline.into_std(),
                    );
                    #[cfg(test)]
                    let begin = {
                        let gate = if self.callbacks.fault == Some(super::Fault::BeginGate) {
                            self.callbacks.gate.take()
                        } else {
                            None
                        };
                        async move {
                            let result = begin.await;
                            if result.is_ok()
                                && let Some(gate) = gate
                            {
                                gate.wait().await;
                            }
                            result
                        }
                    };
                    let begun = drive.pump(&mut self.callbacks, runner, begin).await;
                    // Original grants stay in the retained batch on every
                    // failure or unwind; begin is never reconstructed/rearmed.
                    begun.output.map_err(|error| {
                        Failure::lost(crate::AuthoritySite::ApprovalBegin, &error)
                    })?;
                    #[cfg(test)]
                    if self.callbacks.fault == Some(super::Fault::BeginAck) {
                        return Err(Failure::LostAuthority {
                            site: crate::AuthoritySite::ApprovalBegin,
                            cause: crate::AuthorityCause::Other,
                        });
                    }
                    for (key, grant) in self.batch.ids.drain(..).zip(self.batch.grants.drain(..)) {
                        let entry = self
                            .callbacks
                            .entries
                            .get_mut(&key)
                            .ok_or(Failure::Protocol)?;
                        entry.admitted = true;
                        entry.grant = Some(grant);
                        #[cfg(any(test, feature = "test-diagnostics"))]
                        entry.mark("admitted");
                    }
                    if begun.terminal? {
                        // Same quiet-end authority as the authorize pump.
                        return Ok(());
                    }
                }
            }
            // A new observed callback can repark this same attempt after begin.
            // Handle its decision/batch first, retaining any older unsent frame.
            if self
                .callbacks
                .entries
                .values()
                .any(|e| e.write.is_none() && !e.admitted)
            {
                let wake = drive
                    .pump(
                        &mut self.callbacks,
                        runner,
                        tokio::time::sleep(Duration::from_millis(100)),
                    )
                    .await;
                if wake.terminal? {
                    return Ok(());
                }
                continue;
            }
            if self.sending.is_none()
                && let Some((key, entry)) = self
                    .callbacks
                    .entries
                    .iter_mut()
                    .find(|(_, e)| e.admitted && e.write.is_none() && !e.in_flight)
            {
                // The frame is about to be committed to the transport. Set
                // this on the same borrow, BEFORE the recheck pump below,
                // because that pump can deliver this entry's own resolution
                // and must not cancel the frame it is answering.
                entry.in_flight = true;
                #[cfg(any(test, feature = "test-diagnostics"))]
                entry.mark("in-flight");
                self.sending = Some(Sending {
                    id: key.clone(),
                    prepared: entry.prepared.take().ok_or(Failure::Protocol)?,
                    grant: entry.grant.take().ok_or(Failure::Protocol)?,
                });
            }
            if let Some(sending) = &mut self.sending {
                let check = domain.check_approval_response(cap.clone(), &sending.grant);
                #[cfg(test)]
                let check = {
                    let gate = if self.callbacks.fault == Some(super::Fault::RecheckGate) {
                        self.callbacks.gate.take()
                    } else {
                        None
                    };
                    async move {
                        let result = check.await;
                        if result.is_ok()
                            && let Some(gate) = gate
                        {
                            gate.wait().await;
                        }
                        result
                    }
                };
                let checked = drive.pump(&mut self.callbacks, runner, check).await;
                let new_barrier = self
                    .callbacks
                    .entries
                    .values()
                    .any(|e| e.write.is_none() && !e.admitted);
                if new_barrier
                    && matches!(
                        checked.output,
                        Ok(()) | Err(hagency_store::Error::RunnerAuthority)
                    )
                {
                    checked.terminal?;
                    continue;
                }
                checked
                    .output
                    .map_err(|error| Failure::lost(crate::AuthoritySite::ApprovalCheck, &error))?;
                // Deliberate decision (ADR-046 amendment 2026-09-12), revised
                // by the approval-loss verdicts: a turn end on this pump is
                // classified by the turn-end rule, not by this site. When the
                // rule's cancel arm fired (`ApprovalCancelled` was already
                // decided by the rule for a non-in-flight unwritten entry)
                // `?` propagates it; when the rule returned the quiet verdict
                // for a host-side close with zero accepted bytes, this pump
                // honors it with a clean exit — never a re-mapped
                // cancellation, which inverted the in-flight untransmitted
                // scenario's own subject.
                if checked.terminal? {
                    return Ok(());
                }
                #[cfg(any(test, feature = "test-diagnostics"))]
                if let Some(entry) = self.callbacks.entries.get_mut(&sending.id) {
                    entry.mark("checked");
                }
                // The runtime resolved this request before any byte was
                // accepted. The retained ADR-046 rule for a pre-send
                // resolution is the quiet path: the resolution is
                // informational, the armed frame is dropped (its transmit
                // path is gone), the entry keeps `in_flight` so it is never
                // re-selected, and the drive continues — the same quiet
                // completion the pre-admission resolution produces, never a
                // named failure.
                if !runner.prepared_admissible(&sending.id) {
                    let id = sending.id.clone();
                    drop(self.sending.take());
                    if let Some(entry) = self.callbacks.entries.get_mut(&id) {
                        entry.in_flight = true;
                        #[cfg(any(test, feature = "test-diagnostics"))]
                        entry.mark("resolved-before-send");
                    }
                    continue;
                }
                #[cfg(test)]
                if self.callbacks.fault == Some(super::Fault::SendGate)
                    && let Some(gate) = self.callbacks.gate.take()
                {
                    // A pure hold: nothing reads the wire here, so a peer
                    // that leaves during it is first observed by the send.
                    gate.wait().await;
                }
                let step = crate::operation::bounded(
                    runner.send_prepared_approval(&mut sending.prepared),
                    cancel,
                    until,
                )
                .await?
                .map_err(|error| {
                    // The classifier (H2/H3+Q3), one evidence path: the send
                    // site's frame is armed BY DEFINITION (it is the frame
                    // being sent), so the verdict differs only on the
                    // transport's own zero-byte observation. Peer-gone causes
                    // with an observed zero — or a never-armed entry — are
                    // the named refusal; any accepted byte is the uncertain
                    // `SettlementUnknown`; host-side `Closed`/`HostClosed`
                    // and genuine protocol errors never carry the peer-gone
                    // verdict.
                    send_failure_with_termination(runner, true, error)
                })?;
                match step {
                    PreparedUpdate::Update(update, observation) => {
                        if drive
                            .update(&mut self.callbacks, runner, update, *observation)
                            .await?
                        {
                            return Err(Failure::ApprovalCancelled);
                        }
                    }
                    PreparedUpdate::WriteAccepted(write) => {
                        let entry = self
                            .callbacks
                            .entries
                            .get_mut(&sending.id)
                            .ok_or(Failure::Protocol)?;
                        entry.write = Some(write); // actual receipt before any await
                        // The write is accepted: the entry is no longer in
                        // flight, and `write` now carries the receipt. A later
                        // resolution is post-write and is informational.
                        entry.in_flight = false;
                        #[cfg(any(test, feature = "test-diagnostics"))]
                        entry.mark("write-accepted");
                        #[cfg(test)]
                        if self.callbacks.fault == Some(super::Fault::WritePanic) {
                            panic!("actual owned response write unwind");
                        }
                        // ReceiptGate: hold between the transport's write
                        // receipt and the acceptance observation, so a test
                        // can drive a resolution in exactly that window. The
                        // gate does not pump the session, so the resolution
                        // stays unparsed across the hold.
                        #[cfg(test)]
                        if self.callbacks.fault == Some(super::Fault::ReceiptGate)
                            && let Some(gate) = self.callbacks.gate.take()
                        {
                            gate.wait().await;
                        }
                        let written = drive
                            .pump(
                                &mut self.callbacks,
                                runner,
                                domain.observe_approval_response(
                                    &mut sending.grant,
                                    ApprovalResponseObservation::WriteAccepted,
                                ),
                            )
                            .await;
                        // The frame is physically accepted; only the store call that
                        // RECORDS that acceptance may have failed. ADR-053's rule
                        // applies: reconcile before retrying. Exactly one bounded read
                        // decides whether the store recorded it. The frame is NEVER
                        // re-sent and the acceptance write is NEVER re-issued here.
                        //
                        // The read is NOT a snapshot. `approval_response_summary` runs
                        // through the same single-writer FIFO queue as the acceptance
                        // write above, and that queue skips an enqueued job whose caller
                        // stopped waiting (the `!reply.is_closed()` guard in
                        // `call_with_policy`). Once this caller's two-second wait has
                        // expired and dropped its receiver, the read is therefore ordered
                        // behind the abandoned acceptance job's fate: either that job was
                        // skipped (the row stays `response_may_send`, `write_accepted`
                        // 0) or it had already executed (`write_accepted` 1). When the
                        // read answers it is conclusive; when it does not answer at all
                        // (the writer is stalled) it is inconclusive and the operation
                        // stays `SettlementUnknown`. The acceptance write uses
                        // `ReceiverPolicy::CancelIfDropped`, never
                        // `RetainEnqueuedInvalidation`, so an abandoned acceptance job
                        // cannot execute after a negative read.
                        #[cfg(test)]
                        let outcome = if self.callbacks.fault == Some(super::Fault::WriteAckLost) {
                            // Test seam: a successful call converted to the reply-loss
                            // verdict, so the reconcile sees an error while the row is
                            // committed. It can only convert a success into an error; it
                            // can never manufacture an accepted row.
                            Err(hagency_store::Error::OutcomeUnknown)
                        } else {
                            written.output
                        };
                        #[cfg(not(test))]
                        let outcome = written.output;
                        match outcome {
                            Ok(summary) => {
                                // Continue exactly as today: the acceptance record
                                // exists (or the call simply succeeded).
                                let _ = summary;
                            }
                            Err(write_error) => {
                                #[cfg(any(test, feature = "test-diagnostics"))]
                                if let Some(entry) = self.callbacks.entries.get_mut(&sending.id) {
                                    entry.mark("acceptance-lost");
                                    entry.mark(SettlementCause::of(&write_error).label());
                                }
                                #[cfg(not(any(test, feature = "test-diagnostics")))]
                                let _ = &write_error;
                                let id = sending.grant.application().id.clone();
                                match crate::operation::bounded(
                                    domain.approval_response_summary(id),
                                    cancel,
                                    until,
                                )
                                .await?
                                {
                                    Ok(summary) if summary.write_accepted => {
                                        // Reconciled: the store did record the
                                        // acceptance; the reply was simply lost.
                                        // Continue along the successful path.
                                        #[cfg(any(test, feature = "test-diagnostics"))]
                                        if let Some(entry) =
                                            self.callbacks.entries.get_mut(&sending.id)
                                        {
                                            entry.mark("acceptance-reconciled");
                                        }
                                    }
                                    Ok(_) => {
                                        #[cfg(any(test, feature = "test-diagnostics"))]
                                        if let Some(entry) =
                                            self.callbacks.entries.get_mut(&sending.id)
                                        {
                                            entry.mark("acceptance-unrecorded");
                                        }
                                        // The ordered read answered and found no
                                        // accepted row, so this is conclusive: the
                                        // record is genuinely absent. Name the cause
                                        // for the operator and keep the verdict.
                                        // First writer on this path: nothing sets
                                        // `settlement_cause` before the reconcile,
                                        // so a direct assignment is exact here.
                                        *drive.settlement_cause =
                                            Some(SettlementCause::AcceptanceUnrecorded);
                                        return Err(Failure::SettlementUnknown);
                                    }
                                    Err(read_error) => {
                                        #[cfg(any(test, feature = "test-diagnostics"))]
                                        if let Some(entry) =
                                            self.callbacks.entries.get_mut(&sending.id)
                                        {
                                            entry.mark("acceptance-unreconciled");
                                        }
                                        // The read was refused or did not answer in
                                        // time, so nothing is decided: stay
                                        // `SettlementUnknown` with the read's own
                                        // refusal as the cause. The original write
                                        // error is retained in the trace above.
                                        // First writer on this path (see above): the
                                        // read's own refusal is the root cause and is
                                        // recorded directly.
                                        *drive.settlement_cause =
                                            Some(SettlementCause::of(&read_error));
                                        return Err(Failure::SettlementUnknown);
                                    }
                                }
                            }
                        }
                        #[cfg(test)]
                        if self.callbacks.fault == Some(super::Fault::WriteAck) {
                            return Err(Failure::SettlementUnknown);
                        }
                        let sent = self.sending.take().ok_or(Failure::Protocol)?;
                        let entry = self
                            .callbacks
                            .entries
                            .get_mut(&sent.id)
                            .ok_or(Failure::Protocol)?;
                        entry.prepared = Some(sent.prepared);
                        entry.grant = Some(sent.grant);
                        entry.recorded = true;
                        #[cfg(any(test, feature = "test-diagnostics"))]
                        entry.mark("recorded");
                        self.callbacks.release_written();
                        if written.terminal? {
                            return Ok(());
                        }
                    }
                }
            } else {
                let wake = drive
                    .pump(
                        &mut self.callbacks,
                        runner,
                        tokio::time::sleep(Duration::from_millis(100)),
                    )
                    .await;
                if wake.terminal? {
                    return Ok(());
                }
            }
        }
    }
}

#[cfg(test)]
mod send_failure_tests {
    //! The never-transmitted classifier (ADR-046 amendment, review H2/H3):
    //! one function decides the verdict for every transport-observing arm —
    //! the send path, the pump and the turn-end rule. The table below pins
    //! EVERY arm: peer-gone `Io`/`PeerEof` with an observed zero-byte
    //! snapshot is `PeerUnavailable`; the same causes with bytes accepted
    //! are the uncertain `SettlementUnknown`; host-side `Closed` (the
    //! parse-removed-id sentinel) and `HostClosed` (the host's own action)
    //! never carry the peer-gone verdict at ANY offset; genuine protocol
    //! errors stay `Protocol` regardless of the snapshot.
    use super::send_failure;
    use crate::Failure;
    use hagency_runtime::codex::{session, transport};

    #[test]
    fn native_never_transmitted_frame_is_peer_unavailable() {
        // Only the peer-gone causes, only with OBSERVED zero bytes, on an
        // ARMED frame (a snapshot exists implies a writer was installed).
        for cause in [
            transport::Error::Io("stdin write"),
            transport::Error::Io("stdin flush"),
            transport::Error::Io("stdout read"),
            transport::Error::Io("stderr read"),
            transport::Error::PeerEof,
        ] {
            assert_eq!(
                send_failure(session::Error::Transport(cause), true, true),
                Failure::PeerUnavailable,
                "peer-gone {cause:?} with an observed zero-byte snapshot must be the named refusal"
            );
        }
        // Q3's widened class: a NEVER-ARMED entry (no prepared frame, no
        // in-flight flag — the read-side `PeerEof` with no writer ever
        // installed) is in the zero-byte class even without a snapshot,
        // because there is no frame whose fate could be unknown.
        for cause in [
            transport::Error::Io("stdout read"),
            transport::Error::Io("stderr read"),
            transport::Error::PeerEof,
        ] {
            assert_eq!(
                send_failure(session::Error::Transport(cause), false, false),
                Failure::PeerUnavailable,
                "peer-gone {cause:?} with no frame ever armed is the named refusal, not a coin flip"
            );
        }
        // Host-side causes are never "peer gone", even with zero bytes.
        for host_side in [transport::Error::Closed, transport::Error::HostClosed] {
            assert_eq!(
                send_failure(session::Error::Transport(host_side), true, true),
                Failure::SettlementUnknown,
                "host-side {host_side:?} must not carry the peer-gone verdict"
            );
        }
    }

    #[test]
    fn native_partial_write_keeps_protocol_and_uncertainty() {
        // The negative control: once any byte is accepted the frame was
        // transmitted, so its fate is UNKNOWN — the uncertain verdict (never
        // a silent completion, never a protocol fault), and a *recorded*
        // frame's fate belongs to the reconcile's rules. This holds for the
        // host-side causes too (bytes accepted dominates the cause).
        for cause in [
            transport::Error::Io("stdin write"),
            transport::Error::PeerEof,
            transport::Error::HostClosed,
            transport::Error::Closed,
        ] {
            assert_eq!(
                send_failure(session::Error::Transport(cause), false, true),
                Failure::SettlementUnknown,
                "{cause:?} with bytes accepted is uncertain, never silent"
            );
        }
        // A genuine protocol error is never re-labelled, at any offset.
        assert_eq!(
            send_failure(
                session::Error::Transport(transport::Error::Capacity),
                true,
                true
            ),
            Failure::Protocol
        );
        assert_eq!(
            send_failure(
                session::Error::Transport(transport::Error::Capacity),
                false,
                false
            ),
            Failure::Protocol
        );
    }
}
