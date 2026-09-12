//! A second, distinct event operation retaining the original large-media permit.
use super::*;
use crate::outgoing::{OutgoingSummary, Source};
use hagency_core::file_delivery::{
    CapturedFile, FileDeliveryAcceptance, FileDeliveryRequest, FileEventState,
    FilePublicationLocator,
};
use hagency_store::{FilePublicationClaim, FilePublicationSend};
use std::time::Duration;

pub struct FilePublicationOperation {
    inner: Arc<Inner>,
    job: Arc<Job>,
}
pub(super) struct Job {
    _original: Arc<super::state::Job>,
    identity: hagency_store::FileDeliveryIdentity,
    fence: u64,
    locator: FilePublicationLocator,
    metadata: FileDeliveryRequest,
    captured: CapturedFile,
    source: std::sync::Mutex<Option<FileSource>>,
    outcome: std::sync::Mutex<Option<Result<OutgoingSummary, Error>>>,
}
pub(crate) struct FileSource {
    pub cap: RunnerCapability,
    pub claim: FilePublicationClaim,
    pub start: Option<crate::sdk::file_publication::Start>,
    pub locator: hagency_core::file_delivery::FilePublicationLocator,
}
pub struct FilePublicationAdmissionFailure {
    error: Error,
    inputs: Box<(UploadOperation, FilePublicationClaim, FilePublicationSend)>,
}
impl FilePublicationAdmissionFailure {
    pub fn error(&self) -> Error {
        self.error
    }
    pub fn into_parts(self) -> (UploadOperation, FilePublicationClaim, FilePublicationSend) {
        *self.inputs
    }
}

impl UploadOperation {
    /// Synchronous admission returns original custody on refusal. No raw upload
    /// identifier, descriptor or historical receipt can construct this operation.
    pub fn prepare_file_publication(
        self,
        claim: FilePublicationClaim,
        send: FilePublicationSend,
    ) -> Result<FilePublicationOperation, FilePublicationAdmissionFailure> {
        let checked = (|| {
            let state = self.job.state.try_lock().map_err(|_| Error::Busy)?;
            if !matches!(state.outcome, Some(Ok(_))) {
                return Err(Error::OutcomeUnknown);
            }
            let reference = state.reference.as_ref().ok_or(Error::OutcomeUnknown)?;
            let l = send.locator();
            let receipt = state.input.media.receipt();
            if !send.matches_claim(&claim)
                || !send.matches_upload_claim(&state.input.claim)
                || reference.id() != l.upload_id
                || reference.fence() != l.upload_fence
                || reference.stage() != &l.stage
                || reference.route() != &l.route
                || send.captured().size != receipt.len() as u64
                || receipt.len() != state.input.media.ciphertext().len()
            {
                return Err(Error::Conflict);
            }
            Ok((
                state.input.cap.clone(),
                reference.clone(),
                state.input.media.descriptor().private_event_json().to_vec(),
            ))
        })();
        let (cap, reference, descriptor) = match checked {
            Ok(v) => v,
            Err(error) => {
                return Err(FilePublicationAdmissionFailure {
                    error,
                    inputs: Box::new((self, claim, send)),
                });
            }
        };
        // Keep a separate Arc so returning the exact original input never moves
        // a value while borrowing its registry. This does not duplicate media.
        let inner = self.inner.clone();
        let mut entries = match inner.uploads.entries.lock() {
            Ok(v) => v,
            Err(_) => {
                return Err(FilePublicationAdmissionFailure {
                    error: Error::Storage,
                    inputs: Box::new((self, claim, send)),
                });
            }
        };
        let id = send.locator().delivery_id.clone();
        if entries.closed || entries.publications.contains_key(&id) {
            return Err(FilePublicationAdmissionFailure {
                error: Error::Conflict,
                inputs: Box::new((self, claim, send)),
            });
        }
        let identity = send.identity().clone();
        let locator = send.locator().clone();
        let fence = locator.fence;
        let metadata = send.metadata().clone();
        let captured = send.captured().clone();
        let source = FileSource {
            cap,
            claim,
            locator: locator.clone(),
            start: Some(crate::sdk::file_publication::Start {
                send,
                reference,
                descriptor,
                joined: Default::default(),
            }),
        };
        let job = Arc::new(Job {
            _original: self.job,
            identity,
            fence,
            locator,
            metadata,
            captured,
            source: std::sync::Mutex::new(Some(source)),
            outcome: std::sync::Mutex::new(None),
        });
        entries.publications.insert(id, job.clone());
        drop(entries);
        Ok(FilePublicationOperation { inner, job })
    }
}

// Both paths release only the actual retained original Arc, and update its
// externally held operation outcome before removing the registry's custody.
fn finish_retained(inner: &Inner, job: &Arc<Job>, summary: &OutgoingSummary) -> Result<(), Error> {
    let mut entries = inner.uploads.entries.lock().map_err(|_| Error::Storage)?;
    let current = entries
        .publications
        .get(&job.locator.delivery_id)
        .ok_or(Error::Conflict)?;
    if !Arc::ptr_eq(current, job) {
        return Err(Error::Conflict);
    }
    *job.outcome.lock().map_err(|_| Error::Storage)? = Some(Ok(summary.clone()));
    entries.publications.remove(&job.locator.delivery_id);
    Ok(())
}

impl Inner {
    pub(crate) fn complete_file_publication(
        &self,
        locator: &FilePublicationLocator,
        metadata: &FileDeliveryRequest,
        captured: &CapturedFile,
        summary: &OutgoingSummary,
    ) -> Result<(), Error> {
        let job = self
            .uploads
            .entries
            .lock()
            .map_err(|_| Error::Storage)?
            .publications
            .get(&locator.delivery_id)
            .cloned();
        // A fresh process can settle protected Complete without an in-memory
        // upload or publication owner; there is then no media entry to release.
        let Some(job) = job else {
            return Ok(());
        };
        if job.locator != *locator || job.metadata != *metadata || job.captured != *captured {
            return Err(Error::Conflict);
        }
        finish_retained(self, &job, summary)
    }

    pub(crate) async fn resume_settled_file_publication(
        &self,
        receipts: &[crate::outgoing::state::Receipt],
    ) -> Result<OutgoingSummary, Error> {
        use crate::outgoing::{OutgoingState, state::Kind};
        // The original shared media permits bound this snapshot. The registry
        // lock is released before any domain queue or SQLite wait.
        let jobs: Vec<_> = self
            .uploads
            .entries
            .lock()
            .map_err(|_| Error::Storage)?
            .publications
            .values()
            .cloned()
            .collect();
        if jobs.is_empty() {
            return Ok(OutgoingSummary {
                id: None,
                state: OutgoingState::Idle,
                replayed: false,
            });
        }
        for job in jobs {
            let mut matching = receipts.iter().filter(|receipt| {
                receipt.kind == Kind::File
                    && receipt.id == job.locator.delivery_id
                    && receipt.fence == job.locator.fence
            });
            let Some(receipt) = matching.next() else {
                continue;
            };
            if matching.next().is_some() || !crate::outgoing::state::digest(&receipt.attempt_digest)
            {
                return Err(Error::Storage);
            }
            let settlement = Arc::new(
                self.domain
                    .restore_file_delivery_settlement_for_content(
                        job.locator.clone(),
                        job.metadata.clone(),
                        job.captured.clone(),
                    )
                    .await?
                    .ok_or(Error::OutcomeUnknown)?,
            );
            let original = self
                .domain
                .inspect_file_delivery_settlement(settlement.clone())
                .await?;
            if original.event != FileEventState::Delivered {
                return Err(Error::OutcomeUnknown);
            }
            let event_id = original.event_id.ok_or(Error::OutcomeUnknown)?;
            // A settled SDK receipt no longer carries the event response. Only
            // the already-Delivered original domain receipt supplies its event
            // ID, and exact re-record compares the SDK attempt digest with that
            // same prior acceptance. This path cannot commit first Delivered.
            let replay = self
                .domain
                .record_file_delivery_settlement(
                    settlement,
                    FileDeliveryAcceptance {
                        transaction_id: job.locator.transaction_id.clone(),
                        content_digest: job.locator.content_digest.clone(),
                        event_id,
                        receipt_id: format!("file_receipt_{}", receipt.attempt_digest),
                        receipt_digest: receipt.attempt_digest.clone(),
                    },
                )
                .await?;
            if !replay.replayed || replay.event != FileEventState::Delivered {
                return Err(Error::OutcomeUnknown);
            }
            let summary = OutgoingSummary {
                id: Some(job.locator.delivery_id.clone()),
                state: OutgoingState::Delivered,
                replayed: true,
            };
            finish_retained(self, &job, &summary)?;
            return Ok(summary);
        }
        // An unrun or unresolved original job is still custody even when the
        // SDK has no active attempt. Absence never means Idle or safe reissue.
        Err(Error::OutcomeUnknown)
    }
}

async fn confirm_historical_delivery(inner: &Inner, job: &Job) -> Result<(), Error> {
    let settlement = inner
        .domain
        .restore_file_delivery_settlement_for_content(
            job.locator.clone(),
            job.metadata.clone(),
            job.captured.clone(),
        )
        .await?
        .ok_or(Error::OutcomeUnknown)?;
    let receipt = inner
        .domain
        .inspect_file_delivery_settlement(Arc::new(settlement))
        .await?;
    if receipt.event != FileEventState::Delivered || receipt.event_id.is_none() {
        return Err(Error::OutcomeUnknown);
    }
    Ok(())
}

impl FilePublicationOperation {
    /// A single finite owned task retains custody after the caller drops its
    /// wait. The original two-media semaphore remains held throughout.
    pub async fn run(&mut self, cancel: &CancellationToken) -> Result<OutgoingSummary, Error> {
        let permit = self
            .inner
            .busy
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Busy)?;
        let source = self
            .job
            .source
            .lock()
            .map_err(|_| Error::Storage)?
            .take()
            .ok_or(Error::Conflict)?;
        let job = self.job.clone();
        let inner = self.inner.clone();
        let cancel = cancel.child_token();
        tokio::spawn(async move {
            let _permit = permit;
            let work = inner.outgoing(Source::File(Box::new(source)), &cancel);
            tokio::pin!(work);
            let result = tokio::select! {
                result = &mut work => result,
                _ = tokio::time::sleep(Duration::from_secs(45)) => { cancel.cancel(); work.await }
            };
            if result.is_err() {
                // The already owned task retains this negative completion. A
                // lost caller cannot skip cancellation or uncertainty custody.
                let cancelling = cancel.is_cancelled();
                let fenced = if cancelling {
                    inner
                        .domain
                        .cancel_file_delivery(
                            job.identity.clone(),
                            hagency_core::file_delivery::FileDeliveryFailure::Cancelled,
                        )
                        .await
                } else {
                    inner
                        .domain
                        .mark_file_publication_uncertain(job.identity.clone(), job.fence)
                        .await
                };
                if let Err(error) = fenced {
                    let failure = if !cancelling && matches!(error, hagency_store::Error::State) {
                        // Settle's SDK reply can be lost after the domain already
                        // recorded Delivered. Only exact historical row/content
                        // inspection can show this negative setter is inapplicable.
                        // Preserve the original run error and all retained custody;
                        // a later resume still needs the actual private SDK receipt.
                        confirm_historical_delivery(&inner, &job).await.err()
                    } else {
                        Some(Error::from(error))
                    };
                    if let Some(error) = failure {
                        *job.outcome.lock().map_err(|_| Error::Storage)? = Some(Err(error));
                        return Err(error);
                    }
                }
            }
            *job.outcome.lock().map_err(|_| Error::Storage)? = Some(result.clone());
            result
        })
        .await
        .map_err(|_| Error::OutcomeUnknown)?
    }
    pub fn outcome(&self) -> Result<Option<Result<OutgoingSummary, Error>>, Error> {
        Ok(self.job.outcome.lock().map_err(|_| Error::Storage)?.clone())
    }
}
