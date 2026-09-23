//! ADR-181: what a stop leaves behind for the operator, read through a real
//! guardian. Every field asserted here is evidence beside the report; no
//! assertion reads one as proof of cleanup, and no verdict depends on them.
#![cfg(target_os = "macos")]
use hagency_platform::{Launch, StopCause, StopRefusal, SupervisedProcess};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    os::unix::process::ExitStatusExt,
    path::{Path, PathBuf},
    process::{Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

fn binary() -> PathBuf {
    env!("CARGO_BIN_EXE_hagency-platform-probe").into()
}
fn launch(root: &Path, mode: &str, marker: &Path) -> Launch {
    let mut environment = BTreeMap::new();
    environment.insert("PATH".into(), "".into());
    Launch {
        executable: binary(),
        arguments: vec![mode.into(), marker.as_os_str().into()],
        directory: root.into(),
        environment,
        require_crash_containment: false,
    }
}
fn wait_for(path: &Path) {
    let until = Instant::now() + Duration::from_secs(5);
    while !path.exists() {
        assert!(Instant::now() < until, "fixture never wrote {path:?}");
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn quiet(command: &mut Command) -> &mut Command {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
}
/// Startup only, as in the other macOS fixtures: the supervisor's fixed
/// prepare/start budget expires on a loaded build host.
fn spawn(root: &Path, mode: &str, marker: &Path) -> SupervisedProcess {
    let mut last = None;
    for _ in 0..12 {
        match SupervisedProcess::spawn(&binary(), &launch(root, mode, marker)) {
            Ok(process) => return process,
            Err(error) => {
                last = Some(error);
                std::thread::sleep(Duration::from_millis(250));
            }
        }
    }
    panic!("guardian never started: {last:?}");
}

/// A leader that exits leaving one descendant that ignores TERM. The guardian
/// names what its stop proved and what it did not; the host reads the
/// guardian's own exit beside it. The stop budget sends KILL a hundred
/// milliseconds in, which a sleep cannot ignore, so on this host the stop is
/// expected to prove the tree gone; both outcomes are pinned, and a stop that
/// stays unproven must name `live_descendants` and exit 1 with its line.
#[test]
fn native_guardian_report_names_the_stop_refusal() {
    let root = tempfile::tempdir().unwrap();
    let marker = root.path().join("trap");
    let mut owned = spawn(root.path(), "exit-leaving-trap", &marker);
    wait_for(&marker.with_extension("child"));
    let descendant = fs::read_to_string(marker.with_extension("child")).unwrap();
    let descendant: u32 = descendant.trim().parse().unwrap();
    let mut report = None;
    for _ in 0..3 {
        report = owned.wait(Duration::from_secs(5)).unwrap();
        if report.is_some() {
            break;
        }
    }
    let report = report.expect("the guardian must observe the leader's exit");
    assert_eq!(report.cause, StopCause::LeaderExited);
    assert_eq!(report.detail, None);
    assert!(report.scope.leader_exited && report.scope.signals_accepted);
    // The leader's own exit as the guardian reaped it: `exit(0)`.
    let status = ExitStatus::from_raw(
        report
            .leader_status
            .expect("the guardian reaped the leader"),
    );
    assert_eq!(status.code(), Some(0), "{status:?}");
    if report.scope.whole_tree_stopped {
        assert_eq!(report.refusal, None);
        assert_eq!(report.live_count, 0);
        assert_eq!(report.guardian_exit, Some(0));
        assert_eq!(owned.guardian_stderr_tail(), "");
    } else {
        assert_eq!(report.refusal, Some(StopRefusal::LiveDescendants));
        assert!(report.live_count >= 1);
        assert_eq!(report.guardian_exit, Some(1));
        let tail = owned.guardian_stderr_tail();
        assert!(
            tail.contains(&format!(
                "guardian stop unproven: live_descendants rows={}\n",
                report.live_count
            )),
            "{tail:?}"
        );
    }
    // Evidence settles with the report: a later stop repeats it unchanged.
    assert_eq!(owned.stop(Duration::from_secs(1)).unwrap(), report);
    drop(owned);
    // This test started that sleep; whatever the guardian proved, end it.
    let _ = quiet(&mut Command::new("/bin/kill"))
        .args(["-KILL", &descendant.to_string()])
        .status();
}

/// A clean stop through a real guardian: the host reads the guardian's exit,
/// its stderr tail stays empty, and the work's own stderr pipe never carries a
/// byte of the guardian's. The guardian started without a host pipe is the
/// wire-level `native_guardian_observation_protocol` case in `guardian.rs`,
/// which spawns it with a null stderr and reads the same frames as before.
#[test]
fn native_guardian_stderr_reaches_the_host() {
    let root = tempfile::tempdir().unwrap();
    let marker = root.path().join("clean");
    let (mut owned, pipes) =
        SupervisedProcess::spawn_piped(&binary(), &launch(root.path(), "leader", &marker)).unwrap();
    let (stdin, stdout, stderr) = pipes.into_parts();
    let work_stderr = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = fs::File::from(stderr).read_to_end(&mut bytes);
        bytes
    });
    wait_for(&marker.with_extension("child"));
    assert_eq!(owned.guardian_stderr_tail(), "");
    let report = owned.stop(Duration::from_secs(5)).unwrap();
    assert_eq!(report.cause, StopCause::Requested);
    assert!(report.scope.leader_exited && report.scope.whole_tree_stopped);
    assert_eq!((report.refusal, report.live_count), (None, 0));
    assert!(report.leader_status.is_some());
    assert_eq!(report.guardian_exit, Some(0));
    assert_eq!(owned.guardian_stderr_tail(), "");
    drop(owned);
    drop((stdin, stdout));
    // The leader held the only write end of its stderr; it is gone, so this
    // reader has seen everything the work ever wrote there.
    let work = work_stderr.join().unwrap();
    assert!(work.is_empty(), "{:?}", String::from_utf8_lossy(&work));
}
