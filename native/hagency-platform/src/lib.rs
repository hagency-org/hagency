//! Host-only process scope primitives. Not runner authorization or a sandbox.
use std::{collections::BTreeMap, ffi::OsString, io, path::PathBuf, time::Duration};

mod child_identity;
pub use child_identity::{ChildIdentity, OwnedChildIdentity, SignalOutcome};
#[allow(unsafe_code)]
mod directory_identity;
pub use directory_identity::{DirectoryIdentity, directory_identity, same_directory};
#[allow(unsafe_code)]
mod file_identity;
pub use file_identity::same_file;
mod supervisor;
#[cfg(unix)]
pub use supervisor::run_guardian;
pub use supervisor::{StopCause, SupervisedProcess, SupervisedReport};
mod stdio;
pub use stdio::StdioPipes;
#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
mod cgroup;
#[cfg(any(target_os = "linux", test))]
mod cgroup_checks;
#[cfg(target_os = "linux")]
pub use cgroup::CgroupRecovery;
#[cfg(windows)]
pub use windows::stdio::Pipe as WindowsPipe;

#[cfg(unix)]
mod unix;
#[cfg(unix)]
#[allow(unsafe_code)]
mod unix_spawn;
#[cfg(unix)]
use unix::Process;
#[cfg(windows)]
#[allow(unsafe_code)]
mod windows;
#[cfg(windows)]
use windows::Process;

/// Explicit host configuration; no serde constructor and no environment inheritance.
pub struct Launch {
    pub executable: PathBuf,
    pub arguments: Vec<OsString>,
    pub directory: PathBuf,
    pub environment: BTreeMap<OsString, OsString>,
    pub require_crash_containment: bool,
}
impl Launch {
    /// Checks bounded host configuration, not physical directory or sandbox custody.
    pub fn validate(&self) -> io::Result<()> {
        if !self.executable.is_absolute()
            || !self.directory.is_absolute()
            || !self.directory.is_dir()
            || self.arguments.len() > 256
            || self.environment.len() > 256
        {
            return Err(invalid());
        }
        let values = [self.executable.as_os_str(), self.directory.as_os_str()]
            .into_iter()
            .chain(self.arguments.iter().map(|v| v.as_os_str()))
            .chain(
                self.environment
                    .iter()
                    .flat_map(|(k, v)| [k.as_os_str(), v.as_os_str()]),
            );
        let mut length = 0usize;
        for value in values {
            let bytes = value.as_encoded_bytes();
            length = length.checked_add(bytes.len() + 1).ok_or_else(invalid)?;
            if bytes.contains(&0) || length > 64 * 1024 {
                return Err(invalid());
            }
        }
        for key in self.environment.keys() {
            let bytes = key.as_encoded_bytes();
            if bytes.is_empty()
                || !bytes
                    .iter()
                    .all(|v| v.is_ascii_alphanumeric() || *v == b'_')
            {
                return Err(invalid());
            }
        }
        Ok(())
    }
}
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "invalid native launch configuration",
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StopReport {
    pub leader_exited: bool,
    /// Every requested cancellation signal was accepted (or had no live target).
    /// False retains a signal failure even if the leader was subsequently reaped.
    pub signals_accepted: bool,
    /// POSIX group-only cancellation always leaves this false. Detached children
    /// and owner-crash cleanup require the later guardian/identity adapter.
    pub whole_tree_stopped: bool,
}
pub struct OwnedProcess {
    inner: Process,
}
impl OwnedProcess {
    pub fn spawn(launch: &Launch) -> io::Result<Self> {
        launch.validate()?;
        Ok(Self {
            inner: Process::spawn(launch)?,
        })
    }
    #[cfg(unix)]
    pub(crate) fn spawn_piped(launch: &Launch, pipes: stdio::ChildPipes) -> io::Result<Self> {
        launch.validate()?;
        Ok(Self {
            inner: Process::spawn_piped(launch, pipes)?,
        })
    }
    #[cfg(windows)]
    pub(crate) fn spawn_piped(
        launch: &Launch,
        pipes: windows::stdio::ChildPipes,
    ) -> io::Result<Self> {
        launch.validate()?;
        Ok(Self {
            inner: Process::spawn_piped(launch, pipes)?,
        })
    }
    /// Informational only: ownership is the private child/job handle, never this PID.
    pub fn id(&self) -> u32 {
        self.inner.id()
    }
    /// Observe without reaping: the retained leader still anchors final signals.
    pub fn is_leader_running(&self) -> io::Result<bool> {
        self.inner.is_leader_running()
    }
    pub fn stop(&mut self, timeout: Duration) -> io::Result<StopReport> {
        if timeout.is_zero() || timeout > Duration::from_secs(5) {
            return Err(invalid());
        }
        self.inner.stop(timeout)
    }
}
impl Drop for OwnedProcess {
    fn drop(&mut self) {
        let _ = self.inner.stop(Duration::from_secs(2));
    }
}
