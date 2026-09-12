use super::{StopCause, SupervisedReport};
use crate::{Launch, StopReport};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    io,
    net::Shutdown,
    os::{fd::OwnedFd, unix::net::UnixStream},
    path::Path,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
mod pipe;
mod scope;
#[allow(unsafe_code)]
mod stdio;
use pipe::{FRAME_LIMIT, Pipe};

// Private wire data on an anonymous inherited socket. Deserialization is not
// runner authorization: only this child's host endpoint can send these messages.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Configuration {
    executable: OsString,
    arguments: Vec<OsString>,
    directory: OsString,
    environment: Vec<(OsString, OsString)>,
    require_crash_containment: bool,
}
impl Configuration {
    fn from_launch(launch: &Launch) -> Self {
        Self {
            executable: launch.executable.as_os_str().into(),
            arguments: launch.arguments.clone(),
            directory: launch.directory.as_os_str().into(),
            environment: launch
                .environment
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            require_crash_containment: launch.require_crash_containment,
        }
    }
    fn into_launch(self) -> io::Result<Launch> {
        let count = self.environment.len();
        let environment: BTreeMap<_, _> = self.environment.into_iter().collect();
        if count != environment.len() {
            return Err(crate::invalid());
        }
        let launch = Launch {
            executable: self.executable.into(),
            arguments: self.arguments,
            directory: self.directory.into(),
            environment,
            require_crash_containment: self.require_crash_containment,
        };
        launch.validate()?;
        if launch.require_crash_containment {
            return Err(unsupported());
        }
        Ok(launch)
    }
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    Prepare {
        version: u32,
        launch: Configuration,
    },
    PreparePiped {
        version: u32,
        launch: Configuration,
    },
    #[cfg(target_os = "linux")]
    PrepareRecovery {
        version: u32,
        launch: Configuration,
        piped: bool,
    },
    Start,
    Stop,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Reply {
    Prepared {
        version: u32,
    },
    Started {
        pid: u32,
    },
    Failed,
    Stopped {
        cause: StopCause,
        leader_exited: bool,
        signals_accepted: bool,
        whole_tree_stopped: bool,
    },
}
fn unsupported() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "POSIX detached descendant crash containment is not implemented",
    )
}

pub(super) struct Supervisor {
    child: Child,
    pipe: Pipe,
    pid: u32,
    stop_requested: bool,
    report: Option<SupervisedReport>,
    #[cfg(target_os = "linux")]
    recovery: Option<crate::CgroupRecovery>,
}
impl Supervisor {
    pub(super) fn spawn(guardian: &Path, launch: &Launch) -> io::Result<Self> {
        Self::spawn_inner(guardian, launch, false, |_| Ok(())).map(|(owner, _)| owner)
    }
    pub(super) fn spawn_piped(
        guardian: &Path,
        launch: &Launch,
    ) -> io::Result<(Self, crate::StdioPipes)> {
        let (owner, pipes) = Self::spawn_inner(guardian, launch, true, |_| Ok(()))?;
        Ok((owner, pipes.ok_or_else(protocol_error)?))
    }
    #[cfg(target_os = "linux")]
    pub(super) fn spawn_with_recovery(
        guardian: &Path,
        launch: &Launch,
        piped: bool,
        recovery: crate::CgroupRecovery,
    ) -> io::Result<(Self, Option<crate::StdioPipes>)> {
        crate::cgroup::validate_host()?;
        Self::spawn_inner(guardian, launch, piped, |owner| {
            owner.recovery = Some(recovery);
            owner
                .recovery
                .as_mut()
                .ok_or_else(protocol_error)?
                .attach_before_prepare(&owner.child)
        })
    }
    fn spawn_inner(
        guardian: &Path,
        launch: &Launch,
        piped: bool,
        before_prepare: impl FnOnce(&mut Self) -> io::Result<()>,
    ) -> io::Result<(Self, Option<crate::StdioPipes>)> {
        if launch.require_crash_containment {
            return Err(unsupported());
        }
        let configuration = Configuration::from_launch(launch);
        let (host_pipes, child_pipes) = if piped {
            let (host, child) = crate::StdioPipes::pair()?;
            (Some(host), Some(child))
        } else {
            (None, None)
        };
        let (owner, worker) = UnixStream::pair()?;
        let pipe = Pipe::new(owner)?;
        let input: OwnedFd = worker.into();
        // The socket is unnamed and only inherited as stdin by this guardian.
        // Work receives null or separately transferred pipe stdin and cannot
        // retain this socket or its duplicate guardian-reply endpoint.
        let mut command = Command::new(guardian);
        command
            .arg("guardian")
            .env_clear()
            .env("PATH", "")
            .current_dir(&launch.directory)
            .stdin(Stdio::from(input))
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        crate::unix_spawn::seal(&mut command);
        let child = command.spawn()?;
        let mut result = Self {
            child,
            pipe,
            pid: 0,
            stop_requested: false,
            report: None,
            #[cfg(target_os = "linux")]
            recovery: None,
        };
        let until = Instant::now() + Duration::from_secs(5);
        before_prepare(&mut result)?;
        let request = if piped {
            Request::PreparePiped {
                version: 1,
                launch: configuration,
            }
        } else {
            Request::Prepare {
                version: 1,
                launch: configuration,
            }
        };
        #[cfg(target_os = "linux")]
        let request = if result.recovery.is_some() {
            Request::PrepareRecovery {
                version: 1,
                launch: Configuration::from_launch(launch),
                piped,
            }
        } else {
            request
        };
        result.pipe.send(&request, until)?;
        if let Some(pipes) = child_pipes {
            stdio::send(&result.pipe.stream, pipes, until)?;
        }
        if !matches!(
            result.pipe.required::<Reply>(until, 1024)?,
            Reply::Prepared { version: 1 }
        ) {
            return Err(protocol_error());
        }
        result.pipe.send(&Request::Start, until)?;
        match result.pipe.required::<Reply>(until, 1024)? {
            Reply::Started { pid } if pid > 1 => result.pid = pid,
            _ => return Err(io::Error::other("native guardian launch failed")),
        }
        Ok((result, host_pipes))
    }
    pub(super) fn id(&self) -> u32 {
        self.pid
    }
    pub(super) fn wait(&mut self, timeout: Duration) -> io::Result<Option<SupervisedReport>> {
        #[cfg(target_os = "linux")]
        if self.recovery.is_some() {
            if self.report.is_some() {
                return Ok(self.report);
            }
            let until = Instant::now() + timeout;
            let cause = match self.wait_inner(timeout) {
                Ok(None) => return Ok(None),
                Ok(Some(report)) => {
                    if !report.scope.signals_accepted {
                        self.recovery
                            .as_mut()
                            .ok_or_else(protocol_error)?
                            .remember_signal_failure();
                    }
                    report.cause
                }
                Err(_) => StopCause::GuardianLost,
            };
            // A peer report is not the independent cgroup observation.
            self.report = None;
            return self.recover(cause, until).map(Some);
        }
        self.wait_inner(timeout)
    }
    fn wait_inner(&mut self, timeout: Duration) -> io::Result<Option<SupervisedReport>> {
        if self.report.is_some() {
            return Ok(self.report);
        }
        if let Some(reply) = self.pipe.receive::<Reply>(Instant::now() + timeout, 1024)? {
            match reply {
                Reply::Stopped {
                    cause,
                    leader_exited,
                    signals_accepted,
                    whole_tree_stopped,
                } => {
                    // This backend has no complete detached-child proof. Refuse
                    // an impossible stronger report rather than forwarding it.
                    if whole_tree_stopped && !cfg!(target_os = "linux") {
                        return Err(protocol_error());
                    }
                    self.report = Some(SupervisedReport {
                        cause,
                        scope: StopReport {
                            leader_exited,
                            signals_accepted,
                            whole_tree_stopped,
                        },
                    });
                }
                _ => return Err(protocol_error()),
            }
        }
        Ok(self.report)
    }
    pub(super) fn stop(&mut self, timeout: Duration) -> io::Result<SupervisedReport> {
        if let Some(report) = self.report {
            return Ok(report);
        }
        let until = Instant::now() + timeout;
        #[cfg(target_os = "linux")]
        if self.recovery.is_some() {
            return self.recover(StopCause::Requested, until);
        }
        if !self.stop_requested {
            self.stop_requested = true;
            if let Err(error) = self.pipe.send(&Request::Stop, until) {
                // A natural-exit report may already be buffered after peer EOF.
                if !matches!(
                    error.kind(),
                    io::ErrorKind::BrokenPipe | io::ErrorKind::ConnectionReset
                ) {
                    return Err(error);
                }
            }
        }
        self.wait(until.saturating_duration_since(Instant::now()))?
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::TimedOut,
                    "guardian cleanup outcome is unknown",
                )
            })
    }
    #[cfg(target_os = "linux")]
    fn recover(&mut self, cause: StopCause, until: Instant) -> io::Result<SupervisedReport> {
        let scope = self
            .recovery
            .as_mut()
            .ok_or_else(protocol_error)?
            .stop(until)?;
        // Empty population proves no live execution, not adoption/reaping of all
        // zombies. Only our retained guardian Child is reaped by this host.
        let _ = self.child.try_wait();
        let report = SupervisedReport { cause, scope };
        self.report = Some(report);
        Ok(report)
    }
}
impl Drop for Supervisor {
    fn drop(&mut self) {
        let until = Instant::now() + Duration::from_secs(3);
        #[cfg(target_os = "linux")]
        if self.recovery.is_some() && self.report.is_none() {
            // This explicitly optional path can kill a failed guardian safely:
            // its workspace inherited the independently retained cgroup first.
            let _ = self.recover(StopCause::OwnerLost, until);
        }
        let _ = self.pipe.stream.shutdown(Shutdown::Both);
        // EOF authorizes cleanup, not killing the guardian. Retain its independent
        // execution if observation times out; a forced kill could strand work.
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) | Err(_) => break,
                Ok(None) => {}
            }
            if Instant::now() >= until {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
fn protocol_error() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "unexpected guardian protocol state",
    )
}

/// Run only as an independent native guardian process. stdin must be the
/// inherited anonymous Unix socket. No repository, runtime token or TCP listener.
pub fn run_guardian() -> io::Result<()> {
    // CLOEXEC is essential: a runner must never inherit a descriptor that can
    // impersonate guardian replies to its host. stdin itself is replaced with
    // /dev/null or the transferred child pipe during the scoped child spawn.
    let input = rustix::io::fcntl_dupfd_cloexec(std::io::stdin(), 3)?;
    let mut pipe = Pipe::new(UnixStream::from(input))?;
    let until = Instant::now() + Duration::from_secs(5);
    let (launch, piped) = match pipe.required::<Request>(until, FRAME_LIMIT)? {
        Request::Prepare { version: 1, launch } => (launch, false),
        Request::PreparePiped { version: 1, launch } => (launch, true),
        #[cfg(target_os = "linux")]
        Request::PrepareRecovery {
            version: 1,
            launch,
            piped,
        } => {
            crate::cgroup::prepare_guardian()?;
            (launch, piped)
        }
        _ => return Err(protocol_error()),
    };
    let launch = launch.into_launch()?;
    let mut process = scope::Scope::prepare()?;
    let pipes = if piped {
        Some(stdio::receive(&pipe.stream, until)?)
    } else {
        None
    };
    pipe.send(&Reply::Prepared { version: 1 }, until)?;
    if !matches!(pipe.required::<Request>(until, 1024)?, Request::Start) {
        return Err(protocol_error());
    }
    if let Err(error) = process.start(&launch, pipes) {
        let _ = pipe.send(&Reply::Failed, Instant::now() + Duration::from_secs(1));
        return Err(error);
    }
    // If the owner disappeared during spawn, failed notification drops the
    // owned scope. Work has never existed without a live guardian owner.
    pipe.send(
        &Reply::Started { pid: process.id() },
        Instant::now() + Duration::from_secs(1),
    )?;
    let cause = loop {
        match pipe.receive::<Request>(Instant::now() + Duration::from_millis(25), 1024) {
            Ok(Some(Request::Stop)) => break StopCause::Requested,
            Ok(Some(_)) => break StopCause::ProtocolFailure,
            Ok(None) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::UnexpectedEof | io::ErrorKind::ConnectionReset
                ) =>
            {
                break StopCause::OwnerLost;
            }
            Err(_) => break StopCause::ProtocolFailure,
        }
        match process.is_leader_running() {
            Ok(true) => {}
            Ok(false) => break StopCause::LeaderExited,
            Err(_) => break StopCause::ObservationFailure,
        }
    };
    let report = process.stop(Duration::from_secs(2))?;
    let _ = pipe.send(
        &Reply::Stopped {
            cause,
            leader_exited: report.leader_exited,
            signals_accepted: report.signals_accepted,
            whole_tree_stopped: report.whole_tree_stopped,
        },
        Instant::now() + Duration::from_secs(1),
    );
    if report.whole_tree_stopped {
        Ok(())
    } else {
        Err(io::Error::other("complete descendant cleanup is unproven"))
    }
}
