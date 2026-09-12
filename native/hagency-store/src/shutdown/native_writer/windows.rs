//! A real query-only thread handle stays bound to its original writer after exit.
use super::{NativeWriterObservation, NativeWriterUnavailable};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::ptr;
use windows_sys::Win32::{
    Foundation::{DuplicateHandle, FILETIME},
    System::Threading::{
        GetCurrentProcess, GetCurrentProcessId, GetCurrentThread, GetCurrentThreadId,
        GetThreadTimes, THREAD_QUERY_LIMITED_INFORMATION,
    },
};

pub(super) struct Writer {
    handle: OwnedHandle,
    process_id: u32,
    thread_id: u32,
    baseline: Times,
}

#[derive(Clone, Copy)]
struct Times {
    kernel: u64,
    user: u64,
}

fn times(handle: &OwnedHandle) -> Option<Times> {
    let mut creation = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    // SAFETY: The real owned handle stays live through this query. All four
    // outputs are initialized FILETIMEs. No raw pointer escapes; creation/exit
    // values (including undefined exit data for a live thread) are never read.
    if unsafe {
        GetThreadTimes(
            handle.as_raw_handle(),
            &mut creation,
            &mut exit,
            &mut kernel,
            &mut user,
        )
    } == 0
    {
        return None;
    }
    let ticks =
        |time: FILETIME| (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime);
    Some(Times {
        kernel: ticks(kernel),
        user: ticks(user),
    })
}

impl Writer {
    pub(super) fn capture() -> Result<Self, NativeWriterUnavailable> {
        let mut raw = ptr::null_mut();
        // SAFETY: Current process/thread pseudo handles are resolved while still
        // on that writer. DuplicateHandle returns one non-inheritable real handle
        // with query-limited rights only; OwnedHandle takes sole close custody.
        let (process_id, thread_id) = unsafe {
            if DuplicateHandle(
                GetCurrentProcess(),
                GetCurrentThread(),
                GetCurrentProcess(),
                &mut raw,
                THREAD_QUERY_LIMITED_INFORMATION,
                0,
                0,
            ) == 0
            {
                return Err(NativeWriterUnavailable::DuplicateHandle);
            }
            (GetCurrentProcessId(), GetCurrentThreadId())
        };
        // SAFETY: Successful DuplicateHandle above returned this newly owned,
        // non-null real handle. No other owner or numeric-TID reopen is used.
        let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
        let baseline = times(&handle).ok_or(NativeWriterUnavailable::BaselineQuery)?;
        Ok(Self {
            handle,
            process_id,
            thread_id,
            baseline,
        })
    }

    pub(super) fn snapshot(&self) -> NativeWriterObservation {
        let Some(current) = times(&self.handle) else {
            return NativeWriterObservation::Unavailable(NativeWriterUnavailable::SnapshotQuery);
        };
        let (Some(kernel), Some(user)) = (
            current.kernel.checked_sub(self.baseline.kernel),
            current.user.checked_sub(self.baseline.user),
        ) else {
            return NativeWriterObservation::Unavailable(
                NativeWriterUnavailable::CounterRegression,
            );
        };
        // GetThreadTimes uses 100 ns ticks. Subtract before flooring to us.
        NativeWriterObservation::Measured {
            process_id: self.process_id,
            thread_id: self.thread_id,
            kernel_cpu_us: kernel / 10,
            user_cpu_us: user / 10,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, mpsc};
    use windows_sys::Win32::System::Threading::CreateEventW;

    #[test]
    fn native_domain_writer_thread_observation() {
        let caller = Writer::capture().unwrap();
        let (send, receive) = mpsc::sync_channel(1);
        let (release, held) = mpsc::sync_channel(1);
        let thread = std::thread::spawn(move || {
            let writer = Arc::new(super::super::Writer::capture());
            send.send(writer).unwrap();
            held.recv_timeout(std::time::Duration::from_secs(6))
                .unwrap();
        });
        let writer = receive
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        let original = writer.original.as_ref().unwrap();
        assert_eq!(original.process_id, caller.process_id);
        assert_ne!(original.thread_id, caller.thread_id);
        let frozen = writer.snapshot();
        assert!(matches!(frozen, NativeWriterObservation::Measured {
            process_id, thread_id, ..
        } if process_id == original.process_id && thread_id == original.thread_id));
        release.send(()).unwrap();
        thread.join().unwrap();
        // The same retained native handle remains queryable after actual exit.
        let after = times(&original.handle).unwrap();
        assert!(after.kernel >= original.baseline.kernel);
        assert!(after.user >= original.baseline.user);
        assert_eq!(writer.snapshot(), frozen);

        // An actual wrong-kind native object makes GetThreadTimes unavailable.
        // SAFETY: CreateEventW creates one unnamed, non-inherited event without
        // changing any thread; a successful handle passes to its sole RAII owner.
        let raw = unsafe { CreateEventW(ptr::null(), 0, 0, ptr::null()) };
        assert!(!raw.is_null());
        // SAFETY: raw is the fresh event handle just returned above.
        let event = unsafe { OwnedHandle::from_raw_handle(raw) };
        let mut unavailable = super::super::Writer {
            original: Ok(Writer {
                handle: event,
                process_id: caller.process_id,
                thread_id: caller.thread_id,
                baseline: Times { kernel: 0, user: 0 },
            }),
            snapshot: std::sync::OnceLock::new(),
        };
        assert_eq!(
            unavailable.snapshot(),
            NativeWriterObservation::Unavailable(NativeWriterUnavailable::SnapshotQuery)
        );
        // A later valid object cannot retry or upgrade that original failure.
        unavailable.original = Ok(caller);
        assert_eq!(
            unavailable.snapshot(),
            NativeWriterObservation::Unavailable(NativeWriterUnavailable::SnapshotQuery)
        );
    }
}
