//! Fixed native thread accounting for an already-observed original domain drop.
use std::sync::OnceLock;

#[cfg(windows)]
#[allow(unsafe_code)] // Query-only owned Win32 thread boundary; no SQLite FFI.
mod windows;

/// Diagnostic availability only; none of these states changes a shutdown result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeWriterUnavailable {
    DuplicateHandle,
    BaselineQuery,
    SnapshotQuery,
    CounterRegression,
}

/// Fixed native identity and CPU time from domain-drop entry to first snapshot.
/// CPU time cannot distinguish IO, mutex waits, sleeping or descheduling.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeWriterObservation {
    Unobserved,
    Unsupported,
    Unavailable(NativeWriterUnavailable),
    Measured {
        process_id: u32,
        thread_id: u32,
        kernel_cpu_us: u64,
        user_cpu_us: u64,
    },
}

pub(super) struct Writer {
    #[cfg(windows)]
    original: Result<windows::Writer, NativeWriterUnavailable>,
    snapshot: OnceLock<NativeWriterObservation>,
}

impl Writer {
    pub(super) fn capture() -> Self {
        Self {
            #[cfg(windows)]
            original: windows::Writer::capture(),
            snapshot: OnceLock::new(),
        }
    }

    pub(super) fn snapshot(&self) -> NativeWriterObservation {
        // Freeze the first original measurement, including unavailable results.
        // Later phase snapshots must not charge subsequent work on this thread.
        *self.snapshot.get_or_init(|| {
            #[cfg(windows)]
            {
                match &self.original {
                    Ok(writer) => writer.snapshot(),
                    Err(error) => NativeWriterObservation::Unavailable(*error),
                }
            }
            #[cfg(not(windows))]
            NativeWriterObservation::Unsupported
        })
    }
}

#[cfg(all(test, not(windows)))]
mod tests {
    use super::*;

    #[test]
    fn native_domain_writer_thread_observation() {
        let writer = Writer::capture();
        assert_eq!(writer.snapshot(), NativeWriterObservation::Unsupported);
        assert_eq!(writer.snapshot(), NativeWriterObservation::Unsupported);
    }
}
