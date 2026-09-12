//! Native libproc ABI. Unique lifetime identity and PID version are read in one
//! kernel observation; version-checked signalling closes the census/kill race.
use super::SignalOutcome;
use std::{ffi::c_void, io, process::Child};

#[repr(C)]
#[derive(Default)]
struct BsdInfo {
    flags: u32,
    status: u32,
    exit_status: u32,
    pid: u32,
    ppid: u32,
    uid: u32,
    gid: u32,
    ruid: u32,
    rgid: u32,
    suid: u32,
    sgid: u32,
    reserved: u32,
    command: [u8; 16],
    name: [u8; 32],
    files: u32,
    group: u32,
    jobs: u32,
    tty: u32,
    tty_group: u32,
    nice: i32,
    start_seconds: u64,
    start_microseconds: u64,
}
#[repr(C)]
#[derive(Default)]
struct UniqueInfo {
    uuid: [u8; 16],
    unique: u64,
    parent_unique: u64,
    version: i32,
    parent_version: i32,
    reserved: [u64; 2],
}
#[repr(C)]
#[derive(Default)]
struct BsdUnique {
    bsd: BsdInfo,
    unique: UniqueInfo,
}
const _: () = assert!(
    size_of::<BsdInfo>() == 136 && size_of::<UniqueInfo>() == 56 && size_of::<BsdUnique>() == 192
);
#[link(name = "System")]
unsafe extern "C" {
    fn proc_pidinfo(pid: i32, flavor: i32, arg: u64, buffer: *mut c_void, bytes: i32) -> i32;
    fn proc_signal_with_audittoken(token: *mut [u32; 8], signal: i32) -> i32;
}
fn observe(pid: u32) -> io::Result<Option<BsdUnique>> {
    let pid = i32::try_from(pid)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid child PID"))?;
    let mut info = BsdUnique::default();
    // SAFETY: Flavor 18 returns the documented 192-byte BSD+unique ABI, including
    // zombies when arg=1. Storage is initialized, correctly aligned and sized.
    let count = unsafe {
        proc_pidinfo(
            pid,
            18,
            1,
            (&mut info as *mut BsdUnique).cast(),
            size_of::<BsdUnique>() as i32,
        )
    };
    if count == 0 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(3) {
            return Ok(None);
        }
        return Err(error);
    }
    if count as usize != size_of::<BsdUnique>()
        || info.bsd.pid != pid as u32
        || info.unique.unique == 0
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "inconsistent native process identity",
        ));
    }
    Ok(Some(info))
}
pub(super) struct Handle {
    pid: u32,
    birth: u64,
}
impl Handle {
    pub(super) fn capture(child: &Child) -> io::Result<(Self, u64)> {
        let info = observe(child.id())?
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "owned child has exited"))?;
        let birth = info.unique.unique;
        Ok((
            Self {
                pid: child.id(),
                birth,
            },
            birth,
        ))
    }
    pub(super) fn is_current(&self) -> io::Result<bool> {
        Ok(observe(self.pid)?
            .is_some_and(|info| info.unique.unique == self.birth && info.bsd.status != 5))
    }
    pub(super) fn terminate(&self) -> io::Result<SignalOutcome> {
        let Some(info) = observe(self.pid)? else {
            return Ok(SignalOutcome::NoLongerCurrent);
        };
        if info.unique.unique != self.birth || info.bsd.status == 5 {
            return Ok(SignalOutcome::NoLongerCurrent);
        }
        let mut token = [0u32; 8];
        token[5] = self.pid;
        token[7] = info.unique.version as u32;
        // SAFETY: The kernel reads a correctly sized audit token. Only PID/version
        // identify the target; normal caller signal permissions still apply. A
        // concurrent exec/PID reuse returns ESRCH instead of signalling a new target.
        match unsafe { proc_signal_with_audittoken(&mut token, 9) } {
            0 => Ok(SignalOutcome::Sent),
            3 => Ok(SignalOutcome::NoLongerCurrent),
            error => Err(io::Error::from_raw_os_error(error)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_macos_kernel_rejects_stale_audit_version() {
        let pid = std::process::id();
        let info = observe(pid).unwrap().unwrap();
        let mut token = [0u32; 8];
        token[5] = pid;
        token[7] = (info.unique.version as u32).wrapping_add(1);
        // SAFETY: Correct buffer size; SIGCONT is harmless for this running test
        // process, even if a kernel regression were to accept the wrong version.
        assert_eq!(unsafe { proc_signal_with_audittoken(&mut token, 19) }, 3);
        token[7] = info.unique.version as u32;
        // SAFETY: Same harmless signal, now to this actual process version.
        assert_eq!(unsafe { proc_signal_with_audittoken(&mut token, 19) }, 0);
    }
}
