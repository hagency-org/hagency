use super::state::Callbacks;
use crate::{Failure, RuntimeObservation, RuntimeStage, SettlementCause, usage::UsageRun};
use hagency_core::tasks::{RunnerCapability, TaskState};
use hagency_runtime::{
    codex::session::{ControlUpdate, Observation, Update},
    owned::OwnedSession,
};
use hagency_store::DomainStore;
use std::{future::Future, sync::atomic::AtomicBool};
use tokio::time::Instant;

pub(crate) struct Drive<'a> {
    pub domain: &'a DomainStore,
    pub cap: &'a RunnerCapability,
    pub cancel: &'a AtomicBool,
    pub until: Instant,
    pub status: &'a mut Option<TaskState>,
    pub usage: &'a mut UsageRun,
    pub observation: &'a mut Option<RuntimeObservation>,
    /// Diagnostic cause for a `Failure::SettlementUnknown` raised during this
    /// drive. Mirrors `status`: the `Report` is owned by `operation.rs` and is
    /// not reachable from the approval control loop. Diagnostic only — it grants
    /// no retry, reply, lease or completion authority.
    pub settlement_cause: &'a mut Option<SettlementCause>,
}
pub(super) struct Pumped<T> {
    pub output: T,
    pub terminal: Result<bool, Failure>,
}
impl Drive<'_> {
    pub(super) async fn update(
        &mut self,
        callbacks: &mut Callbacks,
        runner: &mut OwnedSession,
        update: Update,
        observation: Observation,
    ) -> Result<bool, Failure> {
        // Retain callback/resolution facts in the original owner BEFORE any
        // usage or request receipt can await or unwind.
        let (request, terminal) = match update {
            Update::Approval(request) => (
                Some(callbacks.retain(runner, request, self.until)?),
                Ok(false),
            ),
            Update::ApprovalResolved { id } => {
                let entry = callbacks.entries.get_mut(&id).ok_or(Failure::Protocol)?;
                let terminal = entry.resolution_arrives();
                #[cfg(any(test, feature = "test-diagnostics"))]
                if terminal.is_err() {
                    super::diagnostics::cancellation(
                        "resolved-before-write",
                        &format!("{:?}", entry.request.id()),
                        entry.trace.as_slice(),
                    );
                }
                (None, terminal)
            }
            Update::TurnEnded => {
                #[cfg(any(test, feature = "test-diagnostics"))]
                for entry in callbacks.entries.values_mut() {
                    if entry.write.is_none() {
                        entry.mark("turn-ended-unwritten");
                        super::diagnostics::cancellation(
                            "turn-ended-unwritten",
                            &format!("{:?}", entry.request.id()),
                            entry.trace.as_slice(),
                        );
                    }
                }
                (
                    None,
                    if callbacks.entries.values().any(|e| e.write.is_none()) {
                        Err(Failure::ApprovalCancelled)
                    } else {
                        Ok(true)
                    },
                )
            }
            _ => (None, Ok(false)),
        };
        if self.usage.observe(&observation) {
            let receipt = self.usage.record_pending().await;
            // Observe only the actual original writer result. This test seam
            // cannot manufacture a failure, positive receipt or extra read.
            #[cfg(test)]
            if receipt.is_err()
                && let Some(observer) = &callbacks.receipt_observer
            {
                observer
                    .usage_failed
                    .store(true, std::sync::atomic::Ordering::Release);
            }
            receipt.map_err(|_| Failure::SettlementUnknown)?;
        }
        if let Some(key) = request {
            let input = callbacks
                .entries
                .get(&key)
                .ok_or(Failure::Protocol)?
                .input
                .clone();
            let summary = self
                .domain
                .request_owner_approval(self.cap.clone(), input)
                .await
                .map_err(|_| Failure::LostAuthority)?;
            #[cfg(test)]
            if callbacks.fault == Some(super::Fault::RequestAck) {
                return Err(Failure::LostAuthority);
            }
            callbacks.acknowledged(&key, summary)?;
        }
        terminal
    }
    pub(super) async fn pump<F: Future>(
        &mut self,
        callbacks: &mut Callbacks,
        runner: &mut OwnedSession,
        future: F,
    ) -> Pumped<F::Output> {
        tokio::pin!(future);
        loop {
            // Keep both the database future and the current original native
            // read alive across ticks. Only a terminal failure drops that read.
            let step = crate::operation::bounded(
                runner.next_observed_or_control(future.as_mut()),
                self.cancel,
                self.until,
            )
            .await;
            let terminal = match step {
                Ok(Ok(ControlUpdate::Control(output))) => {
                    return Pumped {
                        output,
                        terminal: Ok(false),
                    };
                }
                Ok(Ok(ControlUpdate::Update(update, observation))) => {
                    self.update(callbacks, runner, update, *observation).await
                }
                Ok(Err(_)) => Err(Failure::Protocol),
                Err(failure) => Err(failure),
            };
            if !matches!(terminal, Ok(false)) {
                self.observation.get_or_insert_with(|| {
                    RuntimeObservation::capture(RuntimeStage::Update, runner)
                });
                runner.stop();
                // Stopping cannot cancel a commit. Finish the SAME bounded
                // writer receipt and return its actual value for retention.
                return Pumped {
                    output: future.await,
                    terminal,
                };
            }
        }
    }
}
