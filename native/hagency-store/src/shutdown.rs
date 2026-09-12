//! Fixed-size, host-local observations of one shutdown attempt. Never authority.
use rusqlite::trace::{TraceEvent, TraceEventCodes};
use std::{
    cell::RefCell,
    marker::PhantomData,
    rc::Rc,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicU16, AtomicU64, Ordering},
    },
    time::Instant,
};
mod native_writer;
pub use native_writer::{NativeWriterObservation, NativeWriterUnavailable};

/// The original caller verdict, with the wait stage distinguished for diagnosis.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShutdownOutcome {
    Complete,
    EnqueueTimedOut,
    ReplyTimedOut,
    EnqueueClosed,
    ReplyClosed,
}

/// A momentary snapshot, in microseconds relative to this attempt's creation.
///
/// Missing phases mean unobserved, not rollback or repository closure. Worker
/// pickup can precede the caller observing enqueue completion. Likewise a caller
/// can receive the acknowledgement before its sender publishes `sent_us`.
/// `Complete` preserves the original acknowledgement verdict; this snapshot
/// neither proves OS-thread exit nor adds any permission to retry a write.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShutdownSnapshot {
    pub outcome: ShutdownOutcome,
    pub enqueue_started_us: Option<u64>,
    pub enqueue_observed_us: Option<u64>,
    pub worker_picked_up_us: Option<u64>,
    pub drop_started_us: Option<u64>,
    pub drop_finished_us: Option<u64>,
    /// Domain repository fields only; absent for custody-store shutdown.
    pub connection_drop_started_us: Option<u64>,
    /// SQLite's CLOSE callback ran; it does not prove successful close or release.
    pub sqlite_close_entered_us: Option<u64>,
    pub connection_drop_finished_us: Option<u64>,
    pub ownership_drop_started_us: Option<u64>,
    pub ownership_drop_finished_us: Option<u64>,
    pub acknowledgement_started_us: Option<u64>,
    pub acknowledgement_sent_us: Option<u64>,
    pub caller_finished_us: Option<u64>,
    /// CPU interval starts before domain Connection drop, not at SQLite CLOSE.
    /// Frozen by the first snapshot; never identifies the waiting backend call.
    pub native_writer: NativeWriterObservation,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum Phase {
    EnqueueStarted,
    EnqueueObserved,
    WorkerPickedUp,
    DropStarted,
    DropFinished,
    AcknowledgementStarted,
    AcknowledgementSent,
    CallerFinished,
    ConnectionDropStarted,
    ConnectionDropFinished,
    OwnershipDropStarted,
    OwnershipDropFinished,
    SqliteCloseEntered,
}

pub(crate) struct Probe {
    started: Instant,
    timestamps: [AtomicU64; 13],
    completed: AtomicU16,
    native_writer: OnceLock<native_writer::Writer>,
    #[cfg(test)]
    pause: std::sync::Mutex<Option<TestPause>>,
}

impl Probe {
    pub(crate) fn new() -> Self {
        Self {
            started: Instant::now(),
            timestamps: std::array::from_fn(|_| AtomicU64::new(0)),
            completed: AtomicU16::new(0),
            native_writer: OnceLock::new(),
            #[cfg(test)]
            pause: std::sync::Mutex::new(None),
        }
    }

    pub(crate) fn observe_domain_writer(&self) {
        // Only the original domain writer calls this before its Connection drop.
        // A failed capture is retained too; later snapshots cannot retry it.
        self.native_writer
            .get_or_init(native_writer::Writer::capture);
    }

    // Each phase has exactly one publisher per attempt. Publish its timestamp
    // before the Release bit; snapshot's Acquire makes that timestamp visible.
    pub(crate) fn mark(&self, phase: Phase) {
        let index = phase as usize;
        let elapsed = u64::try_from(self.started.elapsed().as_micros()).unwrap_or(u64::MAX);
        self.timestamps[index].store(elapsed, Ordering::Relaxed);
        self.completed.fetch_or(1 << index, Ordering::Release);
        #[cfg(test)]
        self.pause_at(phase);
    }

    pub(crate) fn snapshot(&self, outcome: ShutdownOutcome) -> ShutdownSnapshot {
        let completed = self.completed.load(Ordering::Acquire);
        let at = |phase: Phase| {
            let index = phase as usize;
            (completed & (1 << index) != 0).then(|| self.timestamps[index].load(Ordering::Relaxed))
        };
        ShutdownSnapshot {
            outcome,
            enqueue_started_us: at(Phase::EnqueueStarted),
            enqueue_observed_us: at(Phase::EnqueueObserved),
            worker_picked_up_us: at(Phase::WorkerPickedUp),
            drop_started_us: at(Phase::DropStarted),
            drop_finished_us: at(Phase::DropFinished),
            connection_drop_started_us: at(Phase::ConnectionDropStarted),
            sqlite_close_entered_us: at(Phase::SqliteCloseEntered),
            connection_drop_finished_us: at(Phase::ConnectionDropFinished),
            ownership_drop_started_us: at(Phase::OwnershipDropStarted),
            ownership_drop_finished_us: at(Phase::OwnershipDropFinished),
            acknowledgement_started_us: at(Phase::AcknowledgementStarted),
            acknowledgement_sent_us: at(Phase::AcknowledgementSent),
            caller_finished_us: at(Phase::CallerFinished),
            native_writer: self.native_writer.get().map_or(
                NativeWriterObservation::Unobserved,
                native_writer::Writer::snapshot,
            ),
        }
    }
}

thread_local! {
    // One scoped association on the thread performing the original drop.
    static SQLITE_CLOSE_PROBE: RefCell<Option<Arc<Probe>>> = const { RefCell::new(None) };
}

pub(crate) struct SqliteCloseScope {
    probe: Arc<Probe>,
    // The slot belongs to this thread; the guard must be neither Send nor Sync.
    _thread: PhantomData<Rc<()>>,
}

impl SqliteCloseScope {
    pub(crate) fn install(connection: &rusqlite::Connection, probe: &Arc<Probe>) -> Option<Self> {
        SQLITE_CLOSE_PROBE
            .try_with(|slot| {
                let mut current = slot.try_borrow_mut().ok()?;
                if current.is_some() {
                    return None;
                }
                *current = Some(probe.clone());
                Some(())
            })
            .ok()??;
        let scope = Self {
            probe: probe.clone(),
            _thread: PhantomData,
        };
        // No statement, row, profile or global logger is registered. The safe
        // callback ignores the connection reference, including its filename.
        connection.trace_v2(TraceEventCodes::SQLITE_TRACE_CLOSE, Some(sqlite_close));
        Some(scope)
    }
}

impl Drop for SqliteCloseScope {
    fn drop(&mut self) {
        let _ = SQLITE_CLOSE_PROBE.try_with(|slot| {
            if let Ok(mut current) = slot.try_borrow_mut()
                && current
                    .as_ref()
                    .is_some_and(|probe| Arc::ptr_eq(probe, &self.probe))
            {
                current.take();
            }
        });
    }
}

fn sqlite_close(event: TraceEvent<'_>) {
    if !matches!(event, TraceEvent::Close(_)) {
        return;
    }
    let probe = SQLITE_CLOSE_PROBE
        .try_with(|slot| slot.try_borrow().ok().and_then(|current| current.clone()))
        .ok()
        .flatten();
    // Release the slot borrow before publishing or entering a test-only pause.
    if let Some(probe) = probe {
        probe.mark(Phase::SqliteCloseEntered);
    }
}

pub(crate) fn mark(probe: &Option<std::sync::Arc<Probe>>, phase: Phase) {
    if let Some(probe) = probe {
        probe.mark(phase);
    }
}

#[cfg(test)]
struct TestPause {
    phase: Phase,
    reached: tokio::sync::oneshot::Sender<()>,
    resume: std::sync::mpsc::Receiver<()>,
}

#[cfg(test)]
impl Probe {
    pub(crate) fn paused(
        phase: Phase,
    ) -> (
        std::sync::Arc<Self>,
        tokio::sync::oneshot::Receiver<()>,
        std::sync::mpsc::Sender<()>,
    ) {
        let probe = std::sync::Arc::new(Self::new());
        let (reached, received) = tokio::sync::oneshot::channel();
        let (resume, paused) = std::sync::mpsc::channel();
        *probe.pause.lock().unwrap() = Some(TestPause {
            phase,
            reached,
            resume: paused,
        });
        (probe, received, resume)
    }

    fn pause_at(&self, phase: Phase) {
        let pause = {
            let mut pause = self.pause.lock().unwrap();
            if pause.as_ref().is_some_and(|pause| pause.phase == phase) {
                pause.take()
            } else {
                None
            }
        };
        if let Some(pause) = pause {
            let _ = pause.reached.send(());
            // A test panic drops resume; the worker must never be stranded.
            let _ = pause.resume.recv_timeout(std::time::Duration::from_secs(6));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[tokio::test]
    async fn native_domain_shutdown_sqlite_close_association() {
        let root = tempfile::tempdir().unwrap();
        let held = crate::DomainRepository::open(&root.path().join("held")).unwrap();
        let other = crate::DomainRepository::open(&root.path().join("other")).unwrap();
        let normal = crate::DomainRepository::open(&root.path().join("normal")).unwrap();
        let subsequent = crate::DomainRepository::open(&root.path().join("subsequent")).unwrap();
        let (original, reached, resume) = Probe::paused(Phase::SqliteCloseEntered);
        let first = {
            let original = original.clone();
            std::thread::spawn(move || held.drop_observed(&original))
        };
        tokio::time::timeout(std::time::Duration::from_secs(2), reached)
            .await
            .unwrap()
            .unwrap();
        let before = original.snapshot(ShutdownOutcome::ReplyTimedOut);
        assert!(before.sqlite_close_entered_us.is_some());
        assert_eq!(before.connection_drop_finished_us, None);
        assert!(matches!(
            crate::DomainRepository::open(&root.path().join("held")),
            Err(crate::Error::Locked)
        ));

        let second_probe = Arc::new(Probe::new());
        let (finished, completion) = tokio::sync::oneshot::channel();
        let second = {
            let probe = second_probe.clone();
            std::thread::spawn(move || {
                other.drop_observed(&probe);
                SQLITE_CLOSE_PROBE.with(|slot| assert!(slot.borrow().is_none()));
                let snapshot = probe.snapshot(ShutdownOutcome::Complete);
                assert!(snapshot.sqlite_close_entered_us.is_some());
                assert!(snapshot.ownership_drop_finished_us.is_some());
                drop(normal);
                assert_eq!(probe.snapshot(ShutdownOutcome::Complete), snapshot);
                let next = Arc::new(Probe::new());
                subsequent.drop_observed(&next);
                assert!(
                    next.snapshot(ShutdownOutcome::Complete)
                        .sqlite_close_entered_us
                        .is_some()
                );
                assert_eq!(probe.snapshot(ShutdownOutcome::Complete), snapshot);
                SQLITE_CLOSE_PROBE.with(|slot| assert!(slot.borrow().is_none()));
                finished.send(()).unwrap();
            })
        };
        tokio::time::timeout(std::time::Duration::from_secs(2), completion)
            .await
            .unwrap()
            .unwrap();
        second.join().unwrap();
        // The distinct closes completed without releasing the held original.
        assert_eq!(original.snapshot(ShutdownOutcome::ReplyTimedOut), before);
        resume.send(()).unwrap();
        first.join().unwrap();
        crate::DomainRepository::open(&root.path().join("held")).unwrap();

        // These are real SQLite callbacks, including deliberately unavailable
        // observation slots. No marker is manually published by the fixture.
        let connection = rusqlite::Connection::open_in_memory().unwrap();
        let probe = Arc::new(Probe::new());
        let scope = SqliteCloseScope::install(&connection, &probe).unwrap();
        let nested = rusqlite::Connection::open_in_memory().unwrap();
        let nested_probe = Arc::new(Probe::new());
        assert!(SqliteCloseScope::install(&nested, &nested_probe).is_none());
        drop(nested);
        assert_eq!(
            probe
                .snapshot(ShutdownOutcome::Complete)
                .sqlite_close_entered_us,
            None
        );
        assert_eq!(
            nested_probe
                .snapshot(ShutdownOutcome::Complete)
                .sqlite_close_entered_us,
            None
        );
        SQLITE_CLOSE_PROBE.with(|slot| {
            let _borrow = slot.borrow_mut();
            drop(connection);
        });
        assert_eq!(
            probe
                .snapshot(ShutdownOutcome::Complete)
                .sqlite_close_entered_us,
            None
        );
        drop(scope);
        SQLITE_CLOSE_PROBE.with(|slot| assert!(slot.borrow().is_none()));

        let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let connection = rusqlite::Connection::open_in_memory().unwrap();
            let _scope = SqliteCloseScope::install(&connection, &probe).unwrap();
            panic!("fixture original close scope unwind");
        }));
        assert!(unwind.is_err());
        assert_eq!(
            probe
                .snapshot(ShutdownOutcome::Complete)
                .sqlite_close_entered_us,
            None
        );
        SQLITE_CLOSE_PROBE.with(|slot| assert!(slot.borrow().is_none()));
        assert_eq!(Arc::strong_count(&probe), 1);
    }

    #[test]
    fn native_domain_shutdown_snapshot() {
        let probe = Arc::new(Probe::new());
        let other = Probe::new();
        let (tx, rx) = std::sync::mpsc::sync_channel(0);
        let worker = {
            let probe = probe.clone();
            std::thread::spawn(move || {
                for phase in [
                    Phase::WorkerPickedUp,
                    Phase::DropStarted,
                    Phase::DropFinished,
                ] {
                    probe.mark(phase);
                    tx.send(()).unwrap();
                }
            })
        };
        for _ in 0..3 {
            rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
            let snapshot = probe.snapshot(ShutdownOutcome::ReplyTimedOut);
            assert!(snapshot.worker_picked_up_us.is_some());
            if let Some(finished) = snapshot.drop_finished_us {
                assert!(finished >= snapshot.drop_started_us.unwrap());
            }
            assert_eq!(snapshot.enqueue_started_us, None);
        }
        worker.join().unwrap();
        let snapshot = probe.snapshot(ShutdownOutcome::ReplyTimedOut);
        assert!(snapshot.drop_finished_us.is_some());
        assert_eq!(snapshot.connection_drop_started_us, None);
        assert_eq!(snapshot.sqlite_close_entered_us, None);
        assert_eq!(snapshot.connection_drop_finished_us, None);
        assert_eq!(snapshot.ownership_drop_started_us, None);
        assert_eq!(snapshot.ownership_drop_finished_us, None);
        assert_eq!(snapshot.native_writer, NativeWriterObservation::Unobserved);
        // Caller enqueue observation can arrive after ALL these worker phases;
        // publishing it must not regress the independently observed worker.
        probe.mark(Phase::EnqueueObserved);
        let later = probe.snapshot(ShutdownOutcome::ReplyTimedOut);
        assert_eq!(later.drop_finished_us, snapshot.drop_finished_us);
        assert!(later.enqueue_observed_us.unwrap() >= later.drop_finished_us.unwrap());
        assert_eq!(
            other
                .snapshot(ShutdownOutcome::ReplyTimedOut)
                .drop_finished_us,
            None
        );
        // Zero elapsed microseconds is a real observation, not our absent
        // sentinel. A timestamp without its completion publication stays absent.
        other.timestamps[Phase::EnqueueStarted as usize].store(0, Ordering::Relaxed);
        assert_eq!(
            other
                .snapshot(ShutdownOutcome::EnqueueTimedOut)
                .enqueue_started_us,
            None
        );
        other.completed.store(1, Ordering::Release);
        assert_eq!(
            other
                .snapshot(ShutdownOutcome::EnqueueTimedOut)
                .enqueue_started_us,
            Some(0)
        );
        // The native enum adds at most 32 bytes to ADR106's224-byte cap.
        // Check the whole Debug projection too: labels and scalar widths are
        // fixed, and maximum values must not evade its independent text bound.
        for timestamp in &other.timestamps {
            timestamp.store(u64::MAX, Ordering::Relaxed);
        }
        other.completed.store(u16::MAX, Ordering::Release);
        let mut maximum = other.snapshot(ShutdownOutcome::EnqueueTimedOut);
        let mut maximum_debug_bytes = 0;
        for native in [
            NativeWriterObservation::Unobserved,
            NativeWriterObservation::Unsupported,
            NativeWriterObservation::Unavailable(NativeWriterUnavailable::DuplicateHandle),
            NativeWriterObservation::Unavailable(NativeWriterUnavailable::BaselineQuery),
            NativeWriterObservation::Unavailable(NativeWriterUnavailable::SnapshotQuery),
            NativeWriterObservation::Unavailable(NativeWriterUnavailable::CounterRegression),
            NativeWriterObservation::Measured {
                process_id: u32::MAX,
                thread_id: u32::MAX,
                kernel_cpu_us: u64::MAX,
                user_cpu_us: u64::MAX,
            },
        ] {
            maximum.native_writer = native;
            assert!(std::mem::size_of_val(&maximum) <= 256);
            maximum_debug_bytes = maximum_debug_bytes.max(format!("{maximum:?}").len());
        }
        assert!(maximum_debug_bytes <= 2048);
        eprintln!(
            "native shutdown snapshot bounds: memory_bytes={} maximum_debug_bytes={maximum_debug_bytes}",
            std::mem::size_of::<ShutdownSnapshot>()
        );
    }
}
