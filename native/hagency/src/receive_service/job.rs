use super::{ReceiveError, ReceiveFile, ReceiveView};
use crate::bootstrap::workspace::WorkspaceGuard;
use hagency_core::tasks::RunnerCapability;
use hagency_execution::WorkspaceReceive;
use hagency_matrix::{ReceivedAttachment, ReceivedScope};
use hagency_store::ReceiveAdmission;
use std::sync::Mutex;
use tokio::{sync::watch, time::Instant};

pub(super) struct Job {
    pub key: String,
    pub cap: RunnerCapability,
    pub input: ReceiveFile,
    pub deadline: Instant,
    pub info: Mutex<Info>,
    pub original: tokio::sync::Mutex<Original>,
    pub finished: watch::Sender<Option<Result<ReceiveView, ReceiveError>>>,
}
#[derive(Default)]
pub(super) struct Info {
    pub outcome: Option<Result<ReceiveView, ReceiveError>>,
    pub disposition: Disposition,
}
#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub(super) enum Disposition {
    #[default]
    Retain,
    Release,
    Ready,
}
#[derive(Default)]
pub(super) struct Original {
    pub admission: Option<ReceiveAdmission>,
    pub checked: Option<ReceivedAttachment>,
    pub sink: Option<WorkspaceReceive>,
    pub scope: Option<ReceivedScope>,
    pub workspace: Option<WorkspaceGuard>,
    pub write_possible: bool,
    pub historical_only: bool,
}
impl Job {
    pub fn finish(&self, result: Result<ReceiveView, ReceiveError>, disposition: Disposition) {
        let mut info = self.info.lock().unwrap_or_else(|e| e.into_inner());
        info.outcome = Some(result);
        info.disposition = disposition;
    }
    pub fn unknown(&self) {
        self.finish(Err(ReceiveError::Unknown), Disposition::Retain);
    }
    pub fn acknowledge(&self) {
        let result = self
            .info
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .outcome
            .clone()
            .unwrap_or(Err(ReceiveError::Unknown));
        self.finished.send_replace(Some(result));
    }
}
