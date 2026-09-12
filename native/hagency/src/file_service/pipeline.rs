use super::{FileError, FileStatus, FileView, Registry, job::Job, types};
use crate::bootstrap::Shared;
use hagency_core::{file_delivery::*, uploads::*};
use hagency_files::RelativeFile;
use hagency_matrix::StagedUpload;
use hagency_media::Codec;
use hagency_media_store::{OperationId, Recovery, Store, SyncEvidence};
use std::{
    cell::RefCell,
    rc::Rc,
    sync::{Arc, atomic::Ordering},
};
use tokio::sync::{Semaphore, oneshot};

pub(super) struct Context {
    pub shared: Shared,
    pub registry: Arc<Registry>,
    pub media: Rc<RefCell<Option<Store>>>,
    pub codec: Codec,
    pub gate: Rc<Semaphore>,
    pub limit: usize,
}
pub(super) async fn job(
    context: Context,
    job: Arc<Job>,
    reply: oneshot::Sender<Result<FileView, FileError>>,
) {
    let Context {
        shared,
        registry,
        media,
        codec,
        gate,
        limit,
    } = context;
    // Read-only exact replay is allowed to acknowledge old durable facts. It
    // never restores the lost capture grant or reaches the source pipeline.
    match shared
        .domain
        .restore_file_delivery(job.cap.clone(), job.request.clone())
        .await
    {
        Ok(Some(id)) => {
            let value = shared
                .domain
                .inspect_file_delivery(job.cap.clone(), id.id().to_owned())
                .await
                .map(|r| FileView::from_receipt(r, false))
                .map_err(FileError::from);
            job.publish(value.clone(), false);
            let _ = reply.send(value);
            job.release_when_joined();
            return;
        }
        Ok(None) => {}
        Err(error) => {
            let error = FileError::from(error);
            job.publish(Err(error), false);
            let _ = reply.send(Err(error));
            if error != FileError::Unknown {
                job.release_when_joined();
            }
            return;
        }
    }
    if registry.cancel.is_cancelled() || shared.workspace.acquire(&job.cap).await.is_err() {
        job.publish(Err(FileError::Unauthorized), false);
        let _ = reply.send(Err(FileError::Unauthorized));
        job.release_when_joined();
        return;
    }
    let mut admission = match shared
        .domain
        .reserve_file_delivery(job.cap.clone(), job.request.clone())
        .await
    {
        Ok(value) => value,
        Err(error) => {
            let error = FileError::from(error);
            let mut view = Err(error);
            if error == FileError::Unknown
                && let Ok(Some(id)) = shared
                    .domain
                    .restore_file_delivery(job.cap.clone(), job.request.clone())
                    .await
                && let Ok(receipt) = shared
                    .domain
                    .inspect_file_delivery(job.cap.clone(), id.id().to_owned())
                    .await
            {
                view = Ok(FileView::from_receipt(receipt, false));
            }
            job.publish(view.clone(), false);
            let _ = reply.send(view);
            if error != FileError::Unknown {
                job.release_when_joined();
            }
            return;
        }
    };
    let fresh = admission.upload.preparation.is_some();
    let view = FileView::from_receipt(admission.receipt.clone(), fresh);
    {
        let mut original = job.original.lock().await;
        original.preparation = admission.upload.preparation.take().map(Arc::new);
        original.admission = Some(admission);
    }
    job.publish(Ok(view.clone()), fresh);
    let _ = reply.send(Ok(view));
    if !fresh {
        job.release_when_joined();
        return;
    }
    let _gate = match gate.acquire().await {
        Ok(value) => value,
        Err(_) => {
            job.publish(Err(FileError::Unknown), false);
            return;
        }
    };
    let mut reason = FileDeliveryFailure::SourceRefused;
    let result = transfer(&shared, &registry, &media, &codec, limit, &job, &mut reason).await;
    if result.is_ok() {
        let id = job.info.lock().ok().and_then(|i| i.id.clone());
        if let Some(id) = id
            && let Ok(receipt) = shared
                .domain
                .inspect_file_delivery(job.cap.clone(), id)
                .await
        {
            let view = FileView::from_receipt(receipt, false);
            if view.status == FileStatus::Delivered {
                job.publish(Ok(view), false);
                release(&job).await;
                return;
            }
        }
    }
    if registry.cancel.is_cancelled() {
        reason = FileDeliveryFailure::Cancelled;
    }
    let id = job
        .original
        .lock()
        .await
        .admission
        .as_ref()
        .map(|a| a.identity.clone());
    if let Some(id) = id
        && let Ok(receipt) = shared.domain.cancel_file_delivery(id, reason).await
    {
        let view = FileView::from_receipt(receipt, false);
        let safe = view.status == FileStatus::Failed
            && media
                .borrow()
                .as_ref()
                .is_some_and(|m| m.recovery() == Recovery::Clean);
        job.publish(Ok(view), false);
        if safe {
            release(&job).await;
        }
        return;
    }
    job.publish(Err(FileError::Unknown), false);
}
async fn release(job: &Arc<Job>) {
    // Current job future is the sole driver; no original object is still borrowed
    // by another send. Completed/known-safe domain status remains replayable.
    *job.original.lock().await = Default::default();
    job.release_when_joined();
}
fn checkpoint(registry: &Registry) -> Result<(), FileError> {
    if registry.cancel.is_cancelled() {
        Err(FileError::Unauthorized)
    } else if !registry.ready.load(Ordering::Acquire) {
        Err(FileError::Unavailable)
    } else {
        Ok(())
    }
}
async fn transfer(
    shared: &Shared,
    registry: &Registry,
    media: &Rc<RefCell<Option<Store>>>,
    codec: &Codec,
    limit: usize,
    job: &Job,
    reason: &mut FileDeliveryFailure,
) -> Result<(), FileError> {
    #[cfg(test)]
    registry.tests.block_source();
    checkpoint(registry)?;
    let access = shared
        .workspace
        .acquire(&job.cap)
        .await
        .map_err(|_| FileError::Unauthorized)?;
    checkpoint(registry)?;
    let selection = RelativeFile::new(&job.input.path).map_err(|_| FileError::Invalid)?;
    let mut original = job.original.lock().await;
    original.snapshot = Some(
        access
            .snapshot(&selection, limit)
            .map_err(|_| FileError::Unavailable)?,
    );
    let captured = {
        let value = original.snapshot.as_ref().ok_or(FileError::Unknown)?;
        CapturedFile {
            size: value.len() as u64,
            sha256: types::hex(value.digest()),
        }
    };
    access
        .validate_current()
        .await
        .map_err(|_| FileError::Unauthorized)?;
    checkpoint(registry)?;
    original.encrypted = Some(
        codec
            .encrypt(original.snapshot.take().ok_or(FileError::Unknown)?)
            .map_err(|_| FileError::Unavailable)?,
    );
    *reason = FileDeliveryFailure::StagingRefused;
    let (identity, upload) = original
        .admission
        .as_ref()
        .map(|a| (a.identity.clone(), a.upload.identity.clone()))
        .ok_or(FileError::Unknown)?;
    let operation = OperationId::new(upload.id()).map_err(|_| FileError::Unknown)?;
    let prepare = {
        media
            .borrow_mut()
            .as_mut()
            .ok_or(FileError::Unknown)?
            .prepare_encrypted(
                &operation,
                original.encrypted.take().ok_or(FileError::Unknown)?,
            )
    };
    match prepare {
        Ok(value) => original.prepared = Some(value),
        Err(failure) => {
            let error = failure.error();
            original.returned = failure.into_unadmitted();
            if error == hagency_media_store::Error::OutcomeUnknown {
                registry.ready.store(false, Ordering::Release);
            }
            return Err(FileError::Unknown);
        }
    }
    let prepared = original.prepared.as_ref().ok_or(FileError::Unknown)?;
    let digest = *prepared.digest();
    let stage = StageCommitment {
        namespace_digest: types::hex(prepared.namespace().digest()),
        operation_id: prepared.operation().as_str().to_owned(),
        receipt_digest: types::hex(&digest),
        len: prepared.len() as u64,
    };
    shared
        .domain
        .bind_file_delivery_stage(
            job.cap.clone(),
            identity,
            original.preparation.clone().ok_or(FileError::Unknown)?,
            captured,
            stage.clone(),
        )
        .await
        .map_err(FileError::from)?;
    checkpoint(registry)?;
    let staged = {
        media
            .borrow_mut()
            .as_mut()
            .ok_or(FileError::Unknown)?
            .stage_prepared(original.prepared.take().ok_or(FileError::Unknown)?)
    };
    let observation = match staged {
        Ok(receipt) if receipt.sync_evidence() == SyncEvidence::FileAndDirectorySynced => {
            UploadStageObservation::FileAndDirectorySynced
        }
        Ok(_) => UploadStageObservation::OutcomeUnknown,
        Err(failure) => {
            original.returned = failure.into_unadmitted();
            registry.ready.store(false, Ordering::Release);
            UploadStageObservation::OutcomeUnknown
        }
    };
    shared
        .domain
        .observe_upload_staged(upload.clone(), stage, observation)
        .await
        .map_err(FileError::from)?;
    if !matches!(observation, UploadStageObservation::FileAndDirectorySynced) {
        return Err(FileError::Unknown);
    }
    checkpoint(registry)?;
    original.restored = Some(
        media
            .borrow_mut()
            .as_mut()
            .ok_or(FileError::Unknown)?
            .restore_encrypted(&operation, &digest)
            .map_err(|_| FileError::Unknown)?,
    );
    access
        .validate_current()
        .await
        .map_err(|_| FileError::Unauthorized)?;
    checkpoint(registry)?;
    *reason = FileDeliveryFailure::PublicationRefused;
    let claim = shared
        .domain
        .claim_upload(job.cap.clone(), upload, 30_000)
        .await
        .map_err(FileError::from)?
        .ok_or(FileError::Unknown)?;
    let send = shared
        .domain
        .begin_upload(job.cap.clone(), claim.clone())
        .await
        .map_err(FileError::from)?;
    let input = match StagedUpload::new(
        job.cap.clone(),
        claim,
        send,
        original.restored.take().ok_or(FileError::Unknown)?,
    ) {
        Ok(value) => value,
        Err(value) => {
            original.rejected_upload = Some(value);
            return Err(FileError::Unknown);
        }
    };
    match shared.collector.stage_upload(input) {
        Ok(value) => original.upload = Some(value),
        Err(value) => {
            original.rejected_upload = Some(value);
            return Err(FileError::Unknown);
        }
    }
    if original
        .upload
        .as_mut()
        .ok_or(FileError::Unknown)?
        .run(&registry.cancel)
        .await
        .is_err()
    {
        // One exact historical acceptance inspection; never another POST or Send.
        let id = original
            .upload
            .as_ref()
            .ok_or(FileError::Unknown)?
            .id()
            .to_owned();
        let _ = shared
            .collector
            .settle_upload(&id, &hagency_matrix::CancellationToken::new())
            .await;
        return Err(FileError::Unknown);
    }
    access
        .validate_current()
        .await
        .map_err(|_| FileError::Unauthorized)?;
    checkpoint(registry)?;
    let identity = original
        .admission
        .as_ref()
        .ok_or(FileError::Unknown)?
        .identity
        .clone();
    let claim = shared
        .domain
        .claim_file_publication(job.cap.clone(), identity, 30_000)
        .await
        .map_err(FileError::from)?
        .ok_or(FileError::Unknown)?;
    let send = shared
        .domain
        .begin_file_publication(job.cap.clone(), claim.clone())
        .await
        .map_err(FileError::from)?;
    match original
        .upload
        .take()
        .ok_or(FileError::Unknown)?
        .prepare_file_publication(claim, send)
    {
        Ok(value) => original.publication = Some(value),
        Err(value) => {
            original.rejected_publication = Some(value);
            return Err(FileError::Unknown);
        }
    }
    if original
        .publication
        .as_mut()
        .ok_or(FileError::Unknown)?
        .run(&registry.cancel)
        .await
        .is_err()
    {
        // Reconcile only the original private receipt. Possible stays nonrearmable.
        let resumed = shared
            .collector
            .resume_outgoing_custody(&hagency_matrix::CancellationToken::new())
            .await;
        let expected = original
            .admission
            .as_ref()
            .ok_or(FileError::Unknown)?
            .identity
            .id();
        if let Ok(summary) = resumed
            && summary.state == hagency_matrix::OutgoingState::Delivered
            && summary.id.as_deref() == Some(expected)
            && let Ok(Some(Ok(retained))) = original
                .publication
                .as_ref()
                .ok_or(FileError::Unknown)?
                .outcome()
            && retained.state == hagency_matrix::OutgoingState::Delivered
            && retained.id.as_deref() == Some(expected)
        {
            return Ok(());
        }
        return Err(FileError::Unknown);
    }
    Ok(())
}
