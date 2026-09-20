//! Narrow macOS ABI: suspended spawn, bounded census, lifetime-checked signals.
use crate::{Launch, stdio::ChildPipes};
use std::{
    ffi::CString,
    io,
    os::{fd::AsRawFd, unix::ffi::OsStrExt},
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug)]
pub(super) struct Snapshot {
    pub pid: i32,
    pub parent_pid: i32,
    /// Session, or zero when the kernel refused to name one. A process enters a
    /// session only by being forked inside it or by creating it, and `setpgid`
    /// never crosses a session, so a session carries the same evidence a group
    /// does one level coarser: it survives a descendant changing only its group.
    pub session: i32,
    /// Process group. A group lives inside exactly one session, and a session is
    /// entered only by fork inheritance or by creating it, so a group never mixes
    /// the owned leader's descendants with unrelated processes.
    pub group: i32,
    /// Resource and jetsam coalition, or zeros when the kernel refused to name
    /// them. A coalition is inherited across fork, exec and `setsid`, and leaving
    /// one takes a spawn attribute only launchd may use. It therefore survives
    /// the one move that defeats session and group evidence: a daemon that opens
    /// its own session through a parent no census ever saw.
    pub coalition: [u64; 2],
    pub birth: u64,
    pub parent_birth: u64,
    pub version: u32,
    pub original_parent_version: u32,
    pub status: u32,
}
#[repr(C)]
#[derive(Default, PartialEq)]
struct Unique {
    uuid: [u8; 16],
    birth: u64,
    parent: u64,
    version: i32,
    parent_version: i32,
    reserved: [u64; 2],
}
#[repr(C)]
#[derive(Default)]
struct Short {
    pid: u32,
    parent: u32,
    group: u32,
    status: u32,
    command: [u8; 16],
    flags: u32,
    uid: u32,
    gid: u32,
    ruid: u32,
    rgid: u32,
    suid: u32,
    sgid: u32,
    reserved: u32,
}
/// `struct proc_pidcoalitioninfo`, <sys/proc_info.h>: one id per coalition type
/// (resource, jetsam), then reserved words.
#[repr(C)]
#[derive(Default)]
struct Coalition {
    ids: [u64; 2],
    reserved: [u64; 3],
}
const _: () =
    assert!(size_of::<Unique>() == 56 && size_of::<Short>() == 64 && size_of::<Coalition>() == 40);
/// PROC_PIDCOALITIONINFO. Like the two flavors below it permits foreign-user
/// reads: measured unprivileged against every live process, root-owned included.
const COALITION_INFO: i32 = 20;
/// <sys/spawn.h>, macOS 10.15 and later; the libc crate does not export it.
const POSIX_SPAWN_SETSID: i32 = 0x0400;
unsafe extern "C" {
    fn proc_signal_with_audittoken(token: *mut [u32; 8], signal: i32) -> i32;
    fn posix_spawn_file_actions_addchdir_np(
        actions: *mut libc::posix_spawn_file_actions_t,
        path: *const libc::c_char,
    ) -> i32;
}
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "native process observation is incomplete",
    )
}
fn code(value: i32) -> io::Result<()> {
    if value == 0 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(value))
    }
}
fn query<T: Default>(pid: i32, flavor: i32) -> io::Result<Option<T>> {
    let mut value = T::default();
    // SAFETY: Only this module calls query with its three initialized repr(C)
    // layouts and their exact native flavors/sizes. The kernel copies into them.
    let n = unsafe {
        libc::proc_pidinfo(
            pid,
            flavor,
            1,
            (&mut value as *mut T).cast(),
            size_of::<T>() as i32,
        )
    };
    if n == 0 {
        let error = io::Error::last_os_error();
        return if error.raw_os_error() == Some(libc::ESRCH) {
            Ok(None)
        } else {
            Err(error)
        };
    }
    if n as usize != size_of::<T>() {
        return Err(invalid());
    }
    Ok(Some(value))
}
pub(super) fn observe(pid: i32) -> io::Result<Option<Snapshot>> {
    if pid <= 0 {
        return Err(invalid());
    }
    // Unlike BSD+unique flavor18, both flavors below permit foreign-user reads.
    // Bracketing prevents combining different PID lifetimes/exec versions.
    let Some(before) = query::<Unique>(pid, 17)? else {
        return Ok(None);
    };
    let Some(short) = query::<Short>(pid, 13)? else {
        return Ok(None);
    };
    // Read inside the identity bracket, so a session can never be combined with
    // a different PID lifetime. Darwin answers for any PID, but a refusal is not
    // evidence about this row and must never end a census: zero means "no
    // session evidence" and classification ignores it.
    // SAFETY: getsid is a pure lookup on a PID; it writes nothing.
    let mut session = unsafe { libc::getsid(pid) };
    // Only -1 sets errno. A zero answer reads no errno, which would otherwise be
    // a stale value from an earlier row and could report a live process gone.
    if session < 0 {
        if io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
            return Ok(None);
        }
        session = 0;
    }
    // The same rule as the session: read inside the bracket, and a refusal is
    // "no coalition evidence" for this row, never a failed census.
    let coalition = match query::<Coalition>(pid, COALITION_INFO) {
        Ok(Some(value)) => value.ids,
        Ok(None) => return Ok(None),
        Err(_) => [0, 0],
    };
    let Some(after) = query::<Unique>(pid, 17)? else {
        return Ok(None);
    };
    if before != after {
        return Err(io::Error::other(
            "native identity changed during observation",
        ));
    }
    if after.birth == 0 || short.pid != pid as u32 {
        return Err(invalid());
    }
    Ok(Some(Snapshot {
        pid,
        parent_pid: short.parent as i32,
        session,
        group: short.group as i32,
        coalition,
        birth: after.birth,
        parent_birth: after.parent,
        version: after.version as u32,
        original_parent_version: after.parent_version as u32,
        status: short.status,
    }))
}
/// A row read while its process is mid-exec, mid-exit or mid-reuse disagrees
/// with itself: the two identity reads differ, or the kernel refuses one. That is
/// one unrelated process at one instant and says nothing about the owned tree,
/// yet it used to fail the whole sweep, which ends the guardian's observation
/// and stops the tree (live 2026-09-20, `census_failed`, thirty soak rounds in;
/// reproduced within a few hundred sweeps under exec churn). Read it again: every
/// attempt is a complete identity bracket, so a returned row is as consistent as
/// before, and a row that still cannot be read ends the census exactly as it did.
const ROW_ATTEMPTS: usize = 4;
fn observe_settled(pid: i32) -> io::Result<Option<Snapshot>> {
    let mut attempt = 1;
    loop {
        match observe(pid) {
            Err(_) if attempt < ROW_ATTEMPTS => {
                attempt += 1;
                std::thread::yield_now();
            }
            result => return result,
        }
    }
}
pub(super) fn census() -> io::Result<Vec<Snapshot>> {
    let mut pids = vec![0i32; 32768];
    // SAFETY: The initialized aligned buffer has exactly the passed byte size.
    // libproc returns a count of PIDs, not bytes. A saturated result is refused.
    let n = unsafe {
        libc::proc_listallpids(
            pids.as_mut_ptr().cast(),
            size_of_val(pids.as_slice()) as i32,
        )
    };
    if n <= 0 {
        return Err(io::Error::last_os_error());
    }
    if n as usize >= pids.len() {
        return Err(invalid());
    }
    let mut result = Vec::new();
    for &pid in &pids[..n as usize] {
        if pid > 0
            && let Some(row) = observe_settled(pid)?
        {
            result.push(row);
        }
    }
    Ok(result)
}
/// Private caller must already own this birth through spawn or proven ancestry.
pub(super) fn signal(target: Snapshot, signal: i32) -> io::Result<bool> {
    let Some(current) = observe(target.pid)? else {
        return Ok(false);
    };
    if current.birth != target.birth || current.status == 5 {
        return Ok(false);
    }
    let mut token = [0u32; 8];
    token[5] = current.pid as u32;
    token[7] = current.version;
    // SAFETY: Correct native audit-token layout. The kernel verifies PID/version
    // at signal time; concurrent exec/PID reuse cannot retarget this signal.
    match unsafe { proc_signal_with_audittoken(&mut token, signal) } {
        0 => Ok(true),
        libc::ESRCH => Ok(false),
        error => Err(io::Error::from_raw_os_error(error)),
    }
}
fn cstring(value: &std::ffi::OsStr) -> io::Result<CString> {
    CString::new(value.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid native launch value"))
}
struct Attributes(libc::posix_spawnattr_t);
impl Drop for Attributes {
    fn drop(&mut self) {
        // SAFETY: This handle was successfully initialized, is exclusively owned and
        // is destroyed once after the synchronous spawn stopped using its storage.
        unsafe {
            libc::posix_spawnattr_destroy(&mut self.0);
        }
    }
}
struct Actions(libc::posix_spawn_file_actions_t);
impl Drop for Actions {
    fn drop(&mut self) {
        // SAFETY: Same initialized one-owner rule as Attributes.
        unsafe {
            libc::posix_spawn_file_actions_destroy(&mut self.0);
        }
    }
}
pub(super) struct Root {
    pub pid: i32,
    reaped: bool,
}
impl Root {
    pub fn cancel_startup(&mut self) -> io::Result<bool> {
        if self.reaped {
            return Ok(true);
        }
        // SAFETY: This is the exclusive unreaped child anchor, never a census PID.
        if unsafe { libc::kill(self.pid, libc::SIGKILL) } != 0 {
            return Err(io::Error::last_os_error());
        }
        let until = Instant::now() + Duration::from_secs(1);
        loop {
            if self.reap()? {
                return Ok(true);
            }
            if Instant::now() >= until {
                return Ok(false);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    pub fn reap(&mut self) -> io::Result<bool> {
        if self.reaped {
            return Ok(true);
        }
        let mut status = 0;
        // SAFETY: Only this object owns this unreaped child, created by spawn.
        // WNOHANG bounds the call; no other child's status can be consumed.
        let n = unsafe { libc::waitpid(self.pid, &mut status, libc::WNOHANG) };
        if n == self.pid {
            self.reaped = true;
            Ok(true)
        } else if n == 0 {
            Ok(false)
        } else {
            Err(io::Error::last_os_error())
        }
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        if !self.reaped {
            let _ = self.cancel_startup();
        }
    }
}
pub(super) fn spawn(launch: &Launch, pipes: Option<ChildPipes>) -> io::Result<Root> {
    launch.validate()?;
    if launch.require_crash_containment {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "guardian crash containment is unavailable",
        ));
    }
    let executable = cstring(launch.executable.as_os_str())?;
    let directory = cstring(launch.directory.as_os_str())?;
    let args = std::iter::once(launch.executable.as_os_str())
        .chain(launch.arguments.iter().map(|s| s.as_os_str()))
        .map(cstring)
        .collect::<io::Result<Vec<_>>>()?;
    let env = launch
        .environment
        .iter()
        .map(|(k, v)| {
            let mut bytes = k.as_bytes().to_vec();
            bytes.push(b'=');
            bytes.extend_from_slice(v.as_bytes());
            CString::new(bytes).map_err(|_| invalid())
        })
        .collect::<io::Result<Vec<_>>>()?;
    let mut argv = args
        .iter()
        .map(|s| s.as_ptr().cast_mut())
        .collect::<Vec<_>>();
    argv.push(std::ptr::null_mut());
    let mut envp = env
        .iter()
        .map(|s| s.as_ptr().cast_mut())
        .collect::<Vec<_>>();
    envp.push(std::ptr::null_mut());
    let null = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/null")?;
    // SAFETY: Opaque handles are initialized before use; failure leaves no live
    // wrapper to destroy. argv/envp are NUL-terminated owned arrays/strings kept
    // alive through synchronous posix_spawn. File descriptors remain owned here.
    unsafe {
        let mut attributes = std::mem::zeroed();
        code(libc::posix_spawnattr_init(&mut attributes))?;
        let mut attributes = Attributes(attributes);
        let mut actions = std::mem::zeroed();
        code(libc::posix_spawn_file_actions_init(&mut actions))?;
        let mut actions = Actions(actions);
        // The leader starts its own session, not merely its own group. Group
        // membership is then sound tracking evidence: no owned process can join
        // an unrelated group, and no unrelated process can join an owned one,
        // because setpgid never crosses a session.
        let flags = libc::POSIX_SPAWN_START_SUSPENDED
            | libc::POSIX_SPAWN_CLOEXEC_DEFAULT
            | POSIX_SPAWN_SETSID
            | libc::POSIX_SPAWN_SETSIGMASK
            | libc::POSIX_SPAWN_SETSIGDEF;
        code(libc::posix_spawnattr_setflags(
            &mut attributes.0,
            flags as i16,
        ))?;
        let mut mask = std::mem::zeroed();
        code(libc::sigemptyset(&mut mask))?;
        code(libc::posix_spawnattr_setsigmask(&mut attributes.0, &mask))?;
        for sig in [libc::SIGPIPE, libc::SIGTERM, libc::SIGINT, libc::SIGCHLD] {
            code(libc::sigaddset(&mut mask, sig))?;
        }
        code(libc::posix_spawnattr_setsigdefault(
            &mut attributes.0,
            &mask,
        ))?;
        code(posix_spawn_file_actions_addchdir_np(
            &mut actions.0,
            directory.as_ptr(),
        ))?;
        let fds = match &pipes {
            Some(p) => [
                p.stdin.as_raw_fd(),
                p.stdout.as_raw_fd(),
                p.stderr.as_raw_fd(),
            ],
            None => [null.as_raw_fd(); 3],
        };
        for (target, fd) in fds.into_iter().enumerate() {
            code(libc::posix_spawn_file_actions_adddup2(
                &mut actions.0,
                fd,
                target as i32,
            ))?;
        }
        let mut pid = 0;
        code(libc::posix_spawn(
            &mut pid,
            executable.as_ptr(),
            &actions.0,
            &attributes.0,
            argv.as_ptr(),
            envp.as_ptr(),
        ))?;
        if pid <= 1 {
            return Err(invalid());
        }
        Ok(Root { pid, reaped: false })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::CommandExt;

    /// Live 2026-09-20: thirty soak rounds in, a guardian stopped its tree with
    /// `census_failed`. No tracker refusal: one unrelated row, read while it was
    /// mid-exec, failed the whole sweep. Short-lived processes that exec are what
    /// a working machine is made of, so this churns them under a tight census.
    #[test]
    fn native_macos_census_survives_exec_churn() {
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let churn: Vec<_> = (0..4)
            .map(|_| {
                let stop = stop.clone();
                std::thread::spawn(move || {
                    while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                        // fork, exec a shell, which execs again, then exits.
                        let _ = std::process::Command::new("/bin/sh")
                            .args(["-c", "exec /usr/bin/true"])
                            .status();
                    }
                })
            })
            .collect();
        let until = Instant::now() + Duration::from_secs(3);
        let mut sweeps = 0u32;
        let mut failure = None;
        while Instant::now() < until {
            match census() {
                Ok(rows) => assert!(rows.len() > 1),
                Err(error) => {
                    failure = Some((sweeps, error.to_string()));
                    break;
                }
            }
            sweeps += 1;
        }
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        for thread in churn {
            thread.join().unwrap();
        }
        assert!(sweeps > 10, "only {sweeps} sweeps completed");
        assert_eq!(failure, None, "a census failed under exec churn");
    }

    /// The two platform facts coalition evidence rests on, read from real
    /// processes: a child that opens its own session keeps its parent's
    /// coalition, and what launchd runs is in another one. If either ever stops
    /// holding, the evidence is unsound and this fails before anything ships.
    #[test]
    fn native_macos_coalition_survives_setsid_and_differs_from_launchd() {
        let own = observe(std::process::id() as i32).unwrap().unwrap();
        assert!(!own.coalition.contains(&0), "{:?}", own.coalition);
        let mut command = std::process::Command::new("/bin/sleep");
        command.arg("30");
        // SAFETY: setsid is async-signal-safe and the closure touches nothing else.
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() < 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let mut child = command.spawn().unwrap();
        let row = observe(child.id() as i32).unwrap().unwrap();
        let launchd = observe(1).unwrap().unwrap();
        child.kill().unwrap();
        child.wait().unwrap();
        assert_ne!(row.session, own.session, "the child leads its own session");
        assert_eq!(row.coalition, own.coalition);
        assert!(
            launchd.coalition[0] != own.coalition[0] && launchd.coalition[1] != own.coalition[1],
            "{:?} {:?}",
            launchd.coalition,
            own.coalition
        );
    }
}
