//! Private approval delivery and owner-verdict pump (PC-C0, ADR064/112).
//!
//! THE ONE WIRING THAT WORKS (Q1): the driver builds and owns the
//! `Operation` per dispatch, and its only non-borrowed `&mut` window is
//! before `Box::pin(operation.wait_boxed())`. It takes the single-consumer
//! `ApprovalRequests` there — once, by value — and forwards it to this pump,
//! which then owns the receiver for its whole life. Up to16 original receivers
//! share one service task; no replacement notice channel or sender is created.
//!
//! AUTHORITY (P5 / D-PC-WHO): the pump runs on the SERVICE's multi-threaded
//! runtime — host/service scope — never the driver's current-thread runtime
//! and never inside `execute`. No `RunnerCapability`, no `WorkspaceAccess`
//! and no driver runtime reach this module. The card is re-read from the
//! admitted domain request by `request_id`; the notice carries no Matrix
//! path or content, and none is accepted from any caller.
//!
//! TERMINATION: the worker's `finish_notices` drops the sender at run end,
//! so `recv()` returns `None` and only that source is removed. Cancellation is
//! the bootstrap shutdown token; each send runs under a child token. The
//! send outcome is traced and observable via `private_approval_delivery_status`.
//! ADR137 owns fail-closed send policy; this pump never fabricates a verdict.
//! Pending notice IDs feed the same approval SDK's authenticated intake. Any
//! refused or unsettled operation stops polling rather than retrying custody.
use super::Failure;
use hagency_execution::{ApprovalNotice, ApprovalRequests};
use hagency_matrix::{
    ApprovalCollector, CancellationToken, HostApprovalConfig, HostApprovalPlan, HostConfig,
    PrivateApprovalDeliveryState,
};
use hagency_store::DomainStore;
use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::mpsc;

const MAX_SOURCES: usize = 16;
#[derive(Default)]
struct Sources {
    receivers: Vec<mpsc::Receiver<ApprovalNotice>>,
    next: usize,
}
enum SourceEvent {
    Notice(ApprovalNotice),
    Closed,
}
impl Sources {
    fn insert(
        &mut self,
        receiver: mpsc::Receiver<ApprovalNotice>,
    ) -> Result<(), mpsc::Receiver<ApprovalNotice>> {
        if self.receivers.len() == MAX_SOURCES {
            return Err(receiver);
        }
        self.receivers.push(receiver);
        Ok(())
    }
    async fn recv(&mut self) -> SourceEvent {
        // poll_recv is cancellation-safe: a select branch that loses cannot
        // consume a notice. Each original receiver registers this task's waker.
        std::future::poll_fn(|cx| {
            let length = self.receivers.len();
            for offset in 0..length {
                let index = (self.next + offset) % length;
                match self.receivers[index].poll_recv(cx) {
                    std::task::Poll::Ready(Some(notice)) => {
                        self.next = (index + 1) % length;
                        return std::task::Poll::Ready(SourceEvent::Notice(notice));
                    }
                    std::task::Poll::Ready(None) => {
                        self.receivers.remove(index);
                        self.next = if self.receivers.is_empty() {
                            0
                        } else {
                            index % self.receivers.len()
                        };
                        return std::task::Poll::Ready(SourceEvent::Closed);
                    }
                    std::task::Poll::Pending => {}
                }
            }
            std::task::Poll::Pending
        })
        .await
    }
}

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

    /// Explicit configured Matrix SDK enrollment, not provider credential login.
    /// The original Complete path validates existing custody without new keys.
    pub(crate) async fn initialize(&self, shutdown: &CancellationToken) -> Result<(), Failure> {
        let result = async {
            let _turn = self.collector.service_turn(shutdown).await?;
            self.collector.observe(shutdown).await?;
            self.collector.enroll_fresh_account(shutdown).await
        }
        .await;
        result.map_err(|error| {
            tracing::error!("approval startup refused: {error}");
            Failure::Startup
        })
    }

    /// Original run receivers are the only source of request plans. The global
    /// pending set is bounded independently of each original16-slot channel.
    /// No error automatically re-enters an original unknown custody operation.
    pub(crate) async fn drain(
        &self,
        mut handoffs: mpsc::Receiver<ApprovalRequests>,
        shutdown: &CancellationToken,
    ) -> Result<(), Failure> {
        let mut sources = Sources::default();
        let mut accepting = true;
        let mut pending = BTreeMap::new();
        let mut poll = tokio::time::interval(Duration::from_millis(250));
        poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            if !accepting && sources.receivers.is_empty() {
                return Ok(());
            }
            let notice = tokio::select! {
                biased;
                _ = shutdown.cancelled() => return Ok(()),
                _ = poll.tick(), if !pending.is_empty() => {
                    self.poll(&mut pending, shutdown).await?;
                    // Slow SDK IO must not make the next tick immediately
                    // ready forever and starve queued card notices instead.
                    poll.reset();
                    continue;
                },
                source = handoffs.recv(), if accepting && sources.receivers.len()<MAX_SOURCES => {
                    match source {
                        Some(source)=>sources.insert(source.into_receiver()).map_err(|_|Failure::OutcomeUnknown)?,
                        None=>accepting=false,
                    }
                    continue;
                },
                notice = sources.recv(), if !sources.receivers.is_empty() => match notice {
                    SourceEvent::Notice(notice)=>notice,
                    SourceEvent::Closed=>continue,
                },
            };
            if !pending.contains_key(&notice.request_id) && pending.len() == 64 {
                tracing::error!("approval pump capacity exhausted; no new plan");
                return Err(Failure::OutcomeUnknown);
            }
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
            // One send under a child token; ADR137 owns its denial policy.
            let cancel = shutdown.child_token();
            let _turn = self
                .collector
                .service_turn(&cancel)
                .await
                .map_err(|_| Failure::OutcomeUnknown)?;
            match self
                .collector
                .send_private_approval_card(card, &cancel)
                .await
            {
                Ok(summary) if summary.state == PrivateApprovalDeliveryState::Accepted => {
                    if summary.replayed {
                        tracing::info!(
                            "[approval] card {} replayed (already retained)",
                            notice.request_id
                        );
                    }
                    pending.insert(notice.request_id, notice.owner_expires_at);
                }
                Ok(_) => {
                    tracing::warn!("approval card not durably accepted; pump stopped");
                    return Err(Failure::OutcomeUnknown);
                }
                Err(error) => {
                    tracing::warn!(
                        "[approval] card {} send outcome: {error}",
                        notice.request_id
                    );
                    return Err(Failure::OutcomeUnknown);
                }
            }
        }
    }

    async fn poll(
        &self,
        pending: &mut BTreeMap<String, u64>,
        shutdown: &CancellationToken,
    ) -> Result<(), Failure> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Failure::OutcomeUnknown)?
            .as_millis();
        let mut current = Vec::new();
        for (id, cutoff) in pending.iter() {
            // This cutoff only removes polling work. The original collector and
            // writer still enforce their own fresh authority and expiry checks.
            if u128::from(*cutoff) <= now {
                continue;
            }
            let summary = self
                .domain
                .approval_summary(id.clone())
                .await
                .map_err(|_| Failure::OutcomeUnknown)?;
            if summary.state == "pending" {
                current.push(id.clone());
            }
        }
        pending.retain(|id, _| current.contains(id));
        if current.is_empty() {
            return Ok(());
        }
        let plan = HostApprovalPlan::new(current).map_err(|_| Failure::OutcomeUnknown)?;
        let _turn = self
            .collector
            .service_turn(shutdown)
            .await
            .map_err(|_| Failure::OutcomeUnknown)?;
        match self.collector.intake(plan, shutdown).await {
            Ok(summary) if summary.pending == 0 => Ok(()),
            Ok(_) => {
                tracing::warn!("approval intake retained pending custody; pump stopped");
                Err(Failure::OutcomeUnknown)
            }
            Err(error) => {
                tracing::warn!("approval intake refused: {error}; pump stopped");
                Err(Failure::OutcomeUnknown)
            }
        }
    }

    /// Close the approval collector the way `bootstrap.rs` closes the
    /// ordinary one: bounded timeout, spawned on the service runtime, the
    /// original owner retained on an unknown outcome (ADR-112).
    pub(crate) async fn close(&self) -> Result<(), Failure> {
        let close = async {
            let cancel = CancellationToken::new();
            let _turn = self.collector.service_turn(&cancel).await?;
            self.collector.close().await
        };
        match tokio::time::timeout(Duration::from_secs(2), close).await {
            Ok(Ok(())) => Ok(()),
            _ => Err(Failure::OutcomeUnknown),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::Future;

    fn notice(id: &str) -> ApprovalNotice {
        ApprovalNotice {
            request_id: id.into(),
            owner_expires_at: 1,
        }
    }
    async fn next(sources: &mut Sources) -> String {
        match tokio::time::timeout(Duration::from_secs(1), sources.recv())
            .await
            .unwrap()
        {
            SourceEvent::Notice(notice) => notice.request_id,
            SourceEvent::Closed => panic!("expected notice"),
        }
    }
    // Pure channel scheduling fixtures, not positive approval/crypto proofs.
    // Actual native encrypted-owner callback coverage stays in bootstrap tests.
    #[tokio::test]
    async fn native_fleet_approval_notice_multiplex() {
        let (a, first) = mpsc::channel(16);
        let (b, second) = mpsc::channel(16);
        let mut sources = Sources::default();
        sources.insert(first).unwrap();
        sources.insert(second).unwrap();
        b.try_send(notice("second while first is idle")).unwrap();
        assert_eq!(next(&mut sources).await, "second while first is idle");
        assert_eq!(sources.receivers.len(), 2);
        for id in ["a1", "a2"] {
            a.try_send(notice(id)).unwrap();
        }
        for id in ["b1", "b2"] {
            b.try_send(notice(id)).unwrap();
        }
        for id in ["a1", "b1", "a2", "b2"] {
            assert_eq!(next(&mut sources).await, id);
        }
        drop(b);
        assert!(matches!(sources.recv().await, SourceEvent::Closed));
        assert_eq!(sources.receivers.len(), 1);
        a.try_send(notice("original first survives")).unwrap();
        assert_eq!(next(&mut sources).await, "original first survives");
        drop(a);
        assert!(matches!(sources.recv().await, SourceEvent::Closed));
        assert!(sources.receivers.is_empty());
    }

    #[tokio::test]
    async fn native_fleet_approval_notice_capacity() {
        let mut sources = Sources::default();
        let mut senders = Vec::new();
        for _ in 0..MAX_SOURCES {
            let (sender, receiver) = mpsc::channel(16);
            sources.insert(receiver).unwrap();
            senders.push(sender);
        }
        let (extra, receiver) = mpsc::channel(16);
        let rejected = sources.insert(receiver).unwrap_err();
        assert_eq!(sources.receivers.len(), MAX_SOURCES);
        let mut wait = Box::pin(sources.recv());
        assert!(
            std::future::poll_fn(|cx| std::task::Poll::Ready(wait.as_mut().poll(cx).is_pending()))
                .await
        );
        senders[5]
            .try_send(notice("not consumed by cancelled wait"))
            .unwrap();
        drop(wait);
        assert_eq!(next(&mut sources).await, "not consumed by cancelled wait");
        drop(senders.remove(0));
        assert!(matches!(sources.recv().await, SourceEvent::Closed));
        assert_eq!(sources.receivers.len(), MAX_SOURCES - 1);
        extra
            .try_send(notice("original rejected receiver"))
            .unwrap();
        sources.insert(rejected).unwrap();
        assert_eq!(next(&mut sources).await, "original rejected receiver");
    }
}
