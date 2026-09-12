//! Private attachment to the one fresh OwnedSession in operation::execute.
//! Restoring a ledger source is deliberately not an attachment constructor.
use hagency_core::tasks::RunnerCapability;
use hagency_metering::{
    observation::UsageObservation,
    runtime_usage::{CodexUsage, CounterBreakdown, ProjectionDiagnostics},
};
use hagency_runtime::{
    codex::session::{
        Observation, ObservationKind, ObservationSource, UsageBreakdown, UsageEvidence,
    },
    owned::OwnedSession,
};
use hagency_store::{DomainStore, OwnedDispatchScope, UsageSource};

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum UsageFailure {
    #[error("usage source does not match the owned session")]
    Source,
    #[error("usage observation sequence is discontinuous")]
    Sequence,
    #[error("usage observation source was retired")]
    Retired,
    #[error("runtime observation invalidated usage capture")]
    Invalidated,
    #[error("usage counters exceed normalization bounds")]
    Normalization,
    #[error("usage write is unconfirmed; retain the exact pending observation")]
    Storage,
}

/// Fixed host-only diagnostics. Counts are observations, never token amounts.
/// No IDs, paths, credentials or provider-authenticated measurement claims.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UsageStatus {
    pub bound: bool,
    pub attached: bool,
    pub closed: bool,
    pub observed: u64,
    pub acknowledged: u64,
    pub pending: bool,
    pub rejected: bool,
    pub failure: Option<UsageFailure>,
    /// Terminal protocol evidence cannot prove that all usage was received.
    pub incomplete: bool,
}
impl Default for UsageStatus {
    fn default() -> Self {
        Self {
            bound: false,
            attached: false,
            closed: true,
            observed: 0,
            acknowledged: 0,
            pending: false,
            rejected: false,
            failure: None,
            incomplete: true,
        }
    }
}

struct Pending {
    call_id: String,
    observation: UsageObservation,
}
pub(super) struct UsageRun {
    domain: DomainStore,
    source: UsageSource,
    // This is the acknowledged start response, never runtime-supplied metadata.
    _started: OwnedDispatchScope,
    stream: Option<ObservationSource>,
    sequence: u64,
    pending: Option<Pending>,
    rejected: Option<UsageEvidence>,
    status: UsageStatus,
}
impl UsageRun {
    pub(super) async fn bind(
        domain: DomainStore,
        capability: RunnerCapability,
        started: OwnedDispatchScope,
    ) -> Result<Self, hagency_store::Error> {
        let source = domain
            .bind_usage_source(capability, started.clone())
            .await?;
        Ok(Self {
            domain,
            source,
            _started: started,
            stream: None,
            sequence: 0,
            pending: None,
            rejected: None,
            status: UsageStatus {
                bound: true,
                closed: false,
                ..UsageStatus::default()
            },
        })
    }
    pub(super) fn status(&self) -> UsageStatus {
        self.status
    }
    pub(super) fn attach(&mut self, runner: &OwnedSession) {
        // Only called after fresh start_thread + start_turn in the private
        // operation. OwnedSession has no resume or caller-owned source API.
        match runner.observation_source() {
            Ok(source) if runner.matches_observation_source(&source) => self.attach_source(source),
            _ => self.fence(UsageFailure::Source),
        }
    }
    fn attach_source(&mut self, source: ObservationSource) {
        if self.stream.is_some() || self.status.closed || source.is_retired() {
            self.fence(UsageFailure::Source);
        } else {
            self.stream = Some(source);
            self.status.attached = true;
        }
    }
    fn fence(&mut self, failure: UsageFailure) {
        self.status.failure.get_or_insert(failure);
        self.close();
    }
    pub(super) fn close(&mut self) {
        self.status.closed = true;
    }
    /// True only for newly retained usage. Never automatically retries a failed
    /// writer for the next runtime notification, and never reads past one slot.
    pub(super) fn observe(&mut self, event: &Observation) -> bool {
        if self.status.closed {
            return false;
        }
        if self.pending.is_some() {
            self.fence(UsageFailure::Storage);
            return false;
        }
        if self.stream.as_ref() != Some(event.source()) {
            self.fence(UsageFailure::Source);
            return false;
        }
        if event.source().is_retired() {
            self.fence(UsageFailure::Retired);
            return false;
        }
        if self.sequence.checked_add(1) != Some(event.sequence()) {
            self.fence(UsageFailure::Sequence);
            return false;
        }
        self.sequence = event.sequence();
        match event.kind() {
            ObservationKind::Invalidated => self.fence(UsageFailure::Invalidated),
            ObservationKind::TurnEnded(_) => self.close(),
            ObservationKind::Usage(evidence) => {
                self.status.observed += 1; // Session MAX_EVENTS is finite.
                let diagnostics = evidence.diagnostics();
                let observation = UsageObservation::codex_runtime(CodexUsage {
                    total: breakdown(evidence.total()),
                    last: breakdown(evidence.last()),
                    context_window: evidence.model_context_window(),
                    diagnostics: ProjectionDiagnostics {
                        missing: diagnostics.has_missing_fields(),
                        invalid: diagnostics.has_invalid_fields(),
                        unsupported: diagnostics.has_unsupported_fields(),
                    },
                });
                match observation {
                    Ok(observation) => {
                        self.pending = Some(Pending {
                            call_id: format!("runtime_v1_{}", event.sequence()),
                            observation,
                        });
                        self.status.pending = true;
                        return true;
                    }
                    Err(_) => {
                        // Preserve the fixed original projection too; an
                        // arithmetic refusal is not a normalized zero snapshot.
                        self.rejected = Some(evidence.clone());
                        self.status.rejected = true;
                        self.fence(UsageFailure::Normalization);
                    }
                }
            }
            ObservationKind::Ignored | ObservationKind::Tool(_) => {}
        }
        false
    }
    /// Pending is kept before and throughout await. Dropping this future cannot
    /// remove it; a retry reuses the same content-bound writer receipt identity.
    pub(super) async fn record_pending(&mut self) -> Result<UsageStatus, UsageFailure> {
        let domain = self.domain.clone();
        self.record_with(move |source, call, observation| async move {
            domain
                .record_usage_observation(source, call, observation)
                .await
        })
        .await
    }
    // Private seam for actual-writer fixtures that retain/drop its response.
    // Production supplies only DomainStore above; no host API replaces writes.
    async fn record_with<F, R>(&mut self, record: F) -> Result<UsageStatus, UsageFailure>
    where
        F: FnOnce(UsageSource, String, UsageObservation) -> R,
        R: std::future::Future<Output = Result<hagency_store::UsageReceipt, hagency_store::Error>>,
    {
        let Some(pending) = &self.pending else {
            return Ok(self.status());
        };
        let result = record(
            self.source.clone(),
            pending.call_id.clone(),
            pending.observation.clone(),
        )
        .await;
        match result {
            Ok(_) => {
                self.pending = None;
                self.status.pending = false;
                self.status.acknowledged += 1;
                Ok(self.status())
            }
            Err(_) => {
                self.fence(UsageFailure::Storage);
                Err(UsageFailure::Storage)
            }
        }
    }
}
fn breakdown(evidence: &UsageBreakdown) -> CounterBreakdown {
    CounterBreakdown {
        total_tokens: evidence.total_tokens(),
        input_tokens: evidence.input_tokens(),
        cached_input_tokens: evidence.cached_input_tokens(),
        cache_write_input_tokens: evidence.cache_write_input_tokens(),
        output_tokens: evidence.output_tokens(),
        reasoning_output_tokens: evidence.reasoning_output_tokens(),
    }
}

#[cfg(test)]
#[path = "../tests/support/usage.rs"]
mod tests;
