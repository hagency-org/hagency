//! Linux-only guardian custody. Kernel waitability creates authority; /proc is
//! merely bounded discovery. No public PID constructor and no numeric signals.
use rustix::process::{
    Pid, PidfdFlags, Signal, WaitId, WaitIdOptions, child_subreaper, getpid, pidfd_open,
    pidfd_send_signal, set_child_subreaper, waitid,
};
use std::{
    fs::File,
    io::{self, Read},
    marker::PhantomData,
    os::fd::{AsFd, BorrowedFd},
    rc::Rc,
};

pub(super) struct Reaper {
    _same_thread: PhantomData<Rc<()>>,
}
fn options() -> WaitIdOptions {
    // Include clone children whose exit signal is not SIGCHLD. Omitting __WALL
    // could make ECHILD falsely exclude an owned native clone descendant.
    WaitIdOptions::EXITED
        | WaitIdOptions::NOHANG
        | WaitIdOptions::from_bits_retain(libc::__WALL as u32)
}
impl Reaper {
    pub(super) fn prepare() -> io::Result<Self> {
        let threads = std::fs::read_dir("/proc/self/task")?
            .take(2)
            .collect::<io::Result<Vec<_>>>()?;
        if threads.len() != 1 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "guardian must exclusively own one native thread",
            ));
        }
        match waitid(WaitId::All, options() | WaitIdOptions::NOWAIT) {
            Err(rustix::io::Errno::CHILD) => {}
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "guardian must have no pre-existing children",
                ));
            }
            Err(error) => return Err(error.into()),
        }
        crate::unix_spawn::retain_child_exits()?;
        // Verify discovery and the P_PIDFD wait operation before admitting work.
        let _ = children()?;
        let self_fd = pidfd_open(getpid(), PidfdFlags::empty())?;
        match waitid(
            WaitId::PidFd(self_fd.as_fd()),
            options() | WaitIdOptions::NOWAIT,
        ) {
            Err(rustix::io::Errno::CHILD) => {}
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "native pidfd wait support unavailable",
                ));
            }
        }
        // rustix 1.1 represents PR_SET_CHILD_SUBREAPER's nonzero flag as Option<Pid>;
        // this enables the attribute on this process, not on the named PID.
        set_child_subreaper(Some(getpid()))?;
        if child_subreaper()?.is_none() {
            return Err(io::Error::other("kernel did not enable guardian custody"));
        }
        Ok(Self {
            _same_thread: PhantomData,
        })
    }
    pub(super) fn step(&mut self, unreaped_root: Option<u32>) -> io::Result<bool> {
        for pid in children()? {
            if Some(pid.as_raw_pid() as u32) == unreaped_root {
                continue;
            }
            let fd = match pidfd_open(pid, PidfdFlags::empty()) {
                Ok(fd) => fd,
                Err(rustix::io::Errno::SRCH) => continue,
                Err(error) => return Err(error.into()),
            };
            reap_owned(fd.as_fd())?;
        }
        // /proc children can omit live entries when another exits. Only this
        // kernel observation, after root reaping, proves that none remain.
        match waitid(WaitId::All, options() | WaitIdOptions::NOWAIT) {
            Err(rustix::io::Errno::CHILD) => Ok(unreaped_root.is_none()),
            Ok(_) => Ok(false),
            Err(error) => Err(error.into()),
        }
    }
}
fn reap_owned(fd: BorrowedFd<'_>) -> io::Result<()> {
    match waitid(WaitId::PidFd(fd), options() | WaitIdOptions::NOWAIT) {
        // A stale discovery PID cannot authorize a signal to a non-child.
        Err(rustix::io::Errno::CHILD) => return Ok(()),
        Err(error) => return Err(error.into()),
        Ok(Some(_)) => {}
        Ok(None) => match pidfd_send_signal(fd, Signal::KILL) {
            Ok(()) | Err(rustix::io::Errno::SRCH) => {}
            Err(error) => return Err(error.into()),
        },
    }
    match waitid(WaitId::PidFd(fd), options()) {
        Ok(_) => Ok(()),
        Err(error) => Err(error.into()),
    }
}
fn children() -> io::Result<Vec<Pid>> {
    let mut bytes = Vec::new();
    const LIMIT: usize = 1024 * 1024;
    File::open("/proc/thread-self/children")?
        .take((LIMIT + 1) as u64)
        .read_to_end(&mut bytes)?;
    // A bounded prefix is sufficient for progress, never for completion. Discard
    // an incomplete final PID and revisit after reaping the first owned batch.
    let end = if bytes.len() > LIMIT {
        bytes[..LIMIT]
            .iter()
            .rposition(|b| b.is_ascii_whitespace())
            .map_or(0, |n| n + 1)
    } else {
        bytes.len()
    };
    let text = std::str::from_utf8(&bytes[..end])
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid native child list"))?;
    text.split_whitespace()
        .take(512)
        .map(|value| {
            value
                .parse::<i32>()
                .ok()
                .filter(|v| *v > 1)
                .and_then(Pid::from_raw)
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "invalid native child identity")
                })
        })
        .collect()
}
