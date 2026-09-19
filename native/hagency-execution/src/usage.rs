//! Private attachment to the original fresh owned execution session.
//! Restoring a ledger source is deliberately not an attachment constructor.
use hagency_core::tasks::RunnerCapability;
use hagency_metering::{observation::UsageObservation, runtime_usage::CounterBreakdown};
use hagency_runtime::codex::session::UsageBreakdown;
use hagency_store::{DomainStore, OwnedDispatchScope, UsageSource};
mod capture;
use capture::{CapturedEvent, Evidence, Kind, OwnedCapture, Source};

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
    stream: Option<Source>,
    sequence: u64,
    pending: Option<Pending>,
    rejected: Option<Evidence>,
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
    pub(super) fn attach(&mut self, runner: &impl OwnedCapture) {
        // Only the original owned adapter can supply this source: after fresh
        // Codex start_turn or Claude system/init, before later stream messages.
        match runner.capture_source() {
            Some(source) => self.attach_source(source),
            _ => self.fence(UsageFailure::Source),
        }
    }
    fn attach_source(&mut self, source: impl Into<Source>) {
        let source = source.into();
        if self.stream.is_some()
            || self.status.closed
            || source.is_retired()
            || self._started.resource().framework != source.family()
        {
            self.fence(UsageFailure::Source);
        } else {
            self.sequence = source.baseline();
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
    pub(super) fn observe(&mut self, event: &impl CapturedEvent) -> bool {
        if self.status.closed {
            return false;
        }
        if self.pending.is_some() {
            self.fence(UsageFailure::Storage);
            return false;
        }
        let source = event.source();
        if self.stream.as_ref() != Some(&source) {
            self.fence(UsageFailure::Source);
            return false;
        }
        if source.is_retired() {
            self.fence(UsageFailure::Retired);
            return false;
        }
        if self.sequence.checked_add(1) != Some(event.sequence()) {
            self.fence(UsageFailure::Sequence);
            return false;
        }
        self.sequence = event.sequence();
        match event.kind() {
            Kind::Invalidated => self.fence(UsageFailure::Invalidated),
            Kind::End => self.close(),
            Kind::Usage { evidence, terminal } => {
                self.status.observed += 1; // Both runtime event budgets are finite.
                let evidence = evidence.retain();
                let observation = evidence.normalize();
                match observation {
                    Ok(observation) => {
                        self.pending = Some(Pending {
                            call_id: format!("runtime_v1_{}", event.sequence()),
                            observation,
                        });
                        self.status.pending = true;
                        if terminal {
                            self.close();
                        }
                        return true;
                    }
                    Err(_) => {
                        // Preserve the fixed original projection too; an
                        // arithmetic refusal is not a normalized zero snapshot.
                        self.rejected = Some(evidence);
                        self.status.rejected = true;
                        self.fence(UsageFailure::Normalization);
                    }
                }
            }
            Kind::Ignored => {}
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
