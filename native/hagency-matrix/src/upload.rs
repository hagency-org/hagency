//! One owned staged POST, separate from room-event delivery and task truth.
use crate::{
    CancellationToken, Collector, Error, UploadResponse,
    collector::Inner,
    sdk::{
        Owner,
        upload_state::{Phase, Reference},
    },
};
use hagency_core::{tasks::RunnerCapability, uploads::UploadReceipt};
use std::{future::Future, sync::Arc};
use tokio::time::Instant;
mod operation;
pub(crate) mod publication;
mod state;
pub use publication::{FilePublicationAdmissionFailure, FilePublicationOperation};
pub(crate) use state::Registry;
use state::{Job, State};
pub use state::{StagedUpload, UploadAdmissionFailure};

/// Unique driving handle. Dropping a run future never removes the Collector's
/// retained job. Dropping every owner loses in-memory uncertainty; it does not
/// prove an upload was unsent or permit another POST.
pub struct UploadOperation {
    inner: Arc<Inner>,
    job: Arc<Job>,
}
impl UploadOperation {
    pub fn id(&self) -> &str {
        &self.job.id
    }
    /// Drives at most once without a network worker or automatic retry. A negative
    /// account observation retains one bounded fencing completion independently.
    pub async fn run(&mut self, cancel: &CancellationToken) -> Result<UploadReceipt, Error> {
        let mut state = self.job.state.try_lock().map_err(|_| Error::Busy)?;
        if state.attempted {
            return Err(Error::Conflict);
        }
        state.attempted = true;
        let deadline = Instant::now() + self.inner.config.limits.sdk;
        let result = bounded(
            cancel,
            deadline,
            operation::run(&self.inner, &self.job, &mut state, cancel, deadline),
        )
        .await;
        state.outcome = Some(result.clone());
        // This completed local driver knows it never polled HTTP. Domain and
        // SDK Possible records remain unchanged and cannot reissue Send.
        if result.is_ok() || !state.http_started {
            self.inner.uploads.release(&self.job.id)?;
        }
        result
    }
    pub fn outcome(&self) -> Result<Option<Result<UploadReceipt, Error>>, Error> {
        let state = self.job.state.try_lock().map_err(|_| Error::Busy)?;
        Ok(state
            .outcome
            .clone()
            .or_else(|| self.job.fence_error.lock().ok().and_then(|v| v.map(Err))))
    }
}
impl Collector {
    /// Synchronous finite admission, before any async cancellation point. On
    /// refusal the caller receives its exact original typed input.
    pub fn stage_upload(
        &self,
        input: StagedUpload,
    ) -> Result<UploadOperation, UploadAdmissionFailure> {
        let failure = |error, input| UploadAdmissionFailure::new(error, input);
        if !input.matches() {
            return Err(failure(Error::Conflict, input));
        }
        let Ok(mut entries) = self.inner.uploads.entries.lock() else {
            return Err(failure(Error::Storage, input));
        };
        if entries.closed || self.inner.config.endpoint.scheme() != "https" {
            return Err(failure(Error::Config, input));
        }
        let id = input
            .send
            .as_ref()
            .expect("unadmitted send retained")
            .identity()
            .id()
            .to_owned();
        if entries.jobs.contains_key(&id) {
            return Err(failure(Error::Conflict, input));
        }
        let Ok(slot) = self.inner.uploads.slots.clone().try_acquire_owned() else {
            return Err(failure(Error::Capacity, input));
        };
        let job = Arc::new(Job {
            id: id.clone(),
            state: tokio::sync::Mutex::new(State {
                input,
                attempted: false,
                http_started: false,
                reference: None,
                response: None,
                outcome: None,
            }),
            _slot: slot,
            fence_error: std::sync::Mutex::new(None),
            #[cfg(test)]
            fence_finished: tokio::sync::Notify::new(),
        });
        entries.jobs.insert(id, job.clone());
        Ok(UploadOperation {
            inner: self.inner.clone(),
            job,
        })
    }
    /// Release an abandoned local attempt only if its driver is not actively running
    /// and never polled HTTP. This changes no durable Possible state or grant.
    pub fn release_unstarted_upload(&self, id: &str) -> Result<(), Error> {
        let job = self.inner.uploads.find(id)?.ok_or(Error::OutcomeUnknown)?;
        let mut state = job.state.try_lock().map_err(|_| Error::Busy)?;
        if state.http_started || state.response.is_some() {
            return Err(Error::OutcomeUnknown);
        }
        state.attempted = true;
        self.inner.uploads.release(id)
    }
    /// Exact historical settlement. A known ID is a bounded lookup selector,
    /// never proof of acceptance; evidence comes only from the protected SDK.
    pub async fn settle_upload(
        &self,
        id: &str,
        cancel: &CancellationToken,
    ) -> Result<UploadReceipt, Error> {
        let retained = self.inner.uploads.find(id)?;
        let deadline = Instant::now() + self.inner.config.limits.sdk;
        let result = if let Some(job) = retained {
            let mut state = job.state.try_lock().map_err(|_| Error::Busy)?;
            let result = bounded(
                cancel,
                deadline,
                operation::settle(&self.inner, id, Some(&mut state)),
            )
            .await;
            if result.is_ok() {
                state.outcome = Some(result.clone());
            }
            result
        } else {
            bounded(cancel, deadline, operation::settle(&self.inner, id, None)).await
        };
        if result.is_ok() {
            self.inner.uploads.release(id)?;
        }
        result
    }
    /// Replace only the SDK owner while retained operation/response custody
    /// stays in this Collector. No POST, current grant or fresh SDK identity.
    pub async fn reopen_upload_owner(&self, cancel: &CancellationToken) -> Result<(), Error> {
        let deadline = Instant::now() + self.inner.config.limits.sdk;
        bounded(cancel, deadline, async {
            let _busy = self
                .inner
                .busy
                .clone()
                .try_acquire_owned()
                .map_err(|_| Error::Busy)?;
            let mut owner = self.inner.owner.lock().await;
            if let Some(previous) = owner.take() {
                previous.close().await?;
            }
            *owner = Some(Owner::open_existing(&self.inner.config).await?);
            Ok(())
        })
        .await
    }
}
fn checkpoint(cancel: &CancellationToken, deadline: Instant) -> Result<(), Error> {
    if cancel.is_cancelled() {
        Err(Error::Cancelled)
    } else if Instant::now() >= deadline {
        Err(Error::Timeout)
    } else {
        Ok(())
    }
}
async fn bounded<T>(
    cancel: &CancellationToken,
    deadline: Instant,
    future: impl Future<Output = Result<T, Error>>,
) -> Result<T, Error> {
    checkpoint(cancel, deadline)?;
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(Error::Cancelled),
        _ = tokio::time::sleep_until(deadline) => Err(Error::Timeout),
        result = future => { checkpoint(cancel, deadline)?; result }
    }
}
#[cfg(test)]
#[path = "../tests/file_publication/mod.rs"]
mod file_tests;
#[cfg(test)]
#[path = "../tests/staged_upload/mod.rs"]
mod tests;
