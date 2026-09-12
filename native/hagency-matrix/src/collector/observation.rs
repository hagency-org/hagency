//! Test-only, per-original-operation evidence. No payload or global last-operation state.
use crate::Error;
use std::{
    future::Future,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

tokio::task_local! { static ACTIVE: Option<Arc<Trace>>; }
const EVENTS: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Phase {
    OutgoingPreview,
    OutgoingBegin,
    OutgoingValidate,
    OutgoingQueryHttp,
    OutgoingWriteHttp,
    OutgoingSettle,
    OutgoingRetireRoom,
    ExpectedTransport,
    Whoami,
    OwnerLock,
    OpenOwner,
    Batch,
    IntakeMode,
    Cursor,
    SyncHttp,
    SyncApply,
    PublishTransport,
    RoomPrior,
    RoomHttp,
    RoomPublish,
    RoomRecheck,
    Targets,
    IntakeStart,
    IntakeFilesCheck,
    IntakeFilesChecked,
    IntakeBatchBuild,
    IntakeBatchBuilt,
    IntakeReplayPersist,
    IntakeReplayPersisted,
    IntakePreparedPersist,
    IntakePreparedPersisted,
    IntakeApplyingPersist,
    IntakeApplyingPersisted,
    IntakeResponseDecode,
    IntakeResponseDecoded,
    IntakeSyncApply,
    IntakeSyncApplied,
    IntakeDerive,
    IntakeDerived,
    IntakeEventQuarantine,
    IntakeEventQuarantined,
    IntakeAttachmentQuarantine,
    IntakeAttachmentQuarantined,
    IntakeDerivedPersist,
    IntakeDerivedPersisted,
    IntakeFinalFilesCheck,
    IntakeFinalFilesChecked,
    HandoffLock,
    HistoricalReceipt,
    Admission,
    Acknowledge,
    Finish,
    Quarantine,
    CloseTransportRead,
    CloseTransportFence,
    CloseOwnerLock,
    CloseSdk,
    Fence,
    FenceReturned,
    PrimaryError,
    OperationReturned,
    OwnerReturned,
    OpenRequested,
    PrepareStarted,
    PrepareFailed,
    RuntimeFailed,
    Prepared,
    RuntimeReady,
    SdkOpenStarted,
    StateStoreOpen,
    CryptoStoreOpen,
    AccountLoad,
    Activate,
    Identity,
    JournalLoad,
    SdkOpenReturned,
    OpenCallerReturned,
    Queued,
    Started,
    Returned,
    CallerReturned,
    CloseStoresStarted,
    CloseStoresReturned,
    RuntimeDropStarted,
    RuntimeDropped,
    LockDropStarted,
    LockDropped,
    CloseAcknowledgement,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SdkCommand {
    OutgoingRead,
    OutgoingStart,
    OutgoingBegun,
    OutgoingQuery,
    OutgoingEncrypt,
    OutgoingPossible,
    OutgoingAccept,
    OutgoingSettle,
    Cursor,
    Sync,
    IntakeMode,
    Batch,
    Start,
    Ack,
    Finish,
    Quarantine,
    Close,
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct Event {
    pub phase: Phase,
    pub elapsed_us: u64,
    pub command: Option<SdkCommand>,
    pub sequence: u32,
    pub index: Option<usize>,
    pub error: Option<Error>,
}
#[derive(Clone, Debug)]
pub(crate) struct Snapshot {
    pub callsite: &'static str,
    pub variant: Option<&'static str>,
    pub batch: Option<u8>,
    pub events: [Option<Event>; EVENTS],
    pub next: usize,
    pub primary: Option<Event>,
    pub fence: Option<Event>,
    pub sdk_failure: Option<Event>,
    sequence: u32,
}
pub(crate) struct Trace {
    start: Instant,
    snapshot: Mutex<Snapshot>,
    gate: Mutex<Option<Gate>>,
    changed: tokio::sync::Notify,
}
impl Trace {
    pub(crate) fn new(
        callsite: &'static str,
        variant: Option<&'static str>,
        batch: Option<u8>,
    ) -> Arc<Self> {
        assert!(batch.is_none_or(|n| n < 2));
        Arc::new(Self {
            start: Instant::now(),
            snapshot: Mutex::new(Snapshot {
                callsite,
                variant,
                batch,
                events: [None; EVENTS],
                next: 0,
                primary: None,
                fence: None,
                sdk_failure: None,
                sequence: 0,
            }),
            gate: Mutex::new(None),
            changed: tokio::sync::Notify::new(),
        })
    }
    pub(crate) fn snapshot(&self) -> Snapshot {
        self.snapshot
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
    pub(crate) fn record(&self, phase: Phase, error: Option<Error>) {
        self.event(phase, None, 0, None, error);
    }
    fn event(
        &self,
        phase: Phase,
        command: Option<SdkCommand>,
        sequence: u32,
        index: Option<usize>,
        error: Option<Error>,
    ) {
        {
            let mut state = self.snapshot.lock().unwrap_or_else(|e| e.into_inner());
            let event = Event {
                phase,
                command,
                sequence,
                index,
                error,
                elapsed_us: self
                    .start
                    .elapsed()
                    .as_micros()
                    .try_into()
                    .unwrap_or(u64::MAX),
            };
            let next = state.next;
            state.events[next] = Some(event);
            state.next = (next + 1) % EVENTS;
            if phase == Phase::PrimaryError && state.primary.is_none() {
                state.primary = Some(event);
            }
            if phase == Phase::FenceReturned && error.is_some() && state.fence.is_none() {
                state.fence = Some(event);
            }
            if command.is_some() && error.is_some() && state.sdk_failure.is_none() {
                state.sdk_failure = Some(event);
            }
        }
        self.changed.notify_one();
        let gate = {
            let mut gate = self.gate.lock().unwrap_or_else(|e| e.into_inner());
            if gate.as_ref().is_some_and(|g| g.phase == phase) {
                gate.take()
            } else {
                None
            }
        };
        if let Some(gate) = gate {
            let _ = gate.reached.send(());
            gate.release
                .recv_timeout(Duration::from_secs(6))
                .expect("original SDK fixture boundary was not released");
        }
    }
    pub(crate) fn has(&self, phase: Phase) -> bool {
        self.snapshot()
            .events
            .iter()
            .flatten()
            .any(|e| e.phase == phase)
    }
    pub(crate) async fn wait(&self, phase: Phase) {
        tokio::time::timeout(Duration::from_secs(2), async {
            while !self.has(phase) {
                self.changed.notified().await;
            }
        })
        .await
        .expect("original operation phase not observed");
    }
    pub(crate) fn hold(&self, phase: Phase) -> Held {
        let (reached, ready) = tokio::sync::oneshot::channel();
        let (release, held) = std::sync::mpsc::channel();
        let mut gate = self.gate.lock().unwrap_or_else(|e| e.into_inner());
        assert!(gate.is_none());
        *gate = Some(Gate {
            phase,
            reached,
            release: held,
        });
        Held {
            ready,
            release: Some(release),
        }
    }
    pub(crate) fn print(&self) {
        eprintln!("original Matrix operation: {:?}", self.snapshot());
    }
}
struct Gate {
    phase: Phase,
    reached: tokio::sync::oneshot::Sender<()>,
    release: std::sync::mpsc::Receiver<()>,
}
pub(crate) struct Held {
    ready: tokio::sync::oneshot::Receiver<()>,
    release: Option<std::sync::mpsc::Sender<()>>,
}
impl Held {
    pub(crate) async fn reached(&mut self) {
        tokio::time::timeout(Duration::from_secs(2), &mut self.ready)
            .await
            .unwrap()
            .unwrap();
    }
    pub(crate) fn release(mut self) {
        self.release.take().unwrap().send(()).unwrap();
    }
}
impl Drop for Held {
    fn drop(&mut self) {
        if let Some(release) = self.release.take() {
            let _ = release.send(());
        }
    }
}
pub(crate) fn current() -> Option<Arc<Trace>> {
    ACTIVE.try_with(Clone::clone).ok().flatten()
}
pub(crate) async fn scope<T>(trace: Option<Arc<Trace>>, future: impl Future<Output = T>) -> T {
    ACTIVE.scope(trace, future).await
}
pub(crate) async fn owned<T>(
    trace: Option<Arc<Trace>>,
    future: impl Future<Output = Result<T, Error>>,
) -> Result<T, Error> {
    let result = scope(trace.clone(), future).await;
    if let Some(trace) = trace {
        trace.record(Phase::OwnerReturned, result.as_ref().err().copied());
    }
    result
}
pub(crate) fn mark(phase: Phase, index: Option<usize>) {
    if let Some(trace) = current() {
        trace.event(phase, None, 0, index, None);
    }
}
pub(crate) fn primary(error: Error) {
    if let Some(trace) = current() {
        trace.record(Phase::PrimaryError, Some(error));
    }
}
pub(crate) fn fence(error: Option<Error>) {
    if let Some(trace) = current() {
        trace.record(Phase::FenceReturned, error);
    }
}
struct OnPanic(Arc<Trace>);
impl Drop for OnPanic {
    fn drop(&mut self) {
        if std::thread::panicking() {
            self.0.print();
        }
    }
}
pub(crate) async fn observed<T>(
    trace: Arc<Trace>,
    future: impl Future<Output = Result<T, Error>>,
) -> Result<T, Error> {
    let _panic = OnPanic(trace.clone());
    let result = scope(Some(trace.clone()), future).await;
    trace.record(Phase::OperationReturned, result.as_ref().err().copied());
    if result.is_err() {
        trace.print();
    }
    result
}
#[derive(Clone)]
pub(crate) struct CommandTrace {
    trace: Arc<Trace>,
    command: SdkCommand,
    sequence: u32,
}
impl CommandTrace {
    pub(crate) fn current(command: SdkCommand) -> Option<Self> {
        let trace = current()?;
        let sequence = {
            let mut state = trace.snapshot.lock().unwrap_or_else(|e| e.into_inner());
            state.sequence = state.sequence.saturating_add(1);
            state.sequence
        };
        Some(Self {
            trace,
            command,
            sequence,
        })
    }
    pub(crate) fn record(&self, phase: Phase, error: Option<Error>) {
        self.trace
            .event(phase, Some(self.command), self.sequence, None, error);
    }
}
pub(crate) fn command(trace: &Option<CommandTrace>, phase: Phase, error: Option<Error>) {
    if let Some(trace) = trace {
        trace.record(phase, error);
    }
}
