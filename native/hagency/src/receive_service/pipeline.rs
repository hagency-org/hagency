use super::{
    ReceiveError, ReceiveView, Registry,
    job::{Disposition, Job},
    types,
};
use crate::bootstrap::Shared;
use hagency_core::received_files::{ReceiveFailure, ReceivedFileFacts, ReceivedFileState};
use std::sync::{Arc, atomic::Ordering};
use tokio::{sync::Semaphore, time::Instant};

pub(super) fn checkpoint(registry: &Registry, deadline: Instant) -> Result<(), ReceiveError> {
    if registry.cancel.is_cancelled() || registry.closed.load(Ordering::Acquire) {
        return Err(ReceiveError::Unauthorized);
    }
    if Instant::now() >= deadline {
        return Err(ReceiveError::Unavailable);
    }
    Ok(())
}
pub(super) async fn run(
    shared: Shared,
    registry: Arc<Registry>,
    gate: Arc<Semaphore>,
    job: Arc<Job>,
    limit: usize,
) {
    let result = transfer(&shared, &registry, &gate, &job, limit).await;
    if let Ok(view) = result {
        job.finish(Ok(view), Disposition::Ready);
        return;
    }
    let error = result.expect_err("checked failure branch");
    let mut original = job.original.lock().await;
    if original.historical_only {
        // No new operation or file owner was acquired. Preserve the permanent
        // record, but an inspect-only refusal need not occupy a live write slot.
        *original = Default::default();
        job.finish(Err(ReceiveError::Unknown), Disposition::Release);
        return;
    }
    // Uncertain reservation/start replies and all admitted writes retain the
    // actual checked result, root and any prepared/partial destination.
    if original.write_possible || error == ReceiveError::Unknown {
        if let Some(admission) = &original.admission {
            let _ = shared
                .domain
                .record_received_file_negative(
                    job.cap.clone(),
                    admission.identity.clone(),
                    ReceiveFailure::OutcomeUnknown,
                )
                .await;
        }
        job.unknown();
        return;
    }
    if let Some(admission) = &original.admission {
        let reason = if registry.cancel.is_cancelled() {
            ReceiveFailure::Cancelled
        } else {
            ReceiveFailure::SourceRefused
        };
        match shared
            .domain
            .record_received_file_negative(job.cap.clone(), admission.identity.clone(), reason)
            .await
        {
            Ok(observation) if observation.state == ReceivedFileState::Failed => {}
            _ => {
                job.unknown();
                return;
            }
        }
    }
    *original = Default::default();
    job.finish(Err(error), Disposition::Release);
}
async fn transfer(
    shared: &Shared,
    registry: &Registry,
    gate: &Semaphore,
    job: &Job,
    limit: usize,
) -> Result<ReceiveView, ReceiveError> {
    checkpoint(registry, job.deadline)?;
    let workspace = shared
        .workspace
        .acquire(&job.cap)
        .await
        .map_err(|_| ReceiveError::Unauthorized)?;
    let mut original = job.original.lock().await;
    original.workspace = Some(workspace);
    checkpoint(registry, job.deadline)?;
    // This command persists quota and complete original request association
    // before the first authenticated media GET.
    original.admission = Some(
        shared
            .domain
            .reserve_received_file(job.cap.clone(), job.input.event_id.clone(), limit)
            .await
            .map_err(ReceiveError::from)?,
    );
    if original
        .admission
        .as_ref()
        .ok_or(ReceiveError::Unknown)?
        .reservation
        .is_none()
    {
        // Historical Ready/Reserved cannot recreate lost same-process custody.
        original.historical_only = true;
        return Err(ReceiveError::Unknown);
    }
    checkpoint(registry, job.deadline)?;
    original.checked = Some(
        shared
            .collector
            .receive_attachment_until(
                job.cap.clone(),
                job.input.event_id.clone(),
                &registry.cancel,
                job.deadline,
                limit,
            )
            .await
            .map_err(|_| ReceiveError::Unavailable)?,
    );
    let checked = original.checked.as_ref().ok_or(ReceiveError::Unknown)?;
    let reservation = original
        .admission
        .as_ref()
        .and_then(|a| a.reservation.as_ref())
        .ok_or(ReceiveError::Unknown)?;
    if !reservation.matches_ticket(checked.ticket()) {
        return Err(ReceiveError::Conflict);
    }
    checked
        .revalidate()
        .await
        .map_err(|_| ReceiveError::Unauthorized)?;
    original
        .workspace
        .as_ref()
        .ok_or(ReceiveError::Unknown)?
        .validate_current()
        .await
        .map_err(|_| ReceiveError::Unauthorized)?;
    // Both live jobs may download, but this one fixed worker performs only one
    // materialize/readback at once. Waiting cannot renew the original deadline.
    let _sink = tokio::select! {
        biased;
        _=registry.cancel.cancelled()=>return Err(ReceiveError::Unauthorized),
        _=tokio::time::sleep_until(job.deadline)=>return Err(ReceiveError::Unavailable),
        permit=gate.acquire()=>permit.map_err(|_|ReceiveError::Unknown)?,
    };
    checkpoint(registry, job.deadline)?;
    let checked = original.checked.as_ref().ok_or(ReceiveError::Unknown)?;
    checked
        .revalidate()
        .await
        .map_err(|_| ReceiveError::Unauthorized)?;
    original
        .workspace
        .as_ref()
        .ok_or(ReceiveError::Unknown)?
        .validate_current()
        .await
        .map_err(|_| ReceiveError::Unauthorized)?;
    let facts = ReceivedFileFacts {
        size: checked.bytes().len() as u64,
        sha256: types::hex(checked.digest()),
    };
    let reservation = original
        .admission
        .as_ref()
        .and_then(|a| a.reservation.clone())
        .ok_or(ReceiveError::Unknown)?;
    // Set before awaiting: a lost transaction reply cannot be treated as
    // permission to download or request a second unique write grant.
    original.write_possible = true;
    let write = shared
        .domain
        .start_received_file_write(job.cap.clone(), reservation, facts.clone())
        .await
        .map_err(ReceiveError::from)?;
    if !write.matches_ticket(
        original
            .checked
            .as_ref()
            .ok_or(ReceiveError::Unknown)?
            .ticket(),
    ) {
        return Err(ReceiveError::Unknown);
    }
    original.sink = Some(
        original
            .workspace
            .as_ref()
            .ok_or(ReceiveError::Unknown)?
            .prepare_receive(write, job.deadline.into_std())
            .map_err(|_| ReceiveError::Unknown)?,
    );
    checkpoint(registry, job.deadline)?;
    original
        .checked
        .as_ref()
        .ok_or(ReceiveError::Unknown)?
        .revalidate()
        .await
        .map_err(|_| ReceiveError::Unauthorized)?;
    // The owner is already in Job before the first materialize poll. Borrow it;
    // neither panic nor a dropped requester can destroy its partial file.
    let original = &mut *original;
    original
        .sink
        .as_mut()
        .ok_or(ReceiveError::Unknown)?
        .materialize(
            original
                .checked
                .as_ref()
                .ok_or(ReceiveError::Unknown)?
                .bytes(),
        )
        .await
        .map_err(|_| ReceiveError::Unknown)?;
    checkpoint(registry, job.deadline)?;
    original
        .checked
        .as_ref()
        .ok_or(ReceiveError::Unknown)?
        .revalidate()
        .await
        .map_err(|_| ReceiveError::Unauthorized)?;
    original
        .workspace
        .as_ref()
        .ok_or(ReceiveError::Unknown)?
        .validate_current()
        .await
        .map_err(|_| ReceiveError::Unauthorized)?;
    let admission = original.admission.as_ref().ok_or(ReceiveError::Unknown)?;
    let ready = shared
        .domain
        .record_received_file_ready(job.cap.clone(), admission.identity.clone(), facts.clone())
        .await;
    let ready = match ready {
        Ok(value) => value,
        Err(_) => shared
            .domain
            .inspect_received_file(job.cap.clone(), admission.identity.id().to_owned())
            .await
            .map_err(|_| ReceiveError::Unknown)?,
    };
    if ready.state != ReceivedFileState::Ready
        || ready.id != admission.identity.id()
        || ready.event_id != job.input.event_id
        || ready.facts.as_ref() != Some(&facts)
        || ready.metadata != admission.observation.metadata
    {
        return Err(ReceiveError::Unknown);
    }
    let view = ReceiveView {
        event_id: job.input.event_id.clone(),
        filename: ready.metadata.filename,
        mime_type: ready.metadata.mime_type,
        size: facts.size,
        sha256: facts.sha256,
        path: original
            .sink
            .as_ref()
            .ok_or(ReceiveError::Unknown)?
            .relative_path()
            .to_owned(),
        replayed: false,
    };
    view.validate()?;
    original.scope = Some(
        original
            .checked
            .take()
            .ok_or(ReceiveError::Unknown)?
            .into_scope(),
    );
    Ok(view)
}
