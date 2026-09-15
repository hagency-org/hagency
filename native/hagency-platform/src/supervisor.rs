//! Private native supervision. Reports describe cleanup, never task completion.
use crate::{Launch, StopReport};
use serde::{Deserialize, Serialize};
use std::{io, path::Path, time::Duration};

#[cfg(unix)]
mod unix;
#[cfg(unix)]
use unix::Supervisor;
#[cfg(unix)]
pub use unix::run_guardian;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows::Supervisor;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopCause {
    Requested,
    LeaderExited,
    OwnerLost,
    ProtocolFailure,
    ObservationFailure,
    /// The trusted guardian channel failed. Does not independently assert that
    /// the guardian process exited; the retained cgroup supplies cleanup proof.
    GuardianLost,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SupervisedReport {
    pub cause: StopCause,
    pub scope: StopReport,
}
pub struct SupervisedProcess {
    inner: Supervisor,
}
impl SupervisedProcess {
    /// Linux-only, explicitly provisioned guardian-loss recovery. This does not
    /// satisfy `require_crash_containment` or enable production runner dispatch.
    #[cfg(target_os = "linux")]
    pub fn spawn_with_recovery(
        guardian: &Path,
        launch: &Launch,
        recovery: crate::CgroupRecovery,
    ) -> io::Result<Self> {
        launch.validate()?;
        if !guardian.is_absolute() {
            return Err(crate::invalid());
        }
        let (inner, _) = Supervisor::spawn_with_recovery(guardian, launch, false, recovery)?;
        Ok(Self { inner })
    }
    #[cfg(target_os = "linux")]
    pub fn spawn_piped_with_recovery(
        guardian: &Path,
        launch: &Launch,
        recovery: crate::CgroupRecovery,
    ) -> io::Result<(Self, crate::StdioPipes)> {
        launch.validate()?;
        if !guardian.is_absolute() {
            return Err(crate::invalid());
        }
        let (inner, pipes) = Supervisor::spawn_with_recovery(guardian, launch, true, recovery)?;
        Ok((Self { inner }, pipes.ok_or_else(crate::invalid)?))
    }
    /// Return one-use pipes while retaining the same guardian/job custody path.
    pub fn spawn_piped(guardian: &Path, launch: &Launch) -> io::Result<(Self, crate::StdioPipes)> {
        launch.validate()?;
        if !guardian.is_absolute() {
            return Err(crate::invalid());
        }
        let (inner, pipes) = Supervisor::spawn_piped(guardian, launch)?;
        Ok((Self { inner }, pipes))
    }
    /// On Unix the trusted executable must implement `guardian` using run_guardian.
    /// Windows owns the job directly and does not need a helper process.
    pub fn spawn(guardian: &Path, launch: &Launch) -> io::Result<Self> {
        launch.validate()?;
        if !guardian.is_absolute() {
            return Err(crate::invalid());
        }
        Ok(Self {
            inner: Supervisor::spawn(guardian, launch)?,
        })
    }
    pub fn id(&self) -> u32 {
        self.inner.id()
    }
    pub fn wait(&mut self, timeout: Duration) -> io::Result<Option<SupervisedReport>> {
        check_timeout(timeout)?;
        self.inner.wait(timeout)
    }
    pub fn stop(&mut self, timeout: Duration) -> io::Result<SupervisedReport> {
        check_timeout(timeout)?;
        self.inner.stop(timeout)
    }
}
fn check_timeout(timeout: Duration) -> io::Result<()> {
    if timeout.is_zero() || timeout > Duration::from_secs(5) {
        Err(crate::invalid())
    } else {
        Ok(())
    }
}
