use super::*;

pub(super) async fn run(
    inner: &Arc<Inner>,
    job: &Arc<Job>,
    state: &mut State,
    cancel: &CancellationToken,
    deadline: Instant,
) -> Result<UploadReceipt, Error> {
    let prior = inner
        .domain
        .matrix_transport_state(inner.config.identity.transport.engagement_id.clone())
        .await?;
    if prior.is_none_or(|p| !p.available || p.observation != inner.config.identity.transport) {
        return Err(Error::Identity);
    }
    inner
        .domain
        .validate_upload_send(state.input.cap.clone(), state.input.claim.clone())
        .await?;
    if let Err(error) = inner.whoami(cancel).await {
        return fence(inner.clone(), job.clone(), error).await;
    }
    let request = inner.http.prepare_upload(
        state.input.media.ciphertext(),
        hagency_core::uploads::MAX_UPLOAD_BYTES as usize,
    )?;
    {
        let _busy = inner
            .busy
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Busy)?;
        let mut owner = inner.owner.lock().await;
        if owner.is_none() {
            *owner = Some(Owner::open_existing(&inner.config).await?);
        }
        let owner = owner.as_ref().ok_or(Error::Storage)?;
        let send = state.input.send.take().ok_or(Error::Conflict)?;
        state.reference = Some(owner.upload_reference(&send)?);
        let permit = owner.reserve_upload(send).await?;
        if owner.possible_upload(permit).await?.phase() != Phase::WritePossible {
            return Err(Error::OutcomeUnknown);
        }
    }
    #[cfg(test)]
    pause(inner, 1).await;
    // Last durable writer validation after all SDK awaits. No owner/busy lock,
    // replacement claim, or unrelated async operation between this and HTTP.
    inner
        .domain
        .validate_upload_send(state.input.cap.clone(), state.input.claim.clone())
        .await?;
    checkpoint(cancel, deadline)?;
    state.http_started = true;
    let response = inner
        .http
        .upload(
            request,
            deadline.min(Instant::now() + inner.config.limits.request),
            cancel,
        )
        .await?;
    // Assignment precedes every await and final cancellation check: a fully
    // checked late response is historical custody even if run then refuses.
    state.response = Some(response);
    checkpoint(cancel, deadline)?;
    #[cfg(test)]
    pause(inner, 2).await;
    let id = state
        .reference
        .as_ref()
        .ok_or(Error::Storage)?
        .id()
        .to_owned();
    settle(inner, &id, Some(state)).await
}

pub(super) async fn settle(
    inner: &Inner,
    id: &str,
    state: Option<&mut State>,
) -> Result<UploadReceipt, Error> {
    let (reference, acceptance) = {
        let _busy = inner
            .busy
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Busy)?;
        let mut owner = inner.owner.lock().await;
        if owner.is_none() {
            *owner = Some(Owner::open_existing(&inner.config).await?);
        }
        let owner = owner.as_ref().ok_or(Error::Storage)?;
        // Always re-read actual protected identity. A retained caller ID or
        // stale reference alone is insufficient historical SDK provenance.
        let reference = owner.restore_upload_reference(id).await?;
        let inspection = if let Some(response) = state.as_deref().and_then(|s| s.response.as_ref())
        {
            // The only response reachable here belongs to this exact job's one
            // HTTP request. Public callers cannot submit or substitute a body.
            let original = state
                .as_deref()
                .and_then(|s| s.reference.as_ref())
                .ok_or(Error::Storage)?;
            owner.accept_upload(original, response).await?
        } else {
            owner.inspect_upload(&reference).await?
        };
        if inspection.phase() != Phase::Accepted {
            return Err(Error::OutcomeUnknown);
        }
        let acceptance = inspection.receipt().cloned().ok_or(Error::OutcomeUnknown)?;
        (reference, acceptance)
    };
    let restored = inner
        .domain
        .restore_upload_settlement(
            reference.id().into(),
            reference.fence(),
            reference.stage().clone(),
            reference.route().clone(),
        )
        .await?
        .ok_or(Error::OutcomeUnknown)?;
    inner
        .domain
        .record_upload_settlement(Arc::new(restored), acceptance)
        .await
        .map_err(Error::from)
}
#[cfg(test)]
async fn pause(inner: &Inner, point: u8) {
    if inner.uploads.gate.load(std::sync::atomic::Ordering::SeqCst) == point {
        inner.uploads.reached.notify_one();
        inner.uploads.proceed.notified().await;
    }
}

// This sole detached completion retains the already admitted job/slot. At most
// two such tasks exist per Collector, with bounded DomainStore waits. It never
// owns HTTP permission or performs network IO. Caller cancellation cannot skip
// the observed negative identity before the writer sees it.
async fn fence(inner: Arc<Inner>, job: Arc<Job>, error: Error) -> Result<UploadReceipt, Error> {
    let completion = tokio::spawn(async move {
        #[cfg(test)]
        pause(&inner, 3).await;
        let result: Result<(), Error> = inner
            .fence_observation(inner.config.identity.transport.clone(), error)
            .await;
        let observed = result.err().unwrap_or(Error::OutcomeUnknown);
        if let Ok(mut value) = job.fence_error.lock() {
            *value = Some(observed);
        }
        #[cfg(test)]
        job.fence_finished.notify_one();
        observed
    });
    Err(completion.await.map_err(|_| Error::OutcomeUnknown)?)
}
