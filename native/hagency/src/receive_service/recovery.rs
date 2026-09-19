//! Read-only original Ready inspection; no path or fact can recreate ownership.
use super::{ReceiveError, ReceiveView, Registry, job::Job, pipeline::checkpoint};
use crate::bootstrap::Shared;
use hagency_core::received_files::ReceivedFileState;
use tokio::time::Instant;

pub(super) async fn lost_task(shared: &Shared, job: &Job) {
    job.unknown();
    let original = job.original.lock().await;
    if let Some(admission) = &original.admission {
        let _ = shared
            .domain
            .record_received_file_negative(
                job.cap.clone(),
                admission.identity.clone(),
                hagency_core::received_files::ReceiveFailure::OutcomeUnknown,
            )
            .await;
    }
    // Failure to acknowledge the observation is still unknown. Neither branch
    // releases this exact original job or its destination.
    job.acknowledge();
}

pub(super) async fn read(
    shared: &Shared,
    registry: &Registry,
    job: &Job,
    deadline: Instant,
    replayed: bool,
) -> Result<ReceiveView, ReceiveError> {
    checkpoint(registry, deadline)?;
    let mut original = job.original.lock().await;
    let original = &mut *original;
    let admission = original.admission.as_ref().ok_or(ReceiveError::Unknown)?;
    let scope = original.scope.as_ref().ok_or(ReceiveError::Unknown)?;
    let sink = original.sink.as_mut().ok_or(ReceiveError::Unknown)?;
    if !admission
        .reservation
        .as_ref()
        .is_some_and(|r| r.matches_ticket(scope.ticket()))
        || sink.identity().id() != admission.identity.id()
    {
        return Err(ReceiveError::Unknown);
    }
    scope
        .revalidate(&registry.cancel, deadline)
        .await
        .map_err(|_| ReceiveError::Unauthorized)?;
    original
        .workspace
        .as_ref()
        .ok_or(ReceiveError::Unknown)?
        .validate_current()
        .await
        .map_err(|_| ReceiveError::Unauthorized)?;
    checkpoint(registry, deadline)?;
    let observation = shared
        .domain
        .inspect_received_file(job.cap.clone(), admission.identity.id().to_owned())
        .await
        .map_err(ReceiveError::from)?;
    if observation.state != ReceivedFileState::Ready
        || observation.id != admission.identity.id()
        || observation.event_id != job.input.event_id
        || observation.metadata != admission.observation.metadata
        || observation.facts.as_ref() != Some(sink.facts())
    {
        return Err(ReceiveError::Unknown);
    }
    sink.revalidate(deadline.into_std())
        .await
        .map_err(|_| ReceiveError::Unauthorized)?;
    scope
        .revalidate(&registry.cancel, deadline)
        .await
        .map_err(|_| ReceiveError::Unauthorized)?;
    original
        .workspace
        .as_ref()
        .ok_or(ReceiveError::Unknown)?
        .validate_current()
        .await
        .map_err(|_| ReceiveError::Unauthorized)?;
    checkpoint(registry, deadline)?;
    let view = ReceiveView {
        event_id: job.input.event_id.clone(),
        filename: observation.metadata.filename,
        mime_type: observation.metadata.mime_type,
        size: sink.facts().size,
        sha256: sink.facts().sha256.clone(),
        path: sink.relative_path().to_owned(),
        replayed,
    };
    view.validate()?;
    Ok(view)
}
