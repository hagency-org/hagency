//! The residual macOS refusal, deliberately kept fatal.
//!
//! Session evidence covers a survivor that only left its process group. A
//! survivor whose unseen parent opened a *session* of its own shares neither a
//! session nor a group with anything classified, and nothing in a census can
//! decide whether it descends from the owned leader. It still refuses, and the
//! guardian must now name why instead of exiting 1 in silence.
//!
//! This test lives alone in its own binary on purpose. The census is
//! whole-system, so the row it creates ends the observation of EVERY guardian
//! on the host — including the ones other tests in the same binary are running
//! in parallel. That blast radius is the defect's most important property and
//! the reason the remaining case is worth closing with a complete fork feed
//! rather than more census evidence.
#![cfg(target_os = "macos")]
use hagency_platform::{Launch, StopCause, StopDetail, SupervisedProcess};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

fn binary() -> PathBuf {
    env!("CARGO_BIN_EXE_hagency-platform-probe").into()
}
fn quiet(command: &mut Command) -> &mut Command {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
}
/// Startup only. The supervisor's fixed five-second prepare/start budget
/// expires on a loaded build host; retrying it is not retrying the observation
/// under test, which is never retried.
fn spawn_supervised(root: &Path, marker: &Path) -> SupervisedProcess {
    let mut environment = BTreeMap::new();
    environment.insert("PATH".into(), "".into());
    let launch = Launch {
        executable: binary(),
        arguments: vec!["leaf".into(), marker.as_os_str().into()],
        directory: root.into(),
        environment,
        require_crash_containment: false,
    };
    let mut last = None;
    for _ in 0..12 {
        match SupervisedProcess::spawn(&binary(), &launch) {
            Ok(process) => return process,
            Err(error) => {
                last = Some(error);
                std::thread::sleep(Duration::from_millis(250));
            }
        }
    }
    panic!("guardian never started: {last:?}");
}

#[test]
fn native_macos_unseen_session_orphan_refuses_and_names_its_cause() {
    let root = tempfile::tempdir().unwrap();
    let marker = root.path().join("idle");
    let mut process = spawn_supervised(root.path(), &marker);
    let entered = marker.with_extension("entered");
    let until = Instant::now() + Duration::from_secs(10);
    while !entered.exists() {
        assert!(Instant::now() < until, "fixture did not start");
        std::thread::sleep(Duration::from_millis(10));
    }
    let until = Instant::now() + Duration::from_secs(5);
    let mut samples = 0;
    let mut lost = false;
    while Instant::now() < until {
        // The middle calls setsid, spawns a subshell that forks a survivor, and
        // exits before the survivor is born. Nothing classified remains in that
        // session or that group.
        let orphan = root.path().join(format!("orphan{samples}"));
        quiet(&mut Command::new(binary()))
            .args(["foreign-session-middle".as_ref(), orphan.as_os_str()])
            .status()
            .unwrap();
        samples += 1;
        if !process
            .observe_leader(Duration::from_secs(2))
            .unwrap_or(false)
        {
            lost = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(
        lost,
        "unclassifiable session orphan was classified anyway after {samples} samples"
    );
    let report = process.stop(Duration::from_secs(5)).unwrap();
    assert_eq!(report.cause, StopCause::ObservationFailure);
    assert_eq!(report.detail, Some(StopDetail::AncestryUnconfirmed));
    // The tracker stays poisoned by that refusal, so the receipt is negative
    // even though the leader was reaped and every signal was accepted. That is
    // the documented consequence of keeping the residual case fatal.
    assert!(report.scope.leader_exited && report.scope.signals_accepted);
    assert!(!report.scope.whole_tree_stopped);
}
