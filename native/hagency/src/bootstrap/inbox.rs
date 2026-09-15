//! One intake and original canonical selection, never a recurring dispatcher.
use super::{Failure, StatusHandle};
use hagency_core::received_files::{ReceiveInboxPlan, ReceiveInboxSelection};
use hagency_matrix::{CancellationToken, Collector, HostIntakePlan};
use hagency_store::DomainStore;

pub(super) async fn prepare(
    domain: &DomainStore,
    collector: &Collector,
    plan: ReceiveInboxPlan,
    cancel: &CancellationToken,
    status: &StatusHandle,
) -> Result<bool, Failure> {
    if cancel.is_cancelled() {
        return Err(Failure::Cancelled);
    }
    status.phase("receiving_input");
    let intake = HostIntakePlan::new(vec![plan.session_id.clone()]).map_err(|_| Failure::Config)?;
    collector.intake(intake, cancel).await.map_err(|error| {
        if error == hagency_matrix::Error::OutcomeUnknown {
            Failure::OutcomeUnknown
        } else {
            Failure::Refresh
        }
    })?;
    if cancel.is_cancelled() {
        return Err(Failure::Cancelled);
    }
    status.phase("selecting_input");
    let result = domain
        .select_receive_inbox(plan)
        .await
        .map_err(|_| Failure::OutcomeUnknown)?;
    if cancel.is_cancelled() {
        return Err(Failure::Cancelled);
    }
    Ok(matches!(result, ReceiveInboxSelection::Selected { .. }))
}
