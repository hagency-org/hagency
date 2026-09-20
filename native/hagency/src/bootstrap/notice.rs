//! Delivery of this agent's own delegated-task notices, then selection of the
//! delegated sessions a delivered notice activated. One bounded pass per
//! attempt, never a recurring dispatcher and never another agent's notice.
use super::{Failure, StatusHandle};
use hagency_core::agent_inbox::AgentInboxPlan;
use hagency_matrix::{CancellationToken, Collector, OutgoingState};
use hagency_store::DomainStore;

/// The rest wait for the next poll; one attempt must not become a send loop.
const MAX_NOTICES: usize = 4;

/// Post the task notices addressed to this engagement. The assignee announces
/// its own delegated task, and Matrix accepting that notice is what activates
/// the intent. Custody is `send_final`'s: anything other than Delivered is
/// unknown and ends the attempt, there is never a second send of one claim, and
/// the next attempt's `resume_outgoing_custody` settles whatever was journaled.
pub(super) async fn deliver(
    domain: &DomainStore,
    collector: &Collector,
    engagement: &str,
    cancel: &CancellationToken,
    status: &StatusHandle,
) -> Result<(), Failure> {
    for _ in 0..MAX_NOTICES {
        if cancel.is_cancelled() {
            return Err(Failure::Cancelled);
        }
        let Some(claim) = domain
            .claim_verified_task_notice_for(engagement.into(), 60_000)
            .await
            .map_err(|_| Failure::OutcomeUnknown)?
        else {
            break;
        };
        status.phase("delivering_notice");
        let sent = collector
            .send_notice(claim, cancel)
            .await
            .map_err(|error| {
                status.matrix_refusal(&error);
                tracing::warn!(error = ?error, "delegated task notice refused");
                if cancel.is_cancelled() {
                    Failure::Cancelled
                } else {
                    Failure::OutcomeUnknown
                }
            })?;
        if sent.state != OutgoingState::Delivered {
            return Err(Failure::OutcomeUnknown);
        }
    }
    Ok(())
}

/// Mint the dispatch each active delegated intent of this engagement is waiting
/// for. The listing is a projection; `select_intent_inbox` re-checks every
/// clause in its own writer transaction, so a listed session may be NoWake.
pub(super) async fn schedule(
    domain: &DomainStore,
    engagement: &str,
    workspace_id: &str,
) -> Result<(), Failure> {
    let sessions = domain
        .intent_inboxes(engagement.into())
        .await
        .map_err(|_| Failure::OutcomeUnknown)?;
    for session_id in sessions {
        match domain
            .select_intent_inbox(AgentInboxPlan {
                session_id: session_id.clone(),
                workspace_id: workspace_id.into(),
            })
            .await
        {
            Ok(_) => {}
            // Another agent's intake can advance a shared project's generation
            // between the listing and this selection and retire the route
            // (ADR153). That plan was superseded, not refused, exactly as for
            // the agent inbox (ADR178): the listing names only sessions with a
            // current route, so a session it no longer names is retired work,
            // which is never replayed. One it still names stays a failure.
            Err(hagency_store::Error::RunnerAuthority)
                if !domain
                    .intent_inboxes(engagement.into())
                    .await
                    .map_err(|_| Failure::OutcomeUnknown)?
                    .contains(&session_id) =>
            {
                tracing::info!(session_id = %session_id, "delegated session superseded by a newer room generation");
            }
            Err(_) => return Err(Failure::OutcomeUnknown),
        }
    }
    Ok(())
}
