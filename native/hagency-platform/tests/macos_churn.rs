//! macOS descendant tracking under process churn. A parent that exits between
//! two censuses is ordinary on a working machine; it must neither cost the
//! owned tree its stop proof nor let an owned survivor escape.
#![cfg(target_os = "macos")]
use hagency_platform::{Launch, SupervisedProcess};
use std::{
    collections::BTreeMap,
    fs,
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
fn alive(pid: &str) -> bool {
    quiet(Command::new("/bin/kill").args(["-0", pid]))
        .status()
        .is_ok_and(|status| status.success())
}
fn wait_for(path: &Path) {
    let until = Instant::now() + Duration::from_secs(5);
    while !path.exists() {
        assert!(Instant::now() < until, "fixture did not write {path:?}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// The warm idle loop observes its owner every 100 ms. Unrelated processes whose
/// parent no census saw must not end that observation or the final stop proof.
#[test]
fn native_macos_unrelated_churn_keeps_descendant_proof() {
    let root = tempfile::tempdir().unwrap();
    let marker = root.path().join("idle");
    let mut process =
        SupervisedProcess::spawn(&binary(), &launch(root.path(), "leaf", &marker)).unwrap();
    wait_for(&marker.with_extension("entered"));
    // The leaf fixture lives eight seconds; stay well inside it.
    let until = Instant::now() + Duration::from_secs(4);
    let mut samples = 0;
    while Instant::now() < until {
        // The subshell forks `sleep` and exits at once: an unrelated survivor
        // whose parent is gone before the guardian's next census.
        quiet(Command::new("/bin/sh").args(["-c", "(/bin/sleep 0.4 &); exit 0"]))
            .status()
            .unwrap();
        samples += 1;
        assert!(
            process.observe_leader(Duration::from_secs(1)).unwrap(),
            "owner observation lost after {samples} samples of unrelated churn"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(samples >= 10, "only {samples} samples");
    let report = process.stop(Duration::from_secs(4)).unwrap();
    assert!(report.scope.leader_exited && report.scope.whole_tree_stopped);
}

/// The same shape inside the owned tree: the survivor's parent is never seen,
/// but it shares the leader's process group, so it is owned and is stopped.
#[test]
fn native_macos_unseen_parent_in_owned_group_is_stopped() {
    let root = tempfile::tempdir().unwrap();
    let marker = root.path().join("unseen");
    let mut process =
        SupervisedProcess::spawn(&binary(), &launch(root.path(), "unseen-middle", &marker))
            .unwrap();
    wait_for(&marker.with_extension("entered"));
    let survivor = fs::read_to_string(marker.with_extension("survivor")).unwrap();
    let survivor = survivor.trim();
    assert!(survivor.parse::<u32>().is_ok_and(|pid| pid > 1));
    assert!(alive(survivor), "survivor never ran");
    let until = Instant::now() + Duration::from_secs(2);
    while Instant::now() < until {
        assert!(process.observe_leader(Duration::from_secs(1)).unwrap());
        std::thread::sleep(Duration::from_millis(100));
    }
    let report = process.stop(Duration::from_secs(4)).unwrap();
    assert!(report.scope.leader_exited && report.scope.whole_tree_stopped);
    let until = Instant::now() + Duration::from_secs(2);
    while alive(survivor) {
        assert!(
            Instant::now() < until,
            "owned survivor {survivor} outlived the stop"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}
