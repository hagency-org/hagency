use hagency_platform::{Launch, SupervisedProcess};
use hagency_runtime::{
    codex::{session::Settings, transport::Limits},
    owned::{OwnedSession, StartError},
};
use hagency_runtime::{
    codex::{
        session::{Error, Outcome, Update},
        transport::Error as TransportError,
    },
    owned::Cleanup,
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
use std::{
    fs,
    time::{Duration, Instant},
};

fn binary() -> PathBuf {
    env!("CARGO_BIN_EXE_hagency-runtime-probe").into()
}
fn launch(root: &Path, mode: &str, marker: &Path) -> Launch {
    let mut environment = BTreeMap::new();
    environment.insert("PATH".into(), "".into());
    if let Some(root) = std::env::var_os("SystemRoot") {
        environment.insert("SystemRoot".into(), root);
    }
    Launch {
        executable: binary(),
        arguments: vec!["fake-server".into(), mode.into(), marker.as_os_str().into()],
        directory: root.into(),
        environment,
        require_crash_containment: false,
    }
}
fn settings(root: &Path) -> Settings {
    Settings::new(root.into(), "offline-fixture".into(), "medium".into()).unwrap()
}
fn limits() -> Limits {
    Limits {
        write_timeout_ms: 500,
        event_wait_ms: 1500,
        lifetime_ms: 10_000,
    }
}
fn spawn(root: &Path, mode: &str, marker: &Path) -> OwnedSession {
    OwnedSession::spawn(
        &binary(),
        &launch(root, mode, marker),
        settings(root),
        limits(),
        1500,
    )
    .unwrap()
}
fn wait_pulse(marker: &Path, minimum: u64, deadline: Instant) -> u64 {
    loop {
        let observed = match fs::metadata(marker.with_extension("pulse")) {
            Ok(metadata) => metadata.len(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
            Err(error) => panic!("pulse observation failed: {:?}", error.kind()),
        };
        assert!(
            Instant::now() < deadline,
            "pulse deadline: observed={observed}, required={minimum}"
        );
        if observed >= minimum {
            return observed;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn stopped(marker: &Path) {
    let path = marker.with_extension("pulse");
    std::thread::sleep(Duration::from_millis(80));
    let before = fs::metadata(&path).map_or(0, |m| m.len());
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(fs::metadata(&path).map_or(0, |m| m.len()), before);
}
fn cleanup(runner: &OwnedSession) {
    match runner.cleanup() {
        Cleanup::Observed(report) => {
            assert!(report.scope.leader_exited);
            assert_eq!(
                report.scope.whole_tree_stopped,
                cfg!(any(target_os = "linux", windows))
            );
        }
        other => panic!("fixture cleanup must be observed: {other:?}"),
    }
}
async fn lifecycle(runner: &mut OwnedSession) {
    runner.initialize().await.unwrap();
    assert_eq!(runner.start_thread().await.unwrap(), "owned-thread");
    assert_eq!(
        runner
            .start_turn("user message; never argv $(literal) 中文".into())
            .await
            .unwrap(),
        "owned-turn"
    );
    let mut delta = false;
    for _ in 0..8 {
        match runner.next_update().await.unwrap() {
            Update::TextDelta { delta: text, .. } => {
                assert_eq!(text, "离线管道验证完成");
                delta = true;
            }
            Update::TurnEnded => break,
            _ => {}
        }
    }
    assert!(delta);
    assert!(
        matches!(runner.protocol_outcome(), Some(Outcome::Completed { text }) if text == "离线管道验证完成")
    );
    cleanup(runner);
}

#[tokio::test]
async fn native_owned_runner_admission_failed_spawn_and_guarantees() {
    let root = tempfile::tempdir().unwrap();
    let marker = root.path().join("refused");
    let mut request = launch(root.path(), "keepalive", &marker);
    {
        request.executable = root.path().join("missing-executable.exe");
        assert!(matches!(
            OwnedSession::spawn(&binary(), &request, settings(root.path()), limits(), 1500),
            Err(StartError::Uncertain { .. })
        ));
        #[cfg(unix)]
        {
            request = launch(root.path(), "keepalive", &marker);
            request.require_crash_containment = true;
            assert!(matches!(
                OwnedSession::spawn(&binary(), &request, settings(root.path()), limits(), 1500),
                Err(StartError::Unsupported)
            ));
        }
        #[cfg(windows)]
        {
            let mut positive = launch(root.path(), "keepalive", &root.path().join("job-contained"));
            positive.require_crash_containment = true;
            let mut owned =
                OwnedSession::spawn(&binary(), &positive, settings(root.path()), limits(), 1500)
                    .unwrap();
            owned.stop();
            cleanup(&owned);
        }
        request = launch(root.path(), "keepalive", &marker);
        request.arguments.push("bad\0argument".into());
        assert!(
            OwnedSession::spawn(&binary(), &request, settings(root.path()), limits(), 1500)
                .is_err()
        );
        request = launch(root.path(), "keepalive", &marker);
        assert!(matches!(
            OwnedSession::spawn(&binary(), &request, settings(root.path()), limits(), 0),
            Err(StartError::Settings)
        ));
    }
    assert!(!marker.with_extension("entered").exists());
}

#[tokio::test]
async fn native_owned_runner_lifecycle_real_owned_pipes() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("管道 工作区");
    fs::create_dir(&directory).unwrap();
    let marker = directory.join("normal");
    // An unrelated socket must remain in this host only; the platform guardian
    // suite separately covers descriptors whose CLOEXEC bit was cleared.
    #[cfg(unix)]
    let _unrelated = std::os::unix::net::UnixStream::pair().unwrap();
    let mut runner = spawn(&directory, "normal", &marker);
    let pid = runner.id();
    assert!(pid > 1);
    lifecycle(&mut runner).await;
    #[cfg(unix)]
    assert_eq!(
        fs::read_to_string(marker.with_extension("sockets")).unwrap(),
        "0"
    );
    let requests = fs::read_to_string(marker.with_extension("requests")).unwrap();
    let methods: Vec<_> = requests
        .lines()
        .map(|line| {
            serde_json::from_str::<serde_json::Value>(line).unwrap()["method"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    assert_eq!(
        methods,
        ["initialize", "initialized", "thread/start", "turn/start"]
    );
    assert!(requests.contains("never argv"));
    assert_eq!(runner.id(), pid);
    stopped(&marker);
    assert_eq!(
        runner.start_turn("replay".into()).await.err(),
        Some(Error::State)
    );
}

#[tokio::test]
async fn native_owned_runner_failures_eof_timeout_and_noisy_stderr() {
    let root = tempfile::tempdir().unwrap();
    let marker = root.path().join("eof");
    let mut runner = spawn(root.path(), "eof", &marker);
    assert_eq!(
        runner.initialize().await.err(),
        Some(Error::Transport(TransportError::PeerEof))
    );
    assert!(matches!(
        runner.protocol_outcome(),
        Some(Outcome::Unknown { .. })
    ));
    cleanup(&runner);
    let marker = root.path().join("timeout");
    let mut runner = spawn(root.path(), "silent", &marker);
    let since = Instant::now();
    assert_eq!(
        runner.initialize().await.err(),
        Some(Error::Transport(TransportError::Timeout))
    );
    assert!(since.elapsed() < Duration::from_secs(6));
    cleanup(&runner);
    stopped(&marker);
    let marker = root.path().join("noisy");
    let mut runner = spawn(root.path(), "noisy", &marker);
    lifecycle(&mut runner).await;
    let stderr = runner.stderr_snapshot();
    // This is bytes observed, not bytes produced: terminal stdout closes the
    // disposable streams and need not drain remaining kernel stderr buffers.
    // More than one retained tail proves active drainage under pipe pressure.
    assert!(stderr.total_bytes > 16 * 1024 && stderr.total_bytes <= 256 * 1024);
    assert_eq!(stderr.tail.len(), 16 * 1024);
    assert!(stderr.tail.iter().all(|&byte| byte == b'e'));
    cleanup(&runner);
}

#[tokio::test]
async fn native_owned_runner_failures_mid_write_cancel_retains_cleanup() {
    let root = tempfile::tempdir().unwrap();
    let marker = root.path().join("blocked");
    let mut runner = spawn(root.path(), "blocked", &marker);
    runner.initialize().await.unwrap();
    runner.start_thread().await.unwrap();
    let since = Instant::now();
    assert!(
        tokio::time::timeout(
            Duration::from_millis(40),
            runner.start_turn("x".repeat(65536))
        )
        .await
        .is_err()
    );
    assert!(since.elapsed() < Duration::from_secs(5));
    assert!(matches!(
        runner.protocol_outcome(),
        Some(Outcome::Unknown {
            reason: Error::Cancelled
        })
    ));
    let unconfirmed = runner
        .transport_termination()
        .unwrap()
        .unconfirmed_write
        .as_ref()
        .unwrap();
    assert!(unconfirmed.accepted_bytes < unconfirmed.total_bytes);
    #[cfg(unix)]
    assert!(unconfirmed.accepted_bytes > 0);
    // Windows reports only confirmed writes. The peer marker proves that even
    // an accepted_bytes lower bound of zero may already have partial effects.
    #[cfg(windows)]
    assert_eq!(
        fs::metadata(marker.with_extension("partial"))
            .unwrap()
            .len(),
        4096
    );
    cleanup(&runner);
    stopped(&marker);
}

#[test]
fn native_owned_runner_custody_stream_close_does_not_stop_child() {
    let root = tempfile::tempdir().unwrap();
    let marker = root.path().join("keepalive");
    let (mut owner, pipes) =
        SupervisedProcess::spawn_piped(&binary(), &launch(root.path(), "keepalive", &marker))
            .unwrap();
    drop(pipes);
    let until = Instant::now() + Duration::from_secs(3);
    wait_pulse(&marker, 3, until);
    assert!(owner.wait(Duration::from_millis(40)).unwrap().is_none());
    let before = fs::metadata(marker.with_extension("pulse")).unwrap().len();
    // Sleep is not a scheduling acknowledgement from the other process. Keep
    // the original absolute deadline and require an actual further write.
    assert!(wait_pulse(&marker, before + 1, until) > before);
    let report = owner.stop(Duration::from_secs(3)).unwrap();
    assert!(report.scope.leader_exited);
    assert_eq!(
        report.scope.whole_tree_stopped,
        cfg!(any(target_os = "linux", windows))
    );
    stopped(&marker);
}

#[test]
fn native_owned_runner_custody_gated_progress() {
    let root = tempfile::tempdir().unwrap();
    let marker = root.path().join("gated");
    let (mut owner, pipes) =
        SupervisedProcess::spawn_piped(&binary(), &launch(root.path(), "gated-keepalive", &marker))
            .unwrap();
    drop(pipes);
    let until = Instant::now() + Duration::from_secs(3);
    let before = wait_pulse(&marker, 3, until);
    assert_eq!(before, 3);
    assert!(owner.wait(Duration::from_millis(40)).unwrap().is_none());
    // The real child is alive but its next write is held behind an explicit
    // gate. Elapsed time alone cannot establish heartbeat progress.
    assert_eq!(
        fs::metadata(marker.with_extension("pulse")).unwrap().len(),
        before
    );
    assert!(!marker.with_extension("release").exists());
    fs::write(marker.with_extension("release"), b"go").unwrap();
    assert!(wait_pulse(&marker, before + 1, until) > before);
    let report = owner.stop(Duration::from_secs(3)).unwrap();
    assert!(report.scope.leader_exited);
    assert_eq!(
        report.scope.whole_tree_stopped,
        cfg!(any(target_os = "linux", windows))
    );
    stopped(&marker);
}

#[tokio::test]
async fn native_owned_runner_custody_descendants_follow_existing_scope_guarantees() {
    let root = tempfile::tempdir().unwrap();
    let modes = if cfg!(any(target_os = "linux", windows)) {
        vec!["descendant", "detached"]
    } else {
        vec!["descendant"]
    };
    for mode in modes {
        let marker = root.path().join(mode);
        let mut runner = spawn(root.path(), mode, &marker);
        lifecycle(&mut runner).await;
        cleanup(&runner);
        let child_marker = marker.with_file_name(format!("{mode}-child"));
        assert!(
            fs::metadata(child_marker.with_extension("pulse"))
                .unwrap()
                .len()
                >= 2
        );
        stopped(&child_marker);
    }
}

#[cfg(windows)]
#[test]
fn native_windows_owned_runner_owner_exit_closes_job_without_drop() {
    use std::process::{Child, Command, Stdio};
    struct Guard(Child);
    impl Drop for Guard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let root = tempfile::tempdir().unwrap();
    let marker = root.path().join("owner");
    let mut controller = Guard(
        Command::new(binary())
            .arg("owner-crash")
            .arg(&marker)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = controller.0.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        assert!(Instant::now() < until, "owner fixture did not exit");
        std::thread::sleep(Duration::from_millis(10));
    }
    let descendant = marker.with_file_name("descendant-child");
    assert!(
        fs::metadata(descendant.with_extension("pulse"))
            .unwrap()
            .len()
            >= 3
    );
    stopped(&descendant);
}

#[cfg(windows)]
#[tokio::test]
async fn native_windows_owned_runner_job_refuses_breakaway() {
    let root = tempfile::tempdir().unwrap();
    let marker = root.path().join("breakaway");
    let mut runner = spawn(root.path(), "breakaway", &marker);
    lifecycle(&mut runner).await;
    assert_eq!(
        fs::read(marker.with_extension("breakaway")).unwrap(),
        b"access-denied"
    );
    assert!(!root.path().join("escape.pulse").exists());
}

#[tokio::test]
async fn native_owned_control_drop_stops_original_process() {
    use hagency_runtime::codex::session::ApprovalControlPolicy;
    let root = tempfile::tempdir().unwrap();
    let marker = root.path().join("control-drop");
    let mut runner = spawn(root.path(), "usage-gate", &marker);
    let original = runner.id();
    runner.initialize().await.unwrap();
    runner.start_thread().await.unwrap();
    runner
        .start_turn("offline control custody".into())
        .await
        .unwrap();
    runner
        .enable_approval_control(ApprovalControlPolicy {
            owner_wait_ms: 2000,
            response_reserve_ms: 500,
        })
        .unwrap();
    let until = tokio::time::Instant::now() + Duration::from_secs(2);
    while !marker.with_extension("usage-ready").is_file() {
        assert!(
            tokio::time::Instant::now() < until,
            "original child gate missing"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let mut control = Box::pin(std::future::pending::<()>());
    assert!(
        tokio::time::timeout(
            Duration::from_millis(30),
            runner.next_observed_or_control(control.as_mut())
        )
        .await
        .is_err()
    );
    assert_eq!(runner.id(), original);
    cleanup(&runner);
    assert!(matches!(
        runner.protocol_outcome(),
        Some(Outcome::Unknown { .. })
    ));
    assert!(!marker.with_extension("usage-release").exists());
    assert!(
        runner
            .next_observed_or_control(control.as_mut())
            .await
            .is_err()
    );
}
