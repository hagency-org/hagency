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
/// Fixed categories naming why a guardian stopped observing. Diagnostic only:
/// a detail never authorizes anything and never strengthens `StopReport`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopDetail {
    /// A newcomer that neither ancestry, its session nor its process group
    /// classified in the same census.
    AncestryUnconfirmed,
    /// A bookkeeping bound, or a row that breaks an identity invariant.
    TrackingGap,
    /// The whole-system census itself could not be completed.
    CensusFailed,
    /// The retained leader's own identity could not be read.
    LeaderUnreadable,
}
/// Fixed categories naming why a stop after the leader exited did not prove the
/// tree gone. Diagnostic only: never authorizes anything and never strengthens
/// `StopReport`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopRefusal {
    /// A whole-system census during the stop could not be completed.
    CensusError,
    /// The tracker refused a census, or had already refused before the stop.
    TrackerGap,
    /// A stop or kill signal to an owned process was rejected by the kernel.
    SignalError,
    /// Owned processes were still live when the stop budget ran out.
    LiveDescendants,
    /// The leader itself could not be reaped inside the stop budget.
    RootUnreaped,
}
/// One owned process that kept a stop unproven. `exe` is the executable's file
/// name only, at most 64 bytes with control characters replaced; a guardian
/// carries at most eight rows. Evidence, never a signal target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveRow {
    pub pid: u32,
    pub ppid: u32,
    pub exe: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SupervisedReport {
    pub cause: StopCause,
    /// Present when the guardian could name a fixed refusal category.
    pub detail: Option<StopDetail>,
    pub scope: StopReport,
    /// Why a stop after the leader exited stayed unproven, when the guardian
    /// could name it. Never load-bearing.
    pub refusal: Option<StopRefusal>,
    /// Rows the guardian reported as still live, saturating at 255.
    pub live_count: u8,
    /// The leader's raw wait status as the guardian's `waitpid` returned it;
    /// `None` when the guardian never reaped it or the platform does not read it.
    pub leader_status: Option<i32>,
    /// The guardian process's own exit as the host read it after the `Stopped`
    /// frame or channel loss: its exit code, or the negated signal number when a
    /// signal ended it. `None` until the host reaped it.
    pub guardian_exit: Option<i32>,
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
    /// Freshly observe only this retained leader. On Unix a correlated reply
    /// requires the original guardian's retained leader observation; Windows
    /// checks its original process handle. Not model/sandbox or cleanup proof.
    /// Synchronous and bounded: call on the original execution worker, not HTTP.
    pub fn observe_leader(&mut self, timeout: Duration) -> io::Result<bool> {
        check_timeout(timeout)?;
        self.inner.observe_leader(timeout)
    }
    pub fn wait(&mut self, timeout: Duration) -> io::Result<Option<SupervisedReport>> {
        check_timeout(timeout)?;
        self.inner.wait(timeout)
    }
    pub fn stop(&mut self, timeout: Duration) -> io::Result<SupervisedReport> {
        check_timeout(timeout)?;
        self.inner.stop(timeout)
    }
    /// The last 4 KiB the guardian wrote to its own stderr, as collected so far,
    /// with control characters other than newline replaced. Diagnostic text
    /// only: it authorizes nothing and never strengthens a report. Empty on
    /// Windows, which owns the job directly and has no guardian.
    pub fn guardian_stderr_tail(&self) -> String {
        self.inner.guardian_stderr_tail()
    }
}
fn check_timeout(timeout: Duration) -> io::Result<()> {
    if timeout.is_zero() || timeout > Duration::from_secs(5) {
        Err(crate::invalid())
    } else {
        Ok(())
    }
}
