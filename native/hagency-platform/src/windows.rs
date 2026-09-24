//! The only FFI boundary in this crate. Windows 10+ JOB_LIST joins the kill-on-close
//! job during CreateProcess, without a suspended-but-unowned crash window.
use crate::{Launch, StopReport, invalid};
use std::{
    ffi::OsStr,
    io,
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    ptr,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{HANDLE_FLAG_INHERIT, SetHandleInformation, WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::{JobObjects::*, Threading::*},
};
pub(crate) mod stdio;

fn wide(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(Some(0)).collect()
}
fn quoted(value: &OsStr) -> Vec<u16> {
    let mut out = vec![34];
    let mut slashes = 0;
    for ch in value.encode_wide() {
        if ch == 92 {
            slashes += 1;
            continue;
        }
        out.extend(std::iter::repeat_n(
            92,
            if ch == 34 { slashes * 2 + 1 } else { slashes },
        ));
        out.push(ch);
        slashes = 0;
    }
    out.extend(std::iter::repeat_n(92, slashes * 2));
    out.push(34);
    out
}
struct Attributes {
    storage: Vec<usize>,
}
impl Attributes {
    fn pointer(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.storage.as_mut_ptr().cast()
    }
    fn new(count: u32) -> io::Result<Self> {
        let mut bytes = 0;
        // SAFETY: The first documented sizing call has a null list and valid
        // size output. The second uses pointer-aligned owned storage of that size.
        unsafe {
            InitializeProcThreadAttributeList(ptr::null_mut(), count, 0, &mut bytes);
        }
        if bytes == 0 || bytes > 65536 {
            return Err(io::Error::last_os_error());
        }
        let mut result = Self {
            storage: vec![0; bytes.div_ceil(size_of::<usize>())],
        };
        // SAFETY: Its allocation remains alive until DeleteProcThreadAttributeList.
        if unsafe { InitializeProcThreadAttributeList(result.pointer(), count, 0, &mut bytes) } == 0
        {
            // An uninitialized attribute list must not enter the normal Drop path.
            let error = io::Error::last_os_error();
            result.storage.clear();
            return Err(error);
        }
        Ok(result)
    }
}
impl Drop for Attributes {
    fn drop(&mut self) {
        if !self.storage.is_empty() {
            // SAFETY: Exactly one successful initialization owns this allocation.
            unsafe {
                DeleteProcThreadAttributeList(self.pointer());
            }
        }
    }
}
pub(super) struct Process {
    job: OwnedHandle,
    child: OwnedHandle,
    pid: u32,
}
impl Process {
    pub(super) fn spawn(launch: &Launch) -> io::Result<Self> {
        Self::spawn_inner(launch, None)
    }
    pub(super) fn spawn_piped(launch: &Launch, pipes: stdio::ChildPipes) -> io::Result<Self> {
        Self::spawn_inner(launch, Some(pipes))
    }
    fn spawn_inner(launch: &Launch, pipes: Option<stdio::ChildPipes>) -> io::Result<Self> {
        if !launch.executable.extension().is_some_and(|v| {
            v.to_str()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
        }) {
            return Err(invalid());
        }
        let executable = wide(launch.executable.as_os_str());
        let directory = wide(launch.directory.as_os_str());
        let mut command = quoted(launch.executable.as_os_str());
        for argument in &launch.arguments {
            command.push(32);
            command.extend(quoted(argument));
        }
        command.push(0);
        if command.len() > 32767 {
            return Err(invalid());
        }
        let mut sorted = std::collections::BTreeMap::new();
        for (key, value) in &launch.environment {
            let name = key.to_str().ok_or_else(invalid)?.to_ascii_uppercase();
            if sorted.insert(name, (key, value)).is_some() {
                return Err(invalid());
            }
        }
        let mut environment = Vec::new();
        for (key, value) in sorted.values() {
            environment.extend(key.encode_wide());
            environment.push(61);
            environment.extend(value.encode_wide());
            environment.push(0);
        }
        if environment.is_empty() {
            environment.push(0);
        }
        environment.push(0);
        // SAFETY: Non-inheritable unnamed job; successful handles are immediately
        // transferred to OwnedHandle. Initialized structs remain valid for calls.
        let job = unsafe {
            let raw = CreateJobObjectW(ptr::null(), ptr::null());
            if raw.is_null() {
                return Err(io::Error::last_os_error());
            }
            OwnedHandle::from_raw_handle(raw)
        };
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: Correct information class, initialized struct and exact size.
        if unsafe {
            SetInformationJobObject(
                job.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of_val(&limits) as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let jobs = [job.as_raw_handle()];
        let handles = pipes.as_ref().map(|pipes| {
            [
                pipes.stdin.as_raw_handle(),
                pipes.stdout.as_raw_handle(),
                pipes.stderr.as_raw_handle(),
            ]
        });
        let mut attributes = Attributes::new(if handles.is_some() { 2 } else { 1 })?;
        // SAFETY: The handle array and the referenced job outlive both the
        // attribute list and CreateProcess. No breakaway limit is enabled.
        if unsafe {
            UpdateProcThreadAttribute(
                attributes.pointer(),
                0,
                PROC_THREAD_ATTRIBUTE_JOB_LIST as usize,
                jobs.as_ptr().cast(),
                size_of_val(&jobs),
                ptr::null_mut(),
                ptr::null(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let mut startup = STARTUPINFOEXW::default();
        startup.StartupInfo.cb = size_of_val(&startup) as u32;
        startup.lpAttributeList = attributes.pointer();
        if let Some(handles) = &handles {
            for &handle in handles {
                // SAFETY: Only these three retained child endpoints become
                // inheritable. HANDLE_LIST below excludes every unrelated handle.
                if unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) }
                    == 0
                {
                    return Err(io::Error::last_os_error());
                }
            }
            // SAFETY: This fixed array and every endpoint outlive the attribute
            // list. JOB_LIST and HANDLE_LIST are applied by the same creation.
            if unsafe {
                UpdateProcThreadAttribute(
                    attributes.pointer(),
                    0,
                    PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                    handles.as_ptr().cast(),
                    size_of_val(handles),
                    ptr::null_mut(),
                    ptr::null(),
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            startup.StartupInfo.dwFlags |= STARTF_USESTDHANDLES;
            startup.StartupInfo.hStdInput = handles[0];
            startup.StartupInfo.hStdOutput = handles[1];
            startup.StartupInfo.hStdError = handles[2];
        }
        let mut info = PROCESS_INFORMATION::default();
        // SAFETY: Every string is NUL-terminated and validated; command is mutable
        // as required. The explicit environment is double-NUL-terminated. Handles
        // are inherited only through the exact list; JOB_LIST establishes ownership before
        // user code executes. Returned handles immediately gain RAII ownership.
        if unsafe {
            CreateProcessW(
                executable.as_ptr(),
                command.as_mut_ptr(),
                ptr::null(),
                ptr::null(),
                i32::from(handles.is_some()),
                EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT | CREATE_NO_WINDOW,
                environment.as_ptr().cast(),
                directory.as_ptr(),
                &startup.StartupInfo,
                &mut info,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: CreateProcess succeeded and returned two owned non-null handles.
        let (child, _thread) = unsafe {
            (
                OwnedHandle::from_raw_handle(info.hProcess),
                OwnedHandle::from_raw_handle(info.hThread),
            )
        };
        Ok(Self {
            job,
            child,
            pid: info.dwProcessId,
        })
    }
    pub(super) fn id(&self) -> u32 {
        self.pid
    }
    pub(super) fn is_leader_running(&self) -> io::Result<bool> {
        // SAFETY: Live retained process handle, nonblocking observation only.
        match unsafe { WaitForSingleObject(self.child.as_raw_handle(), 0) } {
            WAIT_TIMEOUT => Ok(true),
            WAIT_OBJECT_0 => Ok(false),
            _ => Err(io::Error::last_os_error()),
        }
    }
    pub(super) fn stop(&mut self, timeout: Duration) -> io::Result<StopReport> {
        // SAFETY: A retained job handle targets this job, never a recycled PID.
        if unsafe { TerminateJobObject(self.job.as_raw_handle(), 125) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let until = Instant::now() + timeout;
        loop {
            let mut info = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
            // SAFETY: Correct output struct/class/size, live owned job handle.
            if unsafe {
                QueryInformationJobObject(
                    self.job.as_raw_handle(),
                    JobObjectBasicAccountingInformation,
                    (&mut info as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                    size_of_val(&info) as u32,
                    ptr::null_mut(),
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: A process handle remains valid after exit; zero wait never blocks.
            let leader_exited =
                unsafe { WaitForSingleObject(self.child.as_raw_handle(), 0) } == WAIT_OBJECT_0;
            let report = StopReport {
                leader_exited,
                signals_accepted: true,
                whole_tree_stopped: info.ActiveProcesses == 0 && leader_exited,
            };
            if report.whole_tree_stopped || Instant::now() >= until {
                return Ok(report);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
