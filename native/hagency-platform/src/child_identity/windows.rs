use super::SignalOutcome;
use std::{
    io,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    process::Child,
    ptr,
};
use windows_sys::Win32::{
    Foundation::{DUPLICATE_SAME_ACCESS, DuplicateHandle, FILETIME, WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::Threading::{
        GetCurrentProcess, GetProcessTimes, TerminateProcess, WaitForSingleObject,
    },
};

pub(super) struct Handle {
    handle: OwnedHandle,
}
impl Handle {
    pub(super) fn capture(child: &Child) -> io::Result<(Self, u64)> {
        let mut duplicate = ptr::null_mut();
        // SAFETY: Child owns a valid process handle. DuplicateHandle creates one
        // non-inheritable independent reference, immediately transferred to RAII.
        let handle = unsafe {
            if DuplicateHandle(
                GetCurrentProcess(),
                child.as_raw_handle(),
                GetCurrentProcess(),
                &mut duplicate,
                0,
                0,
                DUPLICATE_SAME_ACCESS,
            ) == 0
            {
                return Err(io::Error::last_os_error());
            }
            OwnedHandle::from_raw_handle(duplicate)
        };
        let mut created = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        // SAFETY: Valid retained process and four distinct initialized FILETIMEs.
        if unsafe {
            GetProcessTimes(
                handle.as_raw_handle(),
                &mut created,
                &mut exit,
                &mut kernel,
                &mut user,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let birth = (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime);
        Ok((Self { handle }, birth))
    }
    pub(super) fn is_current(&self) -> io::Result<bool> {
        // SAFETY: A retained handle refers to the same process even after PID reuse.
        match unsafe { WaitForSingleObject(self.handle.as_raw_handle(), 0) } {
            WAIT_TIMEOUT => Ok(true),
            WAIT_OBJECT_0 => Ok(false),
            _ => Err(io::Error::last_os_error()),
        }
    }
    pub(super) fn terminate(&self) -> io::Result<SignalOutcome> {
        if !self.is_current()? {
            return Ok(SignalOutcome::NoLongerCurrent);
        }
        // SAFETY: Only the retained child process handle supplies signal authority.
        if unsafe { TerminateProcess(self.handle.as_raw_handle(), 125) } == 0 {
            let error = io::Error::last_os_error();
            if !self.is_current()? {
                return Ok(SignalOutcome::NoLongerCurrent);
            }
            return Err(error);
        }
        Ok(SignalOutcome::Sent)
    }
}
