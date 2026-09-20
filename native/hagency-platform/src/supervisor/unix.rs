use super::{StopCause, StopDetail, SupervisedReport};
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
#[cfg(not(target_os = "macos"))]
mod scope;
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
#[path = "unix/macos.rs"]
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
    Observe {
        nonce: u64,
    },
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
    Observed {
        nonce: u64,
    },
    Failed,
    Stopped {
        cause: StopCause,
        /// Fixed refusal category. Absent from an older guardian's frame and
        /// from every cause that names no category; never load-bearing.
        #[serde(default)]
        detail: Option<StopDetail>,
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
    observation_nonce: u64,
    pending_observation: Option<u64>,
    observation_failure: Option<io::ErrorKind>,
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
            observation_nonce: 0,
            pending_observation: None,
            observation_failure: None,
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
    pub(super) fn observe_leader(&mut self, timeout: Duration) -> io::Result<bool> {
        if self.report.is_some() || self.stop_requested {
            return Ok(false);
        }
        if let Some(kind) = self.observation_failure {
            return Err(io::Error::new(
                kind,
                "original guardian observation is unknown",
            ));
        }
        let result = self.observe_inner(Instant::now() + timeout);
        if let Err(error) = &result {
            self.observation_failure = Some(error.kind());
        }
        result
    }
    fn observe_inner(&mut self, until: Instant) -> io::Result<bool> {
        if self.child.try_wait()?.is_some() {
            return Ok(false);
        }
        let nonce = self
            .observation_nonce
            .checked_add(1)
            .ok_or_else(protocol_error)?;
        self.observation_nonce = nonce;
        // Retain possible-send custody even if a partial send or read expires.
        self.pending_observation = Some(nonce);
        self.pipe.send(&Request::Observe { nonce }, until)?;
        match self.pipe.required::<Reply>(until, 1024)? {
            Reply::Observed { nonce } => {
                self.accept_observation(nonce)?;
                // A buffered positive from an already-exited guardian is not
                // current positive custody. This checks the retained Child.
                Ok(self.child.try_wait()?.is_none())
            }
            Reply::Stopped {
                cause,
                detail,
                leader_exited,
                signals_accepted,
                whole_tree_stopped,
            } => {
                self.record_stopped(
                    cause,
                    detail,
                    leader_exited,
                    signals_accepted,
                    whole_tree_stopped,
                )?;
                #[cfg(target_os = "linux")]
                if self.recovery.is_some() {
                    // Observation must not cache a peer report in place of the
                    // original independent cgroup cleanup qualification.
                    if !signals_accepted {
                        self.recovery
                            .as_mut()
                            .ok_or_else(protocol_error)?
                            .remember_signal_failure();
                    }
                    self.report = None;
                    self.recover(cause, detail, until)?;
                }
                Ok(false)
            }
            _ => Err(protocol_error()),
        }
    }
    fn accept_observation(&mut self, nonce: u64) -> io::Result<()> {
        if self.pending_observation != Some(nonce) {
            return Err(protocol_error());
        }
        self.pending_observation = None;
        Ok(())
    }
    fn record_stopped(
        &mut self,
        cause: StopCause,
        detail: Option<StopDetail>,
        leader_exited: bool,
        signals_accepted: bool,
        whole_tree_stopped: bool,
    ) -> io::Result<()> {
        // Preserve the original backend's refusal of impossible stronger proof.
        if whole_tree_stopped && !cfg!(any(target_os = "linux", target_os = "macos")) {
            return Err(protocol_error());
        }
        self.pending_observation = None;
        self.report = Some(SupervisedReport {
            cause,
            detail,
            scope: StopReport {
                leader_exited,
                signals_accepted,
                whole_tree_stopped,
            },
        });
        Ok(())
    }
    pub(super) fn wait(&mut self, timeout: Duration) -> io::Result<Option<SupervisedReport>> {
        #[cfg(target_os = "linux")]
        if self.recovery.is_some() {
            if self.report.is_some() {
                return Ok(self.report);
            }
            let until = Instant::now() + timeout;
            let (cause, detail) = match self.wait_inner(timeout) {
                Ok(None) => return Ok(None),
                Ok(Some(report)) => {
                    if !report.scope.signals_accepted {
                        self.recovery
                            .as_mut()
                            .ok_or_else(protocol_error)?
                            .remember_signal_failure();
                    }
                    (report.cause, report.detail)
                }
                Err(_) => (StopCause::GuardianLost, None),
            };
            // A peer report is not the independent cgroup observation.
            self.report = None;
            return self.recover(cause, detail, until).map(Some);
        }
        self.wait_inner(timeout)
    }
    fn wait_inner(&mut self, timeout: Duration) -> io::Result<Option<SupervisedReport>> {
        if self.report.is_some() {
            return Ok(self.report);
        }
        let until = Instant::now() + timeout;
        while let Some(reply) = self.pipe.receive::<Reply>(until, 1024)? {
            match reply {
                Reply::Stopped {
                    cause,
                    detail,
                    leader_exited,
                    signals_accepted,
                    whole_tree_stopped,
                } => {
                    self.record_stopped(
                        cause,
                        detail,
                        leader_exited,
                        signals_accepted,
                        whole_tree_stopped,
                    )?;
                    break;
                }
                Reply::Observed { nonce } => self.accept_observation(nonce)?,
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
            return self.recover(StopCause::Requested, None, until);
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
    fn recover(
        &mut self,
        cause: StopCause,
        detail: Option<StopDetail>,
        until: Instant,
    ) -> io::Result<SupervisedReport> {
        let scope = self
            .recovery
            .as_mut()
            .ok_or_else(protocol_error)?
            .stop(until)?;
        // Empty population proves no live execution, not adoption/reaping of all
        // zombies. Only our retained guardian Child is reaped by this host.
        let _ = self.child.try_wait();
        let report = SupervisedReport {
            cause,
            detail,
            scope,
        };
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
            let _ = self.recover(StopCause::OwnerLost, None, until);
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
    let mut observation_nonce = 0u64;
    // The guardian's stderr is /dev/null by construction, so the only place a
    // refusal can be recorded is the terminal report it sends to its host.
    let mut detail: Option<StopDetail> = None;
    let cause = loop {
        #[cfg(target_os = "macos")]
        if let Err(error) = process.observe() {
            detail = Some(scope::observation_detail(&error));
            break StopCause::ObservationFailure;
        }
        match pipe.receive::<Request>(Instant::now() + Duration::from_millis(25), 1024) {
            Ok(Some(Request::Stop)) => break StopCause::Requested,
            Ok(Some(Request::Observe { nonce }))
                if observation_nonce.checked_add(1) == Some(nonce) =>
            {
                observation_nonce = nonce;
                match process.is_leader_running() {
                    Ok(true) => {
                        if pipe
                            .send(
                                &Reply::Observed { nonce },
                                Instant::now() + Duration::from_secs(1),
                            )
                            .is_err()
                        {
                            break StopCause::OwnerLost;
                        }
                    }
                    Ok(false) => break StopCause::LeaderExited,
                    Err(_) => {
                        detail = Some(StopDetail::LeaderUnreadable);
                        break StopCause::ObservationFailure;
                    }
                }
            }
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
            Err(_) => {
                detail = Some(StopDetail::LeaderUnreadable);
                break StopCause::ObservationFailure;
            }
        }
    };
    let report = process.stop(Duration::from_secs(2))?;
    let _ = pipe.send(
        &Reply::Stopped {
            cause,
            detail,
            leader_exited: report.leader_exited,
            signals_accepted: report.signals_accepted,
            whole_tree_stopped: report.whole_tree_stopped,
        },
        Instant::now() + Duration::from_secs(1),
    );
    if report.whole_tree_stopped {
        Ok(())
    } else {
        // Exit code 1 alone left three live occurrences unexplainable. The host
        // learns the same fact from the report above; this covers any embedding
        // that does give the guardian a stderr.
        Err(io::Error::other(format!(
            "complete descendant cleanup is unproven ({cause:?}, {detail:?})"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_guardian_observation_unknown() {
        // Private protocol negatives only. The retained disposable sleep child
        // stands in for a live guardian Child; scripted replies are NOT actual
        // physical observations, factory receipts or stronger cleanup evidence.
        for variant in ["absent", "late", "wrong", "late_wrong", "eof"] {
            let (host, peer) = UnixStream::pair().unwrap();
            let child = Command::new("/bin/sleep")
                .arg("2")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            let pid = child.id();
            let mut owner = Supervisor {
                child,
                pipe: Pipe::new(host).unwrap(),
                pid,
                stop_requested: false,
                report: None,
                observation_nonce: 0,
                pending_observation: None,
                observation_failure: None,
                #[cfg(target_os = "linux")]
                recovery: None,
            };
            let script = std::thread::spawn(move || {
                let mut peer = Pipe::new(peer).unwrap();
                let until = Instant::now() + Duration::from_secs(2);
                assert!(matches!(
                    peer.required::<Request>(until, 1024).unwrap(),
                    Request::Observe { nonce: 1 }
                ));
                if variant == "eof" {
                    return;
                }
                if variant == "wrong" {
                    peer.send(&Reply::Observed { nonce: 2 }, until).unwrap();
                }
                // Missing/late positive remains held until the host's actual
                // Stop. A second Observe here would fail the original script.
                assert!(matches!(
                    peer.required::<Request>(until, 1024).unwrap(),
                    Request::Stop
                ));
                if variant == "late" {
                    peer.send(&Reply::Observed { nonce: 1 }, until).unwrap();
                }
                if variant == "late_wrong" {
                    peer.send(&Reply::Observed { nonce: 2 }, until).unwrap();
                }
                peer.send(
                    &Reply::Stopped {
                        cause: StopCause::Requested,
                        detail: None,
                        leader_exited: true,
                        signals_accepted: true,
                        whole_tree_stopped: false,
                    },
                    until,
                )
                .unwrap();
            });
            assert!(
                owner.observe_leader(Duration::from_millis(40)).is_err(),
                "{variant}"
            );
            assert!(
                owner.observe_leader(Duration::from_secs(1)).is_err(),
                "unknown cannot rearm {variant}"
            );
            assert_eq!(owner.observation_nonce, 1);
            let result = owner.stop(Duration::from_secs(1));
            if ["eof", "late_wrong"].contains(&variant) {
                assert!(result.is_err());
            } else {
                let report = result.unwrap();
                assert_eq!(report.cause, StopCause::Requested);
                assert!(!report.scope.whole_tree_stopped);
            }
            script.join().unwrap();
            drop(owner);
        }
    }
}
