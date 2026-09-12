use crate::{Launch, OwnedProcess, StopReport};
use std::{
    io,
    time::{Duration, Instant},
};
#[cfg(target_os = "linux")]
#[path = "reaper.rs"]
mod reaper;

pub(super) struct Scope {
    process: Option<OwnedProcess>,
    #[cfg(target_os = "linux")]
    reaper: reaper::Reaper,
}
impl Scope {
    pub(super) fn prepare() -> io::Result<Self> {
        #[cfg(not(target_os = "linux"))]
        crate::unix_spawn::retain_child_exits()?;
        Ok(Self {
            process: None,
            #[cfg(target_os = "linux")]
            reaper: reaper::Reaper::prepare()?,
        })
    }
    pub(super) fn start(
        &mut self,
        launch: &Launch,
        pipes: Option<crate::stdio::ChildPipes>,
    ) -> io::Result<()> {
        if self.process.is_some() {
            return Err(io::Error::other("scope already started"));
        }
        self.process = Some(match pipes {
            Some(pipes) => OwnedProcess::spawn_piped(launch, pipes)?,
            None => OwnedProcess::spawn(launch)?,
        });
        Ok(())
    }
    pub(super) fn id(&self) -> u32 {
        self.process.as_ref().map_or(0, OwnedProcess::id)
    }
    pub(super) fn is_leader_running(&self) -> io::Result<bool> {
        self.process
            .as_ref()
            .ok_or_else(|| io::Error::other("scope has no leader"))?
            .is_leader_running()
    }
    pub(super) fn stop(&mut self, timeout: Duration) -> io::Result<StopReport> {
        let until = Instant::now() + timeout;
        loop {
            let mut report = match &mut self.process {
                Some(process) => process.stop(Duration::from_millis(5))?,
                None => StopReport {
                    leader_exited: true,
                    signals_accepted: true,
                    whole_tree_stopped: false,
                },
            };
            #[cfg(target_os = "linux")]
            {
                let root = (!report.leader_exited).then(|| self.id());
                report.whole_tree_stopped = self.reaper.step(root)? && report.leader_exited;
            }
            #[cfg(not(target_os = "linux"))]
            {
                report.whole_tree_stopped = false;
            }
            if report.whole_tree_stopped
                || (cfg!(not(target_os = "linux")) && report.leader_exited)
                || Instant::now() >= until
            {
                return Ok(report);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
impl Drop for Scope {
    fn drop(&mut self) {
        let _ = self.stop(Duration::from_secs(2));
    }
}
