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

/// A frame that never accepted a byte was never transmitted: a transport
/// refusal there is the peer being gone — **non-uncertain by construction**
/// (nothing was sent, so there is no lost response to be uncertain about and
/// no idempotency question). Any byte accepted keeps the existing verdicts:
/// the send refusal itself stays `Protocol`, and a *written* frame whose
/// acknowledgement was lost keeps the reconcile's uncertainty (ADR-046).
/// `zero_accepted` comes from the transport's termination snapshot
/// (`unconfirmed_write.accepted_bytes == 0`, or no writer at all).
fn send_failure(error: hagency_runtime::codex::session::Error, zero_accepted: bool) -> Failure {
    match error {
        hagency_runtime::codex::session::Error::Transport(
            hagency_runtime::codex::transport::Error::Io(_)
            | hagency_runtime::codex::transport::Error::PeerEof
            | hagency_runtime::codex::transport::Error::HostClosed
            | hagency_runtime::codex::transport::Error::Closed,
        ) if zero_accepted => Failure::PeerUnavailable,
        _ => Failure::Protocol,
    }
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
                .map_err(|_| Failure::LostAuthority)?,
        );
        runner
            .enable_approval_control(self.callbacks.host.policy())
            .map_err(|_| Failure::Admission)
    }

    pub(crate) async fn drive(
        &mut self,
        mut drive: Drive<'_>,
        runner: &mut OwnedSession,
    ) -> Result<(), Failure> {
        let (domain, cap, cancel, until) = (drive.domain, drive.cap, drive.cancel, drive.until);
        loop {
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
            let current = observed.output.map_err(|_| Failure::LostAuthority)?;
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
                if Instant::now() >= entry.owner_deadline {
                    return Err(Failure::Deadline);
                }
                let allow = choice != ApprovalChoice::Deny;
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
                let grant = authorized.output.map_err(|_| Failure::LostAuthority)?;
                #[cfg(test)]
                if self.callbacks.fault == Some(super::Fault::ConsumeAck) {
                    drop(grant);
                    return Err(Failure::LostAuthority);
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
                    return Err(Failure::ApprovalCancelled);
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
                    begun.output.map_err(|_| Failure::LostAuthority)?;
                    #[cfg(test)]
                    if self.callbacks.fault == Some(super::Fault::BeginAck) {
                        return Err(Failure::LostAuthority);
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
                        return Err(Failure::ApprovalCancelled);
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
                checked.output.map_err(|_| Failure::LostAuthority)?;
                // Deliberate decision (ADR-046 amendment 2026-09-12): a turn
                // end arriving on this pump while the frame is armed maps to
                // `ApprovalCancelled`, unlike the four other pumps that map a
                // turn end to a clean exit. A turn end invalidates
                // transmission — the wire is closed — so an armed-but-unwritten
                // frame next to a turn end is a cancellation, and the send is
                // never started. Only the resolution arm exempts an in-flight
                // frame; a turn end never does.
                if checked.terminal? {
                    return Err(Failure::ApprovalCancelled);
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
                let step = crate::operation::bounded(
                    runner.send_prepared_approval(&mut sending.prepared),
                    cancel,
                    until,
                )
                .await?
                .map_err(|error| {
                    // A frame that never accepted a byte was never
                    // transmitted, so a refusal here is the peer being gone,
                    // not a protocol fault — and never an uncertainty: there
                    // is no lost response to be uncertain about. The
                    // accepted-byte count lives in the transport's
                    // termination snapshot (the error itself does not carry
                    // it), and the transport's `Io` now names the stream arm.
                    let zero_accepted = runner
                        .transport_termination()
                        .and_then(|termination| termination.unconfirmed_write.as_ref())
                        .is_none_or(|write| write.accepted_bytes == 0);
                    send_failure(error, zero_accepted)
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
    //! The never-transmitted verdict (ADR-046 amendment): a frame whose peer
    //! vanished before its first byte is a distinct, non-uncertain refusal —
    //! never a protocol fault, never an uncertainty. Any byte accepted keeps
    //! the existing verdicts; a written frame's lost acknowledgement stays
    //! with the reconcile.
    use super::send_failure;
    use crate::Failure;
    use hagency_runtime::codex::{session, transport};

    #[test]
    fn native_never_transmitted_frame_is_peer_unavailable() {
        for cause in [
            transport::Error::Io("stdin write"),
            transport::Error::PeerEof,
            transport::Error::HostClosed,
            transport::Error::Closed,
        ] {
            assert_eq!(
                send_failure(session::Error::Transport(cause), true),
                Failure::PeerUnavailable,
                "zero accepted bytes must be the named refusal"
            );
        }
    }

    #[test]
    fn native_partial_write_keeps_protocol_and_uncertainty() {
        // The negative control: once any byte is accepted the frame was
        // transmitted, so the send refusal is the fencing verdict and the
        // written frame's fate belongs to the reconcile's uncertainty rules.
        assert_eq!(
            send_failure(
                session::Error::Transport(transport::Error::Io("stdin write")),
                false
            ),
            Failure::Protocol
        );
        // A genuine protocol error is never re-labelled, at any offset.
        assert_eq!(
            send_failure(session::Error::Transport(transport::Error::Capacity), true),
            Failure::Protocol
        );
    }
}
