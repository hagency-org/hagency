//! macOS descendant tracking under *detached* process churn.
//!
//! `macos_churn.rs` keeps its survivor in the test runner's own process group,
//! so group evidence is always available. Real hosts also run spawners that
//! detach: Rust's `Command::process_group`, Node's `detached: true`, Python's
//! `start_new_session=True`, `posix_spawn` with SETSID, `ssh -f`, shell job
//! control. Their survivors are the only member of a group no census ever
//! classified, and the tracker refused every one of them — which stopped the
//! owned tree of *every* guardian on the host, because the census is
//! whole-system and the offending process need belong to no agent at all.
//!
//! Session evidence covers the group-only case. The survivor that reaches a
//! session of its own through an unseen parent still refuses; that residual
//! case has its own binary in `macos_session_refusal.rs`, because the row it
//! creates would end these tests' observations too.
#![cfg(target_os = "macos")]
use hagency_platform::{Launch, StopCause, SupervisedProcess};
use std::{
    collections::BTreeMap,
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
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
    let mut last = None;
    for _ in 0..12 {
        match SupervisedProcess::spawn(&binary(), &launch(root, "leaf", marker)) {
            Ok(process) => return process,
            Err(error) => {
                last = Some(error);
                std::thread::sleep(Duration::from_millis(250));
            }
        }
    }
    panic!("guardian never started: {last:?}");
}
fn wait_for(path: &Path) {
    let until = Instant::now() + Duration::from_secs(10);
    while !path.exists() {
        assert!(Instant::now() < until, "fixture did not write {path:?}");
        std::thread::sleep(Duration::from_millis(10));
    }
}
/// A subshell that forks a survivor and exits at once. With `detached` it also
/// leads its own process group, so the survivor it leaves behind is the only
/// member of that group; its session still holds this test process.
fn churn_once(detached: bool) {
    let mut command = Command::new("/bin/sh");
    command.args(["-c", "(/bin/sleep 0.4 &); exit 0"]);
    quiet(&mut command);
    if detached {
        command.process_group(0);
    }
    command.status().unwrap();
}
fn churn(detached: bool) {
    let root = tempfile::tempdir().unwrap();
    let marker = root.path().join("idle");
    let mut process = spawn_supervised(root.path(), &marker);
    wait_for(&marker.with_extension("entered"));
    // The leaf fixture lives eight seconds; stay well inside it.
    let until = Instant::now() + Duration::from_secs(4);
    let mut samples = 0;
    while Instant::now() < until {
        churn_once(detached);
        samples += 1;
        assert!(
            process.observe_leader(Duration::from_secs(2)).unwrap(),
            "owner observation lost after {samples} samples of detached={detached} churn"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(samples >= 10, "only {samples} samples");
    let report = process.stop(Duration::from_secs(5)).unwrap();
    assert_eq!(report.cause, StopCause::Requested);
    assert!(report.scope.leader_exited && report.scope.whole_tree_stopped);
}

/// A survivor alone in its process group keeps its session, and the session
/// still holds classified members, so the owned tree keeps its observation.
#[test]
fn native_macos_detached_unrelated_churn_keeps_descendant_proof() {
    churn(true);
}

/// The same shape without `process_group`, as the A/B control: the only
/// difference between the two is which scope carries the evidence.
#[test]
fn native_macos_attached_unrelated_churn_control() {
    churn(false);
}

/// The census is whole-system, so one unclassifiable row used to be refused by
/// every guardian on the host within milliseconds of the others. No unrelated
/// process may stop any owned tree.
#[test]
fn native_macos_one_detached_foreign_orphan_stops_no_guardian() {
    let root = tempfile::tempdir().unwrap();
    let first = root.path().join("first");
    let second = root.path().join("second");
    let mut a = spawn_supervised(root.path(), &first);
    let mut b = spawn_supervised(root.path(), &second);
    wait_for(&first.with_extension("entered"));
    wait_for(&second.with_extension("entered"));
    let until = Instant::now() + Duration::from_secs(3);
    let mut samples = 0;
    while Instant::now() < until {
        churn_once(true);
        samples += 1;
        for (index, process) in [&mut a, &mut b].into_iter().enumerate() {
            assert!(
                process.observe_leader(Duration::from_secs(2)).unwrap(),
                "guardian {index} lost its owner after {samples} samples"
            );
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(samples >= 8, "only {samples} samples");
    for mut process in [a, b] {
        let report = process.stop(Duration::from_secs(5)).unwrap();
        assert_eq!(report.cause, StopCause::Requested);
        assert!(report.scope.leader_exited && report.scope.whole_tree_stopped);
    }
}
