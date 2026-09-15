//! Private approval delivery pump (PC-C0, plan v4 §3).
//!
//! THE ONE WIRING THAT WORKS (Q1): the driver builds and owns the
//! `Operation` per dispatch, and its only non-borrowed `&mut` window is
//! before `Box::pin(operation.wait_boxed())`. It takes the single-consumer
//! `ApprovalRequests` there — once, by value — and forwards it to this pump,
//! which then owns the receiver for its whole life. There is no second
//! channel and no shared value; the pump `recv()`s on its own task.
//!
//! AUTHORITY (P5 / D-PC-WHO): the pump runs on the SERVICE's multi-threaded
//! runtime — host/service scope — never the driver's current-thread runtime
//! and never inside `execute`. No `RunnerCapability`, no `WorkspaceAccess`
//! and no driver runtime reach this module. The card is re-read from the
//! admitted domain request by `request_id`; the notice carries no Matrix
//! path or content, and none is accepted from any caller.
//!
//! TERMINATION: the worker's `finish_notices` drops the sender at run end,
//! so `recv()` returns `None` and the pump ends by itself. Cancellation is
//! the bootstrap shutdown token; each send runs under a child token. The
//! send outcome is EXPOSED (traced and observable via
//! `private_approval_delivery_status`); the fail-closed denial POLICY is
//! D-PC-FC, a later slice — C0 wires, it does not decide the denial.
use super::Failure;
use hagency_execution::ApprovalRequests;
use hagency_matrix::{ApprovalCollector, CancellationToken, HostApprovalConfig, HostConfig};
use hagency_store::DomainStore;
use std::{sync::Arc, time::Duration};

/// The authority boundary in one place: everything the pump may know.
pub(crate) struct Pump {
    collector: Arc<ApprovalCollector>,
    domain: DomainStore,
}

/// The construction deliverables (Q3), all in one place:
/// `HostApprovalConfig::new` over the approval bot's own `HostConfig`,
/// `with_fresh_account_enrollment(anchors)`, and
/// `ApprovalCollector::new(HostApprovalConfig, DomainStore)`. Nothing here
/// touches the pooled ordinary `Collector` in `Shared`: `Collector::new`
/// refuses a config with `approval == true`, which is exactly what
/// `HostApprovalConfig::new` sets — the two purposes cannot merge.
///
/// Refuses with a named error when the fresh-account enrollment is absent
/// (the `refuses_without_enrollment` selector): a config without
/// `with_fresh_account_enrollment` never yields a collector, and no card can
/// be sent through the pooled ordinary owner.
pub(crate) fn collector(
    config: HostConfig,
    engagement_id: String,
    anchors: Vec<(String, String)>,
    domain: DomainStore,
) -> Result<Arc<ApprovalCollector>, Failure> {
    let approval = HostApprovalConfig::new(config, vec![engagement_id]).map_err(|_| {
        tracing::error!("approval collector refused: host approval config invalid");
        Failure::Config
    })?;
    // The named refusal the `refuses_without_enrollment` selector binds: an
    // approval section whose fresh-account enrollment anchors are absent or
    // invalid never yields a collector, and no card can be sent.
    let approval = approval
        .with_fresh_account_enrollment(anchors)
        .map_err(|_| {
            tracing::error!("approval enrollment refused: fresh-account anchors absent or invalid");
            Failure::Config
        })?;
    Ok(Arc::new(ApprovalCollector::new(approval, domain).map_err(
        |_| {
            tracing::error!("approval collector refused: collector construction failed");
            Failure::Config
        },
    )?))
}

impl Pump {
    pub(crate) fn new(collector: Arc<ApprovalCollector>, domain: DomainStore) -> Self {
        Self { collector, domain }
    }

    /// Drain one run's notices to completion. The `ApprovalRequests` value is
    pub(crate) async fn drain(&self, mut requests: ApprovalRequests, shutdown: &CancellationToken) {
        while let Some(notice) = requests.recv().await {
            // The notice carries only the request id and its expiry; the card
            // is re-read from the admitted domain request, never trusted
            // from the caller or the channel.
            let card = match self
                .domain
                .private_approval_card(notice.request_id.clone(), notice.owner_expires_at)
                .await
            {
                Ok(card) => Arc::new(card),
                Err(error) => {
                    // The admission may have been fenced or replaced between
                    // the recorded write and this read; the outcome is named,
                    // never a silent skip and never a retry.
                    tracing::warn!(
                        "[approval] card read refused for {}: {error}; no send",
                        notice.request_id
                    );
                    continue;
                }
            };
            // One send under a child token: the outcome is exposed, and the
            // denial policy itself is D-PC-FC (a later slice).
            let cancel = shutdown.child_token();
            match self
                .collector
                .send_private_approval_card(card, &cancel)
                .await
            {
                Ok(summary) => {
                    if summary.replayed {
                        tracing::info!(
                            "[approval] card {} replayed (already retained)",
                            notice.request_id
                        );
                    }
                }
                Err(error) => {
                    tracing::warn!(
                        "[approval] card {} send outcome: {error}",
                        notice.request_id
                    );
                }
            }
        }
    }

    /// Close the approval collector the way `bootstrap.rs` closes the
    /// ordinary one: bounded timeout, spawned on the service runtime, the
    /// original owner retained on an unknown outcome (ADR-112).
    pub(crate) async fn close(&self) -> Result<(), Failure> {
        match tokio::time::timeout(Duration::from_secs(2), self.collector.close()).await {
            Ok(Ok(())) => Ok(()),
            _ => Err(Failure::OutcomeUnknown),
        }
    }
}
