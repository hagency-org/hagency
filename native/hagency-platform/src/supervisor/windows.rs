use super::{StopCause, SupervisedReport};
use crate::{Launch, OwnedProcess};
use std::{
    io,
    path::Path,
    time::{Duration, Instant},
};

pub(super) struct Supervisor {
    process: OwnedProcess,
    report: Option<SupervisedReport>,
}
impl Supervisor {
    pub(super) fn spawn_piped(
        _guardian: &Path,
        launch: &Launch,
    ) -> io::Result<(Self, crate::StdioPipes)> {
        let (host, child) = crate::StdioPipes::pair()?;
        let process = OwnedProcess::spawn_piped(launch, child)?;
        Ok((
            Self {
                process,
                report: None,
            },
            host,
        ))
    }
    pub(super) fn spawn(_guardian: &Path, launch: &Launch) -> io::Result<Self> {
        Ok(Self {
            process: OwnedProcess::spawn(launch)?,
            report: None,
        })
    }
    pub(super) fn id(&self) -> u32 {
        self.process.id()
    }
    pub(super) fn wait(&mut self, timeout: Duration) -> io::Result<Option<SupervisedReport>> {
        let until = Instant::now() + timeout;
        while self.report.is_none() {
            if !self.process.is_leader_running()? {
                let remaining = until.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    break;
                }
                let report = SupervisedReport {
                    cause: StopCause::LeaderExited,
                    scope: self.process.stop(remaining.min(Duration::from_secs(2)))?,
                };
                if report.scope.whole_tree_stopped {
                    self.report = Some(report);
                }
                return Ok(Some(report));
            }
            if Instant::now() >= until {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        Ok(self.report)
    }
    pub(super) fn stop(&mut self, timeout: Duration) -> io::Result<SupervisedReport> {
        if let Some(report) = self.report {
            return Ok(report);
        }
        let report = SupervisedReport {
            cause: StopCause::Requested,
            scope: self.process.stop(timeout)?,
        };
        if report.scope.whole_tree_stopped {
            self.report = Some(report);
        }
        Ok(report)
    }
}
