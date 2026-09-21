//! A process the tracker cannot place is not owned, and that is all it is.
//!
//! A survivor whose unseen parent opened a session of its own shares neither a
//! session nor a group with anything classified, and nothing in a census can
//! decide whether it descends from the owned leader. The tracker used to refuse
//! on it, and because the census is whole-system that ended EVERY guardian on the
//! host: live, an hourly browser updater, an operator's `ssh` and the other
//! agent's own shell command each stopped a healthy running agent that way. The
//! retained tracker never had that rule: it tracks what it can prove is its own
//! and ignores the rest. So does this one now. The process is not adopted, is
//! never signalled, and ends nothing.
#![cfg(target_os = "macos")]
use hagency_platform::{Launch, StopCause, SupervisedProcess};
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
fn native_macos_unplaceable_orphan_neither_stops_nor_joins_the_tree() {
    let root = tempfile::tempdir().unwrap();
    let marker = root.path().join("idle");
    let mut process = spawn_supervised(root.path(), &marker);
    let entered = marker.with_extension("entered");
    let until = Instant::now() + Duration::from_secs(10);
    while !entered.exists() {
        assert!(Instant::now() < until, "fixture did not start");
        std::thread::sleep(Duration::from_millis(10));
    }
    let until = Instant::now() + Duration::from_secs(3);
    let mut samples = 0;
    let mut last = None;
    while Instant::now() < until {
        // The middle calls setsid, spawns a subshell that forks a survivor, and
        // exits before the survivor is born. Nothing classified remains in that
        // session or that group, in the same coalition as the leader.
        let orphan = root.path().join(format!("orphan{samples}"));
        quiet(&mut Command::new(binary()))
            .args(["foreign-session-middle".as_ref(), orphan.as_os_str()])
            .status()
            .unwrap();
        last = Some(orphan.with_extension("survivor"));
        samples += 1;
        assert!(
            process.observe_leader(Duration::from_secs(2)).unwrap(),
            "an unplaceable process ended the observation at sample {samples}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(samples >= 5, "only {samples} samples");
    // The newest survivor is still inside its sleep. Stopping the owned tree
    // must prove that tree stopped and must leave this process alone: a
    // guardian signals only what it proved is its own.
    let survivor = last.unwrap();
    let until = Instant::now() + Duration::from_secs(2);
    let pid: i32 = loop {
        if let Ok(text) = std::fs::read_to_string(&survivor)
            && let Ok(pid) = text.trim().parse()
        {
            break pid;
        }
        assert!(
            Instant::now() < until,
            "the survivor never recorded its pid"
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    let report = process.stop(Duration::from_secs(5)).unwrap();
    assert_eq!(report.cause, StopCause::Requested);
    assert_eq!(report.detail, None);
    assert!(report.scope.leader_exited && report.scope.signals_accepted);
    assert!(report.scope.whole_tree_stopped);
    // `kill -0` only asks whether the process exists.
    let alive = |pid: i32| {
        quiet(&mut Command::new("/bin/kill"))
            .args(["-0", &pid.to_string()])
            .status()
            .unwrap()
            .success()
    };
    assert!(
        alive(pid),
        "the unplaceable survivor was signalled by a guardian that never owned it"
    );
    // This test started that sleep and ends it.
    quiet(&mut Command::new("/bin/kill"))
        .args(["-KILL", &pid.to_string()])
        .status()
        .unwrap();
}
