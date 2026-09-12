//! Optional Linux recovery capability. This is not a kill-on-close resource.
use crate::{
    StopReport,
    cgroup_checks::{
        NamespaceKind, full_mount, host_status, kernel_release, namespace_identity,
        namespace_refused, populated, refuse,
    },
};
use rustix::{
    fs::{
        AtFlags, Mode, OFlags, Stat, StatxFlags, fcntl_getfl, fstat, fstatfs, openat, statat, statx,
    },
    io::{FdFlags, fcntl_getfd},
    process::{DumpableBehavior, dumpable_behavior, set_dumpable_behavior},
};
use std::{
    fs::File,
    io::{self, Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::fs::FileExt,
    },
    process::Child,
    time::{Duration, Instant},
};

const CGROUP2: i64 = 0x6367_7270;
const PROC: i64 = 0x9fa0;

/// A provisioned, exclusive cgroup used only to recover a lost guardian while
/// its host survives. No constructor from runner JSON, PID, or path is exposed.
///
/// The privileged host provisioner must open the directory and two write files
/// with CLOEXEC, exclusively reserve this empty domain subtree, and retain that
/// reservation until actual cleanup is observed. It must not duplicate migration
/// capabilities to workspace code, reassign unrelated work, change permissions,
/// move protected processes, or remove/reuse the subtree during this lifetime.
/// These privileged actions cannot be excluded by inspection of three FDs.
///
/// The calling host must already be nonroot, nondumpable, no-new-privileges and
/// empty in all five capability sets (including the bounding set). It must keep
/// those conditions and exclusive child reaping ownership for the whole scope.
/// This API does not configure the host or provide simultaneous host/guardian
/// crash protection. `Launch::require_crash_containment` still fails on POSIX.
pub struct CgroupRecovery {
    directory: OwnedFd,
    procs: File,
    kill: File,
    events: File,
    armed: bool,
    signal_failed: bool,
}

impl CgroupRecovery {
    pub fn from_host_files(
        directory: OwnedFd,
        procs_write: OwnedFd,
        kill_write: OwnedFd,
    ) -> io::Result<Self> {
        validate_namespaces()?;
        let directory_stat = protected(&directory, true)?;
        validate_control(&directory, &procs_write, "cgroup.procs", &directory_stat)?;
        validate_control(&directory, &kill_write, "cgroup.kill", &directory_stat)?;
        validate_ancestors(&directory)?;
        validate_host()?;
        let kind = read_control(&directory, "cgroup.type", 32)?;
        if kind != b"domain\n" {
            return Err(refuse());
        }
        let events = File::from(openat(
            &directory,
            "cgroup.events",
            OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::empty(),
        )?);
        protected_file(&events)?;
        let result = Self {
            directory,
            procs: procs_write.into(),
            kill: kill_write.into(),
            events,
            armed: false,
            signal_failed: false,
        };
        if result.populated()? {
            return Err(refuse());
        }
        Ok(result)
    }

    /// The only PID write is migration of our retained unreaped guardian, which
    /// is blocked on Prepare and cannot yet have forked workspace code. It is
    /// never accepted from a census, caller metadata or a runner message.
    pub(crate) fn attach_before_prepare(&mut self, guardian: &Child) -> io::Result<()> {
        if self.armed {
            return Err(refuse());
        }
        validate_host()?;
        validate_ancestors(&self.directory)?;
        if self.populated()? {
            return Err(refuse());
        }
        self.armed = true; // A failed/partial syscall has an unresolved side effect.
        let value = guardian.id().to_string();
        if guardian.id() <= 1 || self.procs.write(value.as_bytes())? != value.len() {
            return Err(refuse());
        }
        Ok(())
    }

    fn populated(&self) -> io::Result<bool> {
        let mut data = [0u8; 4097];
        let count = self.events.read_at(&mut data, 0)?;
        populated(&data[..count])
    }

    pub(crate) fn remember_signal_failure(&mut self) {
        self.signal_failed = true;
    }

    pub(crate) fn stop(&mut self, until: Instant) -> io::Result<StopReport> {
        if !self.armed {
            return Err(refuse());
        }
        if Instant::now() >= until {
            return Err(unknown());
        }
        // A single kernfs operation; do not replay a short or failed write here.
        match self.kill.write(b"1") {
            Ok(1) => {}
            Ok(_) => {
                self.signal_failed = true;
                return Err(unknown());
            }
            Err(error) => {
                self.signal_failed = true;
                return Err(error);
            }
        }
        loop {
            if Instant::now() >= until {
                return Err(unknown());
            }
            if !self.populated()? {
                self.armed = false;
                return Ok(StopReport {
                    leader_exited: true,
                    signals_accepted: !self.signal_failed,
                    // No live execution remains anywhere in this protected
                    // subtree. This is not proof that adopted zombies are reaped.
                    whole_tree_stopped: true,
                });
            }
            std::thread::sleep(
                Duration::from_millis(5).min(until.saturating_duration_since(Instant::now())),
            );
        }
    }
}

impl Drop for CgroupRecovery {
    fn drop(&mut self) {
        if self.armed {
            let _ = self.stop(Instant::now() + Duration::from_secs(2));
        }
        // Closing these descriptors does NOT itself stop any process. A failed
        // bounded stop remains unresolved and needs the provisioner's inspection.
    }
}

fn unknown() -> io::Error {
    io::Error::new(io::ErrorKind::TimedOut, "cgroup cleanup outcome is unknown")
}

fn protected(fd: &impl std::os::fd::AsFd, directory: bool) -> io::Result<Stat> {
    let value = fstat(fd)?;
    if fstatfs(fd)?.f_type as i64 != CGROUP2
        || value.st_uid != 0
        || value.st_mode & 0o022 != 0
        || value.st_mode & libc::S_IFMT
            != if directory {
                libc::S_IFDIR
            } else {
                libc::S_IFREG
            }
        || !fcntl_getfd(fd)?.contains(FdFlags::CLOEXEC)
    {
        return Err(refuse());
    }
    Ok(value)
}
fn protected_file(fd: &impl std::os::fd::AsFd) -> io::Result<Stat> {
    protected(fd, false)
}

fn validate_control(
    directory: &OwnedFd,
    control: &OwnedFd,
    name: &str,
    parent: &Stat,
) -> io::Result<()> {
    let value = protected_file(control)?;
    let expected = statat(directory, name, AtFlags::SYMLINK_NOFOLLOW)?;
    let flags = fcntl_getfl(control)?;
    if value.st_dev != parent.st_dev
        || value.st_dev != expected.st_dev
        || value.st_ino != expected.st_ino
        || flags & OFlags::ACCMODE != OFlags::WRONLY
        || flags.intersects(OFlags::PATH | OFlags::APPEND)
    {
        return Err(refuse());
    }
    Ok(())
}

fn read_control(directory: &OwnedFd, name: &str, limit: usize) -> io::Result<Vec<u8>> {
    let file = File::from(openat(
        directory,
        name,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )?);
    protected_file(&file)?;
    bounded(file, limit)
}

fn bounded(file: File, limit: usize) -> io::Result<Vec<u8>> {
    let mut data = Vec::new();
    file.take((limit + 1) as u64).read_to_end(&mut data)?;
    if data.len() > limit {
        return Err(refuse());
    }
    Ok(data)
}

fn proc_read(path: &str, limit: usize) -> io::Result<Vec<u8>> {
    let file = File::open(path)?;
    if fstatfs(&file)?.f_type as i64 != PROC {
        return Err(refuse());
    }
    bounded(file, limit)
}

fn fd_mount(fd: &OwnedFd) -> io::Result<u64> {
    // fdinfo directories may be root-owned after nondumpability is set. Read
    // identity from the owned descriptor itself, never relax host dumpability.
    let value = statx(fd, "", AtFlags::EMPTY_PATH, StatxFlags::MNT_ID)?;
    if value.stx_mask & StatxFlags::MNT_ID.bits() == 0 || value.stx_mnt_id == 0 {
        return Err(refuse());
    }
    Ok(value.stx_mnt_id)
}

fn validate_ancestors(directory: &OwnedFd) -> io::Result<()> {
    let id = fd_mount(directory)?;
    full_mount(&proc_read("/proc/thread-self/mountinfo", 256 * 1024)?, id)?;
    let mut cursor = rustix::io::fcntl_dupfd_cloexec(directory, 3)?;
    for _ in 0..64 {
        protected(&cursor, true)?;
        // Protect the common ancestor migration authority, not merely the leaf.
        for name in ["cgroup.procs", "cgroup.threads"] {
            let control = openat(
                &cursor,
                name,
                OFlags::PATH | OFlags::CLOEXEC | OFlags::NOFOLLOW,
                Mode::empty(),
            )?;
            protected_file(&control)?;
        }
        let parent = openat(
            &cursor,
            "..",
            OFlags::PATH | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::empty(),
        )?;
        if fd_mount(&parent)? != id {
            return Ok(());
        }
        let a = fstat(&cursor)?;
        let b = fstat(&parent)?;
        if a.st_dev == b.st_dev && a.st_ino == b.st_ino {
            return Err(refuse());
        }
        cursor = parent;
    }
    Err(refuse())
}

pub(crate) fn validate_host() -> io::Result<()> {
    validate_namespaces()?;
    host_status(&proc_read("/proc/thread-self/status", 64 * 1024)?)?;
    if dumpable_behavior()? != DumpableBehavior::NotDumpable {
        return Err(refuse());
    }
    // Migration uses the retained Child PID only before its first Prepare. No
    // SA_NOCLDWAIT/ignored SIGCHLD may silently reap that identity meanwhile.
    // SAFETY: Reads the process disposition into initialized native ABI storage.
    let action = unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        if libc::sigaction(libc::SIGCHLD, std::ptr::null(), &mut action) != 0 {
            return Err(io::Error::last_os_error());
        }
        action
    };
    if action.sa_sigaction != libc::SIG_DFL || action.sa_flags & libc::SA_NOCLDWAIT != 0 {
        return Err(refuse());
    }
    Ok(())
}

pub(crate) fn prepare_guardian() -> io::Result<()> {
    // exec resets dumpability; this happens in the disposable trusted guardian
    // before Prepare is acknowledged and before any workspace process exists.
    set_dumpable_behavior(DumpableBehavior::NotDumpable)?;
    validate_host()
}

fn namespace_metadata(fd: &OwnedFd, kind: NamespaceKind) -> io::Result<()> {
    // SAFETY: NS_GET_NSTYPE takes no pointer and returns the kernel namespace
    // type of this internally opened descriptor, never a model-supplied handle.
    let ty = unsafe { libc::ioctl(fd.as_raw_fd(), libc::NS_GET_NSTYPE) };
    if ty < 0 {
        return Err(namespace_refused());
    }
    namespace_identity(kind, fstatfs(fd)?.f_type as u64, fstat(fd)?.st_ino, ty)
}

fn validate_namespaces() -> io::Result<()> {
    // SAFETY: uname initializes the supplied correctly aligned utsname; inspect
    // only its fixed release array after success and require an in-array NUL.
    let mut name = std::mem::MaybeUninit::<libc::utsname>::uninit();
    if unsafe { libc::uname(name.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let name = unsafe { name.assume_init() };
    let end = name
        .release
        .iter()
        .position(|v| *v == 0)
        .ok_or_else(namespace_refused)?;
    let release: Vec<u8> = name.release[..end].iter().map(|v| *v as u8).collect();
    kernel_release(&release)?;
    // O_PATH needs no directory read permission after nondumpability. Anchor
    // source entries to actual current-thread procfs, not supplied namespace FDs
    // or /proc/1 (PID 1 may itself be inside a namespace).
    let proc_root = openat(
        rustix::fs::CWD,
        "/proc",
        OFlags::PATH | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )?;
    if fstatfs(&proc_root)?.f_type as i64 != PROC {
        return Err(namespace_refused());
    }
    let proc_mount = fd_mount(&proc_root)?;
    let thread = openat(
        &proc_root,
        "thread-self",
        OFlags::PATH | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
    )?;
    let namespaces = openat(
        &thread,
        "ns",
        OFlags::PATH | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )?;
    for fd in [&thread, &namespaces] {
        if fstatfs(fd)?.f_type as i64 != PROC || fd_mount(fd)? != proc_mount {
            return Err(namespace_refused());
        }
    }
    let mut descriptors = Vec::with_capacity(2);
    for (name, kind) in [
        ("user", NamespaceKind::User),
        ("cgroup", NamespaceKind::Cgroup),
    ] {
        let source = openat(
            &namespaces,
            name,
            OFlags::PATH | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::empty(),
        )?;
        if fstatfs(&source)?.f_type as i64 != PROC
            || fd_mount(&source)? != proc_mount
            || fstat(&source)?.st_mode & libc::S_IFMT != libc::S_IFLNK
        {
            return Err(namespace_refused());
        }
        let fd = openat(
            &namespaces,
            name,
            OFlags::RDONLY | OFlags::CLOEXEC,
            Mode::empty(),
        )?;
        namespace_metadata(&fd, kind)?;
        descriptors.push(fd);
    }
    // ns_get_owner only returns the initial cgroup's user owner when the calling
    // thread is itself in initial user namespace. A nested caller cannot pass an
    // accessible initial namespace descriptor in place of its actual context.
    // SAFETY: No pointer. A nonnegative result is a fresh CLOEXEC FD from kernel.
    let owner = unsafe { libc::ioctl(descriptors[1].as_raw_fd(), libc::NS_GET_USERNS) };
    if owner < 0 {
        return Err(namespace_refused());
    }
    // SAFETY: Adopt the newly returned descriptor exactly once; RAII closes it.
    let owner = unsafe { OwnedFd::from_raw_fd(owner) };
    namespace_metadata(&owner, NamespaceKind::User)?;
    for fd in &descriptors {
        if fd_mount(fd)? != fd_mount(&owner)? {
            return Err(namespace_refused());
        }
    }
    Ok(())
}
