//! Child-side descriptor sealing. No allocation, locks, filesystem iteration or
//! descriptor closing between fork and exec. Rust's exec-error pipe must remain
//! usable until exec succeeds, so descriptors are marked CLOEXEC instead.
use std::{io, os::unix::process::CommandExt, process::Command};

/// Only the independent guardian calls this before starting its sole workload.
/// An embedding host may have ignored SIGCHLD; that must not auto-reap identities.
pub(crate) fn retain_child_exits() -> io::Result<()> {
    // SAFETY: Fully initialized native sigaction; the private guardian owns this
    // signal disposition. No handler pointer or borrowed storage outlives the call.
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = libc::SIG_DFL;
        if libc::sigemptyset(&mut action.sa_mask) != 0
            || libc::sigaction(libc::SIGCHLD, &action, std::ptr::null_mut()) != 0
        {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

pub(crate) fn seal(command: &mut Command) {
    // SAFETY: The callback runs only direct native syscalls on initialized stack
    // storage. It captures no state, allocates nothing and takes no locks. Stdio
    // setup precedes this callback; descriptors 0..=2 are intentionally retained.
    unsafe {
        command.pre_exec(seal_child);
    }
}

#[cfg(target_os = "linux")]
fn seal_child() -> io::Result<()> {
    // SAFETY: Scalar arguments to close_range; CLOEXEC does not close Rust's
    // error pipe early. fork has already isolated this child's descriptor table.
    let result = unsafe {
        libc::syscall(
            libc::SYS_close_range,
            3u32,
            u32::MAX,
            libc::CLOSE_RANGE_CLOEXEC,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(target_os = "macos")]
fn seal_child() -> io::Result<()> {
    // This child has only its post-fork thread. Query its actual descriptor table,
    // rather than a parent census that could race with another spawning thread.
    let mut entries = [libc::proc_fdinfo {
        proc_fd: 0,
        proc_fdtype: 0,
    }; 4096];
    // SAFETY: proc_pidinfo's libproc wrapper directly invokes proc_info. Its
    // output uses the native proc_fdinfo ABI in initialized, aligned stack memory.
    let count = unsafe {
        libc::proc_pidinfo(
            libc::getpid(),
            libc::PROC_PIDLISTFDS,
            0,
            entries.as_mut_ptr().cast(),
            size_of_val(&entries) as i32,
        )
    };
    if count <= 0 {
        return Err(io::Error::last_os_error());
    }
    if count as usize >= size_of_val(&entries)
        || !(count as usize).is_multiple_of(size_of::<libc::proc_fdinfo>())
    {
        // A full buffer might have truncated the table. Fail before executing
        // work rather than leave uninspected descriptors inheritable.
        return Err(io::Error::from_raw_os_error(libc::E2BIG));
    }
    for entry in &entries[..count as usize / size_of::<libc::proc_fdinfo>()] {
        if entry.proc_fd < 3 {
            continue;
        }
        // SAFETY: This fd came from this single-threaded child's kernel table.
        // fcntl only changes flags; it neither opens nor closes descriptors.
        if unsafe { libc::fcntl(entry.proc_fd, libc::F_SETFD, libc::FD_CLOEXEC) } == -1 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}
