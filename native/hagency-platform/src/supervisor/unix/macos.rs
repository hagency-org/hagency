//! Native port of the TS guardian's continuous descendant observation/stop path.
//! This is observed ancestry, not kernel crash containment or a runner sandbox.
use crate::{Launch, StopDetail, StopReport, stdio::ChildPipes};
use std::{
    io,
    time::{Duration, Instant},
};
#[path = "macos/native.rs"]
mod native;
#[path = "macos/tracking.rs"]
mod tracking;
use native::{Root, Snapshot};
use tracking::{Refusal, Tracking};

/// Name the fixed category the guardian reports to its host. A census that could
/// not be completed is a different fact from a census the tracker refused, and
/// the difference is exactly what the next live occurrence needs to explain
/// itself. Read from the error payload; never from its message.
pub(super) fn observation_detail(error: &std::io::Error) -> StopDetail {
    match error.get_ref().and_then(|e| e.downcast_ref::<Refusal>()) {
        Some(Refusal::Ancestry(_)) => StopDetail::AncestryUnconfirmed,
        Some(Refusal::Gap) => StopDetail::TrackingGap,
        None => StopDetail::CensusFailed,
    }
}

pub(super) struct Scope {
    root: Option<Root>,
    identity: Option<Snapshot>,
    tracking: Option<Tracking>,
    report: Option<StopReport>,
}
impl Scope {
    pub(super) fn prepare() -> io::Result<Self> {
        crate::unix_spawn::retain_child_exits()?;
        Ok(Self {
            root: None,
            identity: None,
            tracking: None,
            report: None,
        })
    }
    pub(super) fn start(&mut self, launch: &Launch, pipes: Option<ChildPipes>) -> io::Result<()> {
        if self.root.is_some() {
            return Err(io::Error::other("scope already started"));
        }
        self.root = Some(native::spawn(launch, pipes)?);
        let root = native::observe(self.root.as_ref().unwrap().pid)?
            .ok_or_else(|| io::Error::other("paused child missing"))?;
        self.identity = Some(root);
        if root.parent_pid != std::process::id() as i32 || root.status != 4 {
            return Err(io::Error::other("child was not suspended before work"));
        }
        let baseline = native::census()?;
        self.tracking = Some(Tracking::new(&baseline, root));
        if !native::signal(root, libc::SIGCONT)? {
            return Err(io::Error::other("paused child could not resume"));
        }
        Ok(())
    }
    pub(super) fn id(&self) -> u32 {
        self.root.as_ref().map_or(0, |r| r.pid as u32)
    }
    pub(super) fn is_leader_running(&self) -> io::Result<bool> {
        let root = self
            .identity
            .ok_or_else(|| io::Error::other("scope has no leader"))?;
        Ok(native::observe(root.pid)?.is_some_and(|s| s.birth == root.birth && s.status != 5))
    }
    pub(super) fn observe(&mut self) -> io::Result<()> {
        self.refresh().map(|_| ())
    }
    fn refresh(&mut self) -> io::Result<Vec<Snapshot>> {
        let tracking = self
            .tracking
            .as_mut()
            .ok_or_else(|| io::Error::other("no original process tracking"))?;
        match native::census() {
            Ok(rows) => tracking.update(&rows),
            Err(e) => {
                tracking.fail();
                Err(e)
            }
        }
    }
    pub(super) fn stop(&mut self, timeout: Duration) -> io::Result<StopReport> {
        if let Some(report) = self.report {
            return Ok(report);
        }
        if self.tracking.is_none() {
            // Admission never resumed the suspended child. Its exclusive Root
            // owns startup cleanup; do not burn the normal descendant deadline.
            let exited = match self.root.as_mut() {
                Some(root) => root.cancel_startup().unwrap_or(false),
                None => true,
            };
            let report = StopReport {
                leader_exited: exited,
                signals_accepted: false,
                whole_tree_stopped: false,
            };
            self.report = Some(report);
            return Ok(report);
        }
        let until = Instant::now() + timeout;
        let kill_at = Instant::now() + Duration::from_millis(100).min(timeout / 2);
        let mut signals = true;
        let mut empty = 0;
        loop {
            // Pause before discovering/killing parents, so the final census can
            // see their descendants. Original identities survive reparenting.
            let known = self
                .tracking
                .as_ref()
                .map_or_else(Vec::new, Tracking::owned);
            for row in known {
                if native::signal(row, libc::SIGSTOP).is_err() {
                    signals = false;
                    if let Some(t) = &mut self.tracking {
                        t.fail();
                    }
                }
            }
            let live = match self.refresh() {
                Ok(live) => live,
                Err(_) => self
                    .tracking
                    .as_ref()
                    .map_or_else(Vec::new, Tracking::owned),
            };
            let signal = if Instant::now() >= kill_at {
                libc::SIGKILL
            } else {
                libc::SIGTERM
            };
            for row in &live {
                if native::signal(*row, signal).is_err() {
                    signals = false;
                    if let Some(t) = &mut self.tracking {
                        t.fail();
                    }
                }
                if signal == libc::SIGTERM && native::signal(*row, libc::SIGCONT).is_err() {
                    signals = false;
                    if let Some(t) = &mut self.tracking {
                        t.fail();
                    }
                }
            }
            let exited = match self.root.as_mut() {
                Some(root) => root.reap()?,
                None => true,
            };
            let clean =
                exited && live.is_empty() && self.tracking.as_ref().is_some_and(|t| !t.failed());
            empty = if clean { empty + 1 } else { 0 };
            // A second complete census starts after the leader is already dead,
            // covering children born between an earlier list and parent exit.
            if empty >= 2 || Instant::now() >= until {
                let report = StopReport {
                    leader_exited: exited,
                    signals_accepted: signals,
                    whole_tree_stopped: empty >= 2 && signals,
                };
                self.report = Some(report);
                return Ok(report);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
impl Drop for Scope {
    fn drop(&mut self) {
        if self.root.is_some() && self.report.is_none() {
            let _ = self.stop(Duration::from_secs(2));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_macos_descendant_unknown() {
        let mut scope = Scope::prepare().unwrap();
        let launch = Launch {
            executable: "/bin/sleep".into(),
            arguments: vec!["8".into()],
            directory: std::env::temp_dir(),
            environment: Default::default(),
            require_crash_containment: false,
        };
        scope.start(&launch, None).unwrap();
        // Test-only lost observation on the original owner, not a production bypass.
        scope.tracking.as_mut().unwrap().fail();
        let report = scope.stop(Duration::from_millis(200)).unwrap();
        assert!(report.leader_exited);
        assert!(!report.whole_tree_stopped);
        assert_eq!(scope.stop(Duration::from_millis(200)).unwrap(), report);
    }
}
