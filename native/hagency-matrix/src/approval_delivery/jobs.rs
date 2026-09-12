//! Original host jobs remain owned even when their waiting caller disappears.
use super::{PrivateApprovalDeliveryStatus, PrivateApprovalDeliverySummary};
use crate::Error;
use std::{
    future::Future,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::{
    sync::{Notify, OwnedSemaphorePermit},
    task::JoinHandle,
};
#[derive(Clone)]
pub(crate) enum Value {
    Unit,
    Delivery(PrivateApprovalDeliverySummary),
    Status(PrivateApprovalDeliveryStatus),
}
pub(crate) struct Job {
    result: Mutex<Option<Result<Value, Error>>>,
    blocks_after_error: Arc<AtomicBool>,
    notify: Notify,
    handle: Mutex<Option<JoinHandle<()>>>,
}
impl Job {
    pub(crate) async fn wait(&self) -> Result<Value, Error> {
        loop {
            let notified = self.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if let Some(result) = self
                .result
                .lock()
                .map_err(|_| Error::OutcomeUnknown)?
                .clone()
            {
                return result;
            }
            notified.await;
        }
    }
}
#[derive(Default)]
pub(crate) struct Jobs(Mutex<State>);
#[derive(Default)]
struct State {
    last: Option<Arc<Job>>,
    inspection: Option<Arc<Job>>,
    close: Option<Arc<Job>>,
}
impl Jobs {
    pub(crate) fn check(&self, historical: bool) -> Result<(), Error> {
        let state = self.0.lock().map_err(|_| Error::OutcomeUnknown)?;
        if state.close.is_some() {
            return Err(Error::Generation);
        }
        if let Some(last) = &state.last {
            match last
                .result
                .lock()
                .map_err(|_| Error::OutcomeUnknown)?
                .as_ref()
            {
                None => return Err(Error::Busy),
                Some(Err(error))
                    if !historical && last.blocks_after_error.load(Ordering::Acquire) =>
                {
                    return Err(*error);
                }
                _ => {}
            }
        }
        Ok(())
    }
    pub(crate) fn missing_owner_result(&self) -> Result<(), Error> {
        let state = self.0.lock().map_err(|_| Error::OutcomeUnknown)?;
        if let Some(last) = &state.last
            && !matches!(
                last.result
                    .lock()
                    .map_err(|_| Error::OutcomeUnknown)?
                    .as_ref(),
                Some(Ok(_))
            )
        {
            return Err(Error::OutcomeUnknown);
        }
        Ok(())
    }
    pub(crate) fn closing(&self) -> Result<Option<Arc<Job>>, Error> {
        Ok(self
            .0
            .lock()
            .map_err(|_| Error::OutcomeUnknown)?
            .close
            .clone())
    }
    pub(crate) fn start(
        &self,
        close: bool,
        historical: bool,
        permit: OwnedSemaphorePermit,
        work: impl Future<Output = Result<Value, Error>> + Send + 'static,
    ) -> Result<Arc<Job>, Error> {
        self.start_classified(
            close,
            historical,
            permit,
            Arc::new(AtomicBool::new(true)),
            work,
        )
    }
    pub(crate) fn start_classified(
        &self,
        close: bool,
        historical: bool,
        permit: OwnedSemaphorePermit,
        blocks_after_error: Arc<AtomicBool>,
        work: impl Future<Output = Result<Value, Error>> + Send + 'static,
    ) -> Result<Arc<Job>, Error> {
        let mut state = self.0.lock().map_err(|_| Error::OutcomeUnknown)?;
        if let Some(job) = &state.close {
            return if close {
                Ok(job.clone())
            } else {
                Err(Error::Generation)
            };
        }
        if !close
            && !historical
            && let Some(last) = &state.last
        {
            match last
                .result
                .lock()
                .map_err(|_| Error::OutcomeUnknown)?
                .as_ref()
            {
                Some(Ok(_)) => {}
                Some(Err(e)) if last.blocks_after_error.load(Ordering::Acquire) => {
                    return Err(*e);
                }
                Some(Err(_)) => {}
                None => return Err(Error::Busy),
            }
        }
        let job = Arc::new(Job {
            result: Mutex::new(None),
            blocks_after_error,
            notify: Notify::new(),
            handle: Mutex::new(None),
        });
        if close {
            state.close = Some(job.clone());
        } else if historical {
            state.inspection = Some(job.clone());
        } else {
            state.last = Some(job.clone());
        }
        // The registry keeps the actual handle; dropping wait() cannot drop work.
        let completion = Completion(job.clone());
        let handle = tokio::spawn(async move {
            let _permit = permit;
            let result = work.await;
            completion.finish(result);
        });
        *job.handle.lock().map_err(|_| Error::OutcomeUnknown)? = Some(handle);
        Ok(job)
    }
}
struct Completion(Arc<Job>);
impl Completion {
    fn finish(self, result: Result<Value, Error>) {
        if let Ok(mut slot) = self.0.result.lock() {
            *slot = Some(result);
        }
        self.0.notify.notify_waiters();
    }
}
impl Drop for Completion {
    fn drop(&mut self) {
        if let Ok(mut slot) = self.0.result.lock()
            && slot.is_none()
        {
            self.0.blocks_after_error.store(true, Ordering::Release);
            *slot = Some(Err(Error::OutcomeUnknown));
        }
        self.0.notify.notify_waiters();
    }
}
