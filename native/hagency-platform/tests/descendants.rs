use hagency_platform::{Launch, OwnedProcess, StopCause, SupervisedProcess};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

fn binary() -> PathBuf {
    env!("CARGO_BIN_EXE_hagency-platform-probe").into()
}
fn launch(root: &Path, mode: &str, marker: &Path) -> Launch {
    let mut environment = BTreeMap::new();
    environment.insert("PATH".into(), "".into());
    if let Some(value) = std::env::var_os("SystemRoot") {
        environment.insert("SystemRoot".into(), value);
    }
    Launch {
        executable: binary(),
        arguments: vec![mode.into(), marker.as_os_str().into()],
        directory: root.into(),
        environment,
        require_crash_containment: false,
    }
}
fn supported(root: &Path, marker: &Path) -> bool {
    if cfg!(target_os = "macos") {
        let mut request = launch(root, "detached-root", marker);
        request.require_crash_containment = true;
        assert!(
            matches!(SupervisedProcess::spawn(&binary(), &request), Err(e) if e.kind() == std::io::ErrorKind::Unsupported)
        );
        assert!(!marker.with_extension("entered").exists());
        assert!(!marker.with_extension("detached").exists());
        return false;
    }
    true
}
fn length(marker: &Path) -> u64 {
    fs::metadata(marker.with_extension("pulse")).map_or(0, |v| v.len())
}
fn ready(marker: &Path) {
    let until = Instant::now() + Duration::from_secs(5);
    while length(marker) < 3 {
        assert!(Instant::now() < until, "detached fixture did not start");
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn stopped(marker: &Path) {
    std::thread::sleep(Duration::from_millis(100));
    let before = length(marker);
    std::thread::sleep(Duration::from_millis(180));
    assert_eq!(length(marker), before, "detached child still writes");
}
#[test]
fn native_descendant_scope() {
    let root = tempfile::tempdir().unwrap();
    let marker = root.path().join("detached");
    if !supported(root.path(), &marker) {
        return;
    }
    let other = root.path().join("unrelated");
    let mut unrelated = OwnedProcess::spawn(&launch(root.path(), "leaf", &other)).unwrap();
    ready(&other);
    let mut process =
        SupervisedProcess::spawn(&binary(), &launch(root.path(), "detached-root", &marker))
            .unwrap();
    ready(&marker);
    assert!(marker.with_extension("middle-exited").is_file());
    assert_eq!(
        fs::read_to_string(marker.with_extension("entered")).unwrap(),
        "detached"
    );
    let report = process.stop(Duration::from_secs(4)).unwrap();
    assert!(report.scope.leader_exited && report.scope.whole_tree_stopped);
    assert_eq!(report.cause, StopCause::Requested);
    stopped(&marker);
    let before = length(&other);
    let until = Instant::now() + Duration::from_secs(1);
    while length(&other) <= before {
        assert!(
            unrelated.is_leader_running().unwrap(),
            "unrelated process was stopped"
        );
        assert!(
            Instant::now() < until,
            "unrelated process made no fresh progress"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    unrelated.stop(Duration::from_secs(2)).unwrap();
}
struct Guard(Child);
impl Drop for Guard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
#[test]
fn native_descendant_owner_loss() {
    let root = tempfile::tempdir().unwrap();
    let marker = root.path().join("lost-owner");
    if !supported(root.path(), &marker) {
        return;
    }
    let request = launch(root.path(), "supervisor-detached-crash", &marker);
    let mut owner = Guard(
        Command::new(binary())
            .args(&request.arguments)
            .current_dir(root.path())
            .env_clear()
            .envs(&request.environment)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let until = Instant::now() + Duration::from_secs(5);
    while owner.0.try_wait().unwrap().is_none() {
        assert!(Instant::now() < until, "controller did not exit");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(owner.0.wait().unwrap().success());
    assert!(marker.with_extension("ready").is_file());
    assert!(marker.with_extension("middle-exited").is_file());
    stopped(&marker);
}
#[test]
fn native_descendant_early_exit() {
    let root = tempfile::tempdir().unwrap();
    let marker = root.path().join("early-root");
    if !supported(root.path(), &marker) {
        return;
    }
    let mut process =
        SupervisedProcess::spawn(&binary(), &launch(root.path(), "detached-early", &marker))
            .unwrap();
    let report = process
        .wait(Duration::from_secs(5))
        .unwrap()
        .expect("native descendant cleanup was not reported");
    assert_eq!(report.cause, StopCause::LeaderExited);
    assert!(report.scope.leader_exited && report.scope.whole_tree_stopped);
    assert!(marker.with_extension("middle-exited").is_file());
    assert_eq!(
        fs::read_to_string(marker.with_extension("entered")).unwrap(),
        "detached"
    );
    stopped(&marker);
}
