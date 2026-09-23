use super::{LiveRow, StopCause, StopDetail, StopRefusal, SupervisedReport};
use crate::{Launch, StopReport};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, VecDeque},
    ffi::OsString,
    io::{self, Read, Write},
    net::Shutdown,
    os::{fd::OwnedFd, unix::net::UnixStream, unix::process::ExitStatusExt},
    path::Path,
    process::{Child, ChildStderr, Command, Stdio},
    sync::{Arc, Mutex},
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
        /// Why a stop after the leader exited stayed unproven, the rows that
        /// kept it so and the leader's raw wait status. All optional on the
        /// wire so an older guardian's frame still parses; none load-bearing.
        #[serde(default)]
        refusal: Option<StopRefusal>,
        #[serde(default)]
        live: Vec<LiveRow>,
        #[serde(default)]
        leader_status: Option<i32>,
    },
}
/// A `Stopped` frame with eight bounded rows stays well inside this; every
/// other reply is a few dozen bytes.
const REPLY_LIMIT: usize = 4096;
/// The last bytes the guardian wrote to its own stderr that the host keeps.
const STDERR_TAIL: usize = 4096;
fn unsupported() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "POSIX detached descendant crash containment is not implemented",
    )
}
/// The host end of the guardian's stderr pipe. One blocking thread drains it
/// into a bounded tail so the guardian never blocks on a full pipe and no host
/// operation ever waits on the guardian's output; the thread ends with the pipe.
struct StderrTail {
    bytes: Arc<Mutex<VecDeque<u8>>>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl StderrTail {
    fn drain(mut stderr: ChildStderr) -> Option<Self> {
        let bytes = Arc::new(Mutex::new(VecDeque::with_capacity(STDERR_TAIL)));
        let sink = Arc::clone(&bytes);
        let thread = std::thread::Builder::new()
            .name("guardian-stderr".into())
            .spawn(move || {
                let mut buffer = [0u8; 1024];
                loop {
                    match stderr.read(&mut buffer) {
                        Ok(0) => break,
                        Ok(count) => {
                            let mut tail = sink.lock().unwrap_or_else(|e| e.into_inner());
                            for &byte in &buffer[..count] {
                                if tail.len() == STDERR_TAIL {
                                    tail.pop_front();
                                }
                                tail.push_back(byte);
                            }
                        }
                        Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                        Err(_) => break,
                    }
                }
            })
            // Without a drainer the read end closes here, so a guardian write
            // fails at once instead of ever blocking; the guardian ignores it.
            .ok()?;
        Some(Self {
            bytes,
            thread: Some(thread),
        })
    }
    fn render(&self) -> String {
        let tail = self.bytes.lock().unwrap_or_else(|e| e.into_inner());
        let (front, back) = tail.as_slices();
        let mut bytes = Vec::with_capacity(tail.len());
        bytes.extend_from_slice(front);
        bytes.extend_from_slice(back);
        String::from_utf8_lossy(&bytes)
            .chars()
            .map(|c| {
                if c.is_control() && c != '\n' {
                    '\u{FFFD}'
                } else {
                    c
                }
            })
            .collect()
    }
    /// Once the guardian is reaped its pipe is at EOF, so the drainer is about
    /// to end; wait for it within a small bound so a tail read after the exit
    /// status holds everything the guardian wrote. Never blocks past the bound.
    fn finish(&mut self, until: Instant) {
        let Some(thread) = self.thread.take() else {
            return;
        };
        while !thread.is_finished() {
            if Instant::now() >= until {
                self.thread = Some(thread);
                return;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        let _ = thread.join();
    }
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
    stderr: Option<StderrTail>,
    guardian_exit: Option<i32>,
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
        //
        // The guardian's stderr is a pipe back to this host (ADR-181): the only
        // place its own refusal line can go. The work can never inherit it: on
        // macOS the scope spawns the work with POSIX_SPAWN_CLOEXEC_DEFAULT and
        // dup2s descriptors 0..=2 from the work's own transferred pipes or
        // /dev/null (`macos/native.rs`); elsewhere `unix.rs` sets the work's
        // stdio to those same pipes or null and `seal` marks the rest CLOEXEC.
        let mut command = Command::new(guardian);
        command
            .arg("guardian")
            .env_clear()
            .env("PATH", "")
            .current_dir(&launch.directory)
            .stdin(Stdio::from(input))
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        crate::unix_spawn::seal(&mut command);
        let mut child = command.spawn()?;
        let stderr = child.stderr.take().and_then(StderrTail::drain);
        let mut result = Self {
            child,
            pipe,
            pid: 0,
            stop_requested: false,
            report: None,
            observation_nonce: 0,
            pending_observation: None,
            observation_failure: None,
            stderr,
            guardian_exit: None,
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
            result.pipe.required::<Reply>(until, REPLY_LIMIT)?,
            Reply::Prepared { version: 1 }
        ) {
            return Err(protocol_error());
        }
        result.pipe.send(&Request::Start, until)?;
        match result.pipe.required::<Reply>(until, REPLY_LIMIT)? {
            Reply::Started { pid } if pid > 1 => result.pid = pid,
            _ => return Err(io::Error::other("native guardian launch failed")),
        }
        Ok((result, host_pipes))
    }
    pub(super) fn id(&self) -> u32 {
        self.pid
    }
    pub(super) fn guardian_stderr_tail(&self) -> String {
        self.stderr
            .as_ref()
            .map_or_else(String::new, StderrTail::render)
    }
    /// Read the guardian's own exit once, within `until`, after its terminal
    /// frame or the loss of its channel. Evidence only: whatever it says, no
    /// verdict changes, and a guardian still running is left for `Drop`.
    fn reap_guardian(&mut self, until: Instant) {
        if self.guardian_exit.is_some() {
            return;
        }
        loop {
            match self.child.try_wait() {
                Ok(Some(status)) => {
                    self.guardian_exit = status.code().or_else(|| status.signal().map(|s| -s));
                    break;
                }
                Ok(None) => {}
                Err(_) => return,
            }
            if Instant::now() >= until {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        if let Some(tail) = &mut self.stderr {
            tail.finish(Instant::now() + Duration::from_millis(200));
        }
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
        match self.pipe.required::<Reply>(until, REPLY_LIMIT)? {
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
                refusal,
                live,
                leader_status,
            } => {
                self.record_stopped(
                    Stopped {
                        cause,
                        detail,
                        scope: StopReport {
                            leader_exited,
                            signals_accepted,
                            whole_tree_stopped,
                        },
                        refusal,
                        live_count: u8::try_from(live.len()).unwrap_or(u8::MAX),
                        leader_status,
                    },
                    until,
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
    fn record_stopped(&mut self, stopped: Stopped, until: Instant) -> io::Result<()> {
        // Preserve the original backend's refusal of impossible stronger proof.
        if stopped.scope.whole_tree_stopped && !cfg!(any(target_os = "linux", target_os = "macos"))
        {
            return Err(protocol_error());
        }
        self.pending_observation = None;
        // The terminal frame is the guardian's last word; it exits right after.
        // Read that exit within the caller's remaining budget, one second at
        // most, and keep it beside the report without changing the report.
        self.reap_guardian(until.min(Instant::now() + Duration::from_secs(1)));
        self.report = Some(SupervisedReport {
            cause: stopped.cause,
            detail: stopped.detail,
            scope: stopped.scope,
            refusal: stopped.refusal,
            live_count: stopped.live_count,
            leader_status: stopped.leader_status,
            guardian_exit: self.guardian_exit,
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
        loop {
            let reply = match self.pipe.receive::<Reply>(until, REPLY_LIMIT) {
                Ok(Some(reply)) => reply,
                Ok(None) => break,
                Err(error) => {
                    // Channel loss: the guardian is gone or going. Its exit is
                    // still evidence worth one bounded read; the error stands.
                    if matches!(
                        error.kind(),
                        io::ErrorKind::UnexpectedEof | io::ErrorKind::ConnectionReset
                    ) {
                        self.reap_guardian(until.min(Instant::now() + Duration::from_secs(1)));
                    }
                    return Err(error);
                }
            };
            match reply {
                Reply::Stopped {
                    cause,
                    detail,
                    leader_exited,
                    signals_accepted,
                    whole_tree_stopped,
                    refusal,
                    live,
                    leader_status,
                } => {
                    self.record_stopped(
                        Stopped {
                            cause,
                            detail,
                            scope: StopReport {
                                leader_exited,
                                signals_accepted,
                                whole_tree_stopped,
                            },
                            refusal,
                            live_count: u8::try_from(live.len()).unwrap_or(u8::MAX),
                            leader_status,
                        },
                        until,
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
        self.reap_guardian(Instant::now());
        let report = SupervisedReport {
            cause,
            detail,
            scope,
            refusal: None,
            live_count: 0,
            leader_status: None,
            guardian_exit: self.guardian_exit,
        };
        self.report = Some(report);
        Ok(report)
    }
}
/// A parsed `Stopped` frame on its way into the report.
struct Stopped {
    cause: StopCause,
    detail: Option<StopDetail>,
    scope: StopReport,
    refusal: Option<StopRefusal>,
    live_count: u8,
    leader_status: Option<i32>,
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
    // The terminal report is where a refusal is recorded for the host; the
    // one stderr line written below it is the same fact for whoever kept the
    // guardian's stderr, and /dev/null when nobody did.
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
    // Evidence beside the report (ADR-181): only the macOS scope names why a
    // stop stayed unproven and reaps the leader itself; the other arms carry
    // nothing and behave as before.
    #[cfg(target_os = "macos")]
    let (refusal, live, leader_status) = {
        let evidence = process.evidence();
        (evidence.refusal, evidence.live, evidence.leader_status)
    };
    #[cfg(not(target_os = "macos"))]
    let (refusal, live, leader_status): (Option<StopRefusal>, Vec<LiveRow>, Option<i32>) =
        (None, Vec::new(), None);
    let _ = pipe.send(
        &Reply::Stopped {
            cause,
            detail,
            leader_exited: report.leader_exited,
            signals_accepted: report.signals_accepted,
            whole_tree_stopped: report.whole_tree_stopped,
            refusal,
            live: live.clone(),
            leader_status,
        },
        Instant::now() + Duration::from_secs(1),
    );
    if report.whole_tree_stopped {
        Ok(())
    } else {
        // One line for the host's stderr tail, then exit code 1. The host
        // learns the same fact from the report above; the line is what a host
        // reads when the frame was lost, and /dev/null when nobody kept the
        // guardian's stderr. Fixed labels and a count: never free text.
        let _ = writeln!(
            io::stderr(),
            "guardian stop unproven: {} rows={}",
            refusal_label(refusal),
            live.len()
        );
        Err(io::Error::other(format!(
            "complete descendant cleanup is unproven ({cause:?}, {detail:?}, {refusal:?})"
        )))
    }
}
fn refusal_label(refusal: Option<StopRefusal>) -> &'static str {
    match refusal {
        None => "none",
        Some(StopRefusal::CensusError) => "census_error",
        Some(StopRefusal::TrackerGap) => "tracker_gap",
        Some(StopRefusal::SignalError) => "signal_error",
        Some(StopRefusal::LiveDescendants) => "live_descendants",
        Some(StopRefusal::RootUnreaped) => "root_unreaped",
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
                stderr: None,
                guardian_exit: None,
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
                        refusal: None,
                        live: Vec::new(),
                        leader_status: None,
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
                assert_eq!((report.refusal, report.live_count), (None, 0));
                assert_eq!(report.leader_status, None);
            }
            script.join().unwrap();
            drop(owner);
        }
    }
    /// Host-side handling of what ADR-181 added to the terminal frame, driven
    /// by a scripted peer: the refusal and its rows are carried as evidence,
    /// an older guardian's frame without them still parses, the guardian's
    /// own exit is read after the frame, and its stderr reaches the bounded
    /// tail without ever blocking the host. The stand-in child plays the
    /// guardian's process: it writes one line and exits 1 like an unproven
    /// stop. Nothing here is a physical observation of any owned tree.
    #[test]
    fn native_guardian_evidence_reaches_the_host() {
        for older in [false, true] {
            let (host, peer) = UnixStream::pair().unwrap();
            let mut child = Command::new("/bin/sh")
                .args([
                    "-c",
                    "printf 'guardian stop unproven: live_descendants rows=2\\n\\001x' >&2; exit 1",
                ])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            let stderr = child.stderr.take().and_then(StderrTail::drain);
            assert!(stderr.is_some());
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
                stderr,
                guardian_exit: None,
                #[cfg(target_os = "linux")]
                recovery: None,
            };
            let script = std::thread::spawn(move || {
                let mut peer = Pipe::new(peer).unwrap();
                let until = Instant::now() + Duration::from_secs(2);
                assert!(matches!(
                    peer.required::<Request>(until, 1024).unwrap(),
                    Request::Stop
                ));
                let frame = if older {
                    serde_json::json!({
                        "kind": "stopped", "cause": "leader_exited",
                        "leader_exited": true, "signals_accepted": true,
                        "whole_tree_stopped": false
                    })
                } else {
                    serde_json::json!({
                        "kind": "stopped", "cause": "leader_exited", "detail": null,
                        "leader_exited": true, "signals_accepted": true,
                        "whole_tree_stopped": false, "refusal": "live_descendants",
                        "live": [
                            {"pid": 4242, "ppid": 4241, "exe": "sleep"},
                            {"pid": 4243, "ppid": 4242, "exe": "sh"}
                        ],
                        "leader_status": 256
                    })
                };
                peer.send(&frame, until).unwrap();
            });
            let report = owner.stop(Duration::from_secs(3)).unwrap();
            script.join().unwrap();
            assert_eq!(report.cause, StopCause::LeaderExited);
            assert!(!report.scope.whole_tree_stopped);
            if older {
                assert_eq!((report.refusal, report.live_count), (None, 0));
                assert_eq!(report.leader_status, None);
            } else {
                assert_eq!(report.refusal, Some(StopRefusal::LiveDescendants));
                assert_eq!(report.live_count, 2);
                assert_eq!(report.leader_status, Some(256));
            }
            assert_eq!(report.guardian_exit, Some(1), "older={older}");
            assert_eq!(
                owner.guardian_stderr_tail(),
                "guardian stop unproven: live_descendants rows=2\n\u{FFFD}x"
            );
            // The report is settled: a second stop repeats it unchanged.
            assert_eq!(owner.stop(Duration::from_secs(1)).unwrap(), report);
            drop(owner);
        }
    }
    #[test]
    fn native_guardian_stderr_tail_is_bounded() {
        let mut child = Command::new("/bin/sh")
            .args([
                "-c",
                "i=0; while [ $i -lt 400 ]; do printf '%05d-line-of-noise\\n' $i >&2; i=$((i+1)); done",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut tail = child.stderr.take().and_then(StderrTail::drain).unwrap();
        assert!(child.wait().unwrap().success());
        tail.finish(Instant::now() + Duration::from_secs(2));
        let rendered = tail.render();
        // 400 lines of 20 bytes exceed the bound; only the newest survive.
        assert_eq!(rendered.len(), STDERR_TAIL);
        assert!(rendered.ends_with("00399-line-of-noise\n"), "{rendered}");
        assert!(!rendered.contains("00100-line-of-noise"));
    }
}
