use crate::{Launch, OwnedChildIdentity, StopReport};
use rustix::process::{Pid, Signal, kill_process_group};
use std::{
    io,
    os::unix::process::CommandExt,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

pub(super) struct Process {
    child: Option<Child>,
    pid: Pid,
    signalled: bool,
    identity: OwnedChildIdentity,
}
impl Process {
    pub(super) fn spawn(launch: &Launch) -> io::Result<Self> {
        Self::spawn_inner(launch, None)
    }
    pub(super) fn spawn_piped(
        launch: &Launch,
        pipes: crate::stdio::ChildPipes,
    ) -> io::Result<Self> {
        Self::spawn_inner(launch, Some(pipes))
    }
    fn spawn_inner(launch: &Launch, pipes: Option<crate::stdio::ChildPipes>) -> io::Result<Self> {
        if launch.require_crash_containment {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "POSIX guardian crash containment is not implemented",
            ));
        }
        let mut command = Command::new(&launch.executable);
        command
            .args(&launch.arguments)
            .current_dir(&launch.directory)
            .env_clear()
            .envs(&launch.environment)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0);
        if let Some(pipes) = pipes {
            command
                .stdin(Stdio::from(pipes.stdin))
                .stdout(Stdio::from(pipes.stdout))
                .stderr(Stdio::from(pipes.stderr));
        }
        crate::unix_spawn::seal(&mut command);
        let mut child = command.spawn()?;
        // Never permit kill(-1) semantics, even if an exotic namespace starts
        // with an unexpected PID. Only this unreaped child establishes authority.
        let Some(pid) = i32::try_from(child.id())
            .ok()
            .filter(|pid| *pid > 1)
            .and_then(Pid::from_raw)
        else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::other("invalid process scope leader"));
        };
        let identity = match OwnedChildIdentity::capture(&child) {
            Ok(identity) => identity,
            Err(error) => {
                // Spawn can already have created children. Keep the unreaped
                // leader anchor while cancelling its group on admission failure.
                let _ = kill_process_group(pid, Signal::KILL);
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };
        Ok(Self {
            child: Some(child),
            pid,
            signalled: false,
            identity,
        })
    }
    pub(super) fn id(&self) -> u32 {
        self.pid.as_raw_pid() as u32
    }
    pub(super) fn is_leader_running(&self) -> io::Result<bool> {
        self.identity.is_current()
    }
    pub(super) fn stop(&mut self, timeout: Duration) -> io::Result<StopReport> {
        let Some(child) = self.child.as_mut() else {
            return Ok(StopReport {
                leader_exited: true,
                signals_accepted: self.signalled,
                whole_tree_stopped: false,
            });
        };
        // Do not try_wait or reap before the final signal: the leader's retained
        // PID prevents an unrelated process from creating a reused group ID.
        if !self.signalled {
            let group_accepted = matches!(
                kill_process_group(self.pid, Signal::KILL),
                Ok(()) | Err(rustix::io::Errno::SRCH)
            );
            // The child may have moved out of its original group. Its unreaped
            // Child still owns the individual PID; this is not a census PID kill.
            let child_accepted = match child.kill() {
                Ok(()) => true,
                Err(error) => error.raw_os_error() == Some(rustix::io::Errno::SRCH.raw_os_error()),
            };
            // macOS can return EPERM for a group containing only a zombie.
            // Never infer an empty group from that error. Still attempt the
            // owned child signal and reap it, retaining the failure in the report.
            self.signalled = group_accepted && child_accepted;
        }
        let until = Instant::now() + timeout;
        loop {
            if child.try_wait()?.is_some() {
                self.child = None;
                return Ok(StopReport {
                    leader_exited: true,
                    signals_accepted: self.signalled,
                    whole_tree_stopped: false,
                });
            }
            if Instant::now() >= until {
                return Ok(StopReport {
                    leader_exited: false,
                    signals_accepted: self.signalled,
                    whole_tree_stopped: false,
                });
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
