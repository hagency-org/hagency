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
    environment.insert("HAGENCY_PROBE_ALLOWED".into(), "显式 value".into());
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
fn length(marker: &Path) -> u64 {
    fs::metadata(marker.with_extension("pulse")).map_or(0, |v| v.len())
}
fn ready(marker: &Path) {
    let until = Instant::now() + Duration::from_secs(5);
    while length(marker) < 3 {
        assert!(Instant::now() < until, "fixture did not become ready");
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn stopped(marker: &Path) {
    std::thread::sleep(Duration::from_millis(120));
    let before = length(marker);
    std::thread::sleep(Duration::from_millis(180));
    assert_eq!(length(marker), before, "owned child still writing");
}
struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn wait(child: &mut Child) {
    let until = Instant::now() + Duration::from_secs(5);
    while child.try_wait().unwrap().is_none() {
        assert!(Instant::now() < until, "native child did not exit");
        std::thread::sleep(Duration::from_millis(10));
    }
}
#[test]
fn native_guardian_start_stop() {
    #[cfg(unix)]
    let _inherited = {
        let pair = std::os::unix::net::UnixStream::pair().unwrap();
        // Model an embedding host/CI runner with unrelated inheritable handles.
        for fd in [&pair.0, &pair.1] {
            rustix::io::fcntl_setfd(fd, rustix::io::FdFlags::empty()).unwrap();
        }
        pair
    };
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("监管 工作区");
    fs::create_dir(&directory).unwrap();
    let marker = directory.join("owned");
    let other = directory.join("other");
    let mut unrelated = OwnedProcess::spawn(&launch(&directory, "leaf", &other)).unwrap();
    ready(&other);
    let mut request = launch(&directory, "leader", &marker);
    let arguments = [
        "",
        "汉字 spaces",
        "\"quoted\"",
        "trailing\\",
        "line\nnext",
        "$() `literal`",
    ];
    request
        .arguments
        .extend(arguments.iter().map(|s| (*s).into()));
    let mut owned = SupervisedProcess::spawn(&binary(), &request).unwrap();
    assert_ne!(owned.id(), unrelated.id());
    ready(&marker);
    #[cfg(unix)]
    assert_eq!(
        fs::read_to_string(marker.with_extension("sockets")).unwrap(),
        "0",
        "work must not inherit a guardian channel"
    );
    assert_eq!(
        fs::read_to_string(marker.with_extension("arguments")).unwrap(),
        arguments.join("\0")
    );
    assert_eq!(
        fs::read_to_string(marker.with_extension("environment")).unwrap(),
        "true:true:显式 value"
    );
    assert!(owned.wait(Duration::from_millis(40)).unwrap().is_none());
    let report = owned.stop(Duration::from_secs(3)).unwrap();
    assert_eq!(report.cause, StopCause::Requested);
    assert!(report.scope.leader_exited);
    assert_eq!(
        report.scope.whole_tree_stopped,
        cfg!(any(windows, target_os = "linux"))
    );
    assert_eq!(owned.stop(Duration::from_secs(1)).unwrap(), report);
    #[cfg(unix)]
    for fd in [&_inherited.0, &_inherited.1] {
        assert!(
            rustix::io::fcntl_getfd(fd).unwrap().is_empty(),
            "sealing must not change the host descriptor table"
        );
    }
    stopped(&marker);
    let before = length(&other);
    let until = Instant::now() + Duration::from_secs(1);
    while length(&other) <= before {
        assert!(
            unrelated.is_leader_running().unwrap(),
            "cancellation stopped an unrelated process"
        );
        assert!(
            Instant::now() < until,
            "unrelated live process did not make fresh progress"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    unrelated.stop(Duration::from_secs(2)).unwrap();
    let marker = directory.join("drop");
    let owned =
        SupervisedProcess::spawn(&binary(), &launch(&directory, "leader", &marker)).unwrap();
    ready(&marker);
    drop(owned);
    stopped(&marker);
}
#[test]
fn native_guardian_owner_loss() {
    let root = tempfile::tempdir().unwrap();
    let marker = root.path().join("owner-loss");
    let request = launch(root.path(), "supervisor-crash", &marker);
    let mut owner = ChildGuard(
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
    wait(&mut owner.0);
    assert!(owner.0.wait().unwrap().success());
    assert!(marker.with_extension("ready").is_file());
    stopped(&marker);
}
#[test]
fn native_guardian_early_exit() {
    let root = tempfile::tempdir().unwrap();
    let marker = root.path().join("early");
    let mut owned =
        SupervisedProcess::spawn(&binary(), &launch(root.path(), "early", &marker)).unwrap();
    let report = owned
        .wait(Duration::from_secs(4))
        .unwrap()
        .expect("supervisor must observe leader exit");
    assert_eq!(report.cause, StopCause::LeaderExited);
    assert!(report.scope.leader_exited);
    assert_eq!(
        report.scope.whole_tree_stopped,
        cfg!(any(windows, target_os = "linux"))
    );
    assert!(marker.with_extension("child").is_file());
    stopped(&marker);
}
#[test]
fn native_guardian_admission() {
    let root = tempfile::tempdir().unwrap();
    let marker = root.path().join("refused");
    let mut request = launch(root.path(), "leaf", &marker);
    request.executable = root.path().join(if cfg!(windows) {
        "missing.exe"
    } else {
        "missing"
    });
    assert!(SupervisedProcess::spawn(&binary(), &request).is_err());
    request = launch(root.path(), "leaf", &marker);
    request.arguments.push("bad\0argument".into());
    assert!(SupervisedProcess::spawn(&binary(), &request).is_err());
    assert_eq!(length(&marker), 0);
    #[cfg(unix)]
    unix_admission(root.path(), &marker);
    #[cfg(windows)]
    {
        // Windows has no wire admission; a kernel job is assigned atomically.
        request = launch(root.path(), "leaf", &marker);
        request.require_crash_containment = true;
        let mut owned = SupervisedProcess::spawn(&binary(), &request).unwrap();
        ready(&marker);
        assert!(
            owned
                .stop(Duration::from_secs(2))
                .unwrap()
                .scope
                .whole_tree_stopped
        );
    }
}

#[cfg(unix)]
fn unix_admission(root: &Path, marker: &Path) {
    use std::{
        io::{Read, Write},
        os::{fd::OwnedFd, unix::net::UnixStream},
    };
    fn peer() -> (UnixStream, ChildGuard) {
        let (owner, child) = UnixStream::pair().unwrap();
        owner
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        owner
            .set_write_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let fd: OwnedFd = child.into();
        let child = ChildGuard(
            Command::new(binary())
                .arg("guardian")
                .env_clear()
                .env("PATH", "")
                .stdin(Stdio::from(fd))
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        (owner, child)
    }
    fn send(peer: &mut UnixStream, value: &serde_json::Value) {
        let bytes = serde_json::to_vec(value).unwrap();
        peer.write_all(&(bytes.len() as u32).to_be_bytes()).unwrap();
        peer.write_all(&bytes).unwrap();
    }
    // Invalid sequence, oversized length, and stalled partial frame are bounded.
    for frame in [
        b"\0\0\0\x0f{\"kind\":\"stop\"}".as_slice(),
        &[0xff; 4],
        &[0, 0],
    ] {
        let (mut socket, mut child) = peer();
        socket.write_all(frame).unwrap();
        wait(&mut child.0);
        assert!(!child.0.wait().unwrap().success());
    }
    let (mut socket, mut child) = peer();
    let request = launch(root, "leaf", marker);
    send(
        &mut socket,
        &serde_json::json!({"kind":"prepare", "version":1, "launch":{
            "executable":request.executable.as_os_str(), "arguments":request.arguments,
            "directory":request.directory.as_os_str(), "environment":request.environment.into_iter().collect::<Vec<_>>(),
            "require_crash_containment":false
        }}),
    );
    let mut header = [0u8; 4];
    socket.read_exact(&mut header).unwrap();
    let mut body = vec![0; u32::from_be_bytes(header) as usize];
    assert!(body.len() < 1024);
    socket.read_exact(&mut body).unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&body).unwrap(),
        serde_json::json!({"kind":"prepared","version":1})
    );
    // Admission alone cannot start work. Losing the owner before Start is safe.
    drop(socket);
    wait(&mut child.0);
    assert_eq!(length(marker), 0);
    // A malformed command after launch must cancel the scope, even while a
    // partial frame is being held open. The fixture cannot keep the channel.
    let marker = root.join("invalid-active");
    let (mut socket, mut child) = peer();
    let request = launch(root, "leader", &marker);
    send(
        &mut socket,
        &serde_json::json!({"kind":"prepare", "version":1, "launch":{
            "executable":request.executable.as_os_str(), "arguments":request.arguments,
            "directory":request.directory.as_os_str(), "environment":request.environment.into_iter().collect::<Vec<_>>(),
            "require_crash_containment":false
        }}),
    );
    fn reply(socket: &mut UnixStream) -> serde_json::Value {
        let mut header = [0u8; 4];
        socket.read_exact(&mut header).unwrap();
        let length = u32::from_be_bytes(header) as usize;
        assert!(length < 1024);
        let mut body = vec![0; length];
        socket.read_exact(&mut body).unwrap();
        serde_json::from_slice(&body).unwrap()
    }
    assert_eq!(reply(&mut socket)["kind"], "prepared");
    send(&mut socket, &serde_json::json!({"kind":"start"}));
    assert_eq!(reply(&mut socket)["kind"], "started");
    ready(&marker);
    socket.write_all(&[0, 0]).unwrap();
    let report = reply(&mut socket);
    assert_eq!(report["kind"], "stopped");
    assert_eq!(report["cause"], "protocol_failure");
    assert_eq!(report["whole_tree_stopped"], cfg!(target_os = "linux"));
    wait(&mut child.0);
    stopped(&marker);
    let mut request = launch(root, "leaf", &marker);
    request.require_crash_containment = true;
    assert!(
        matches!(SupervisedProcess::spawn(&binary(), &request), Err(e) if e.kind() == std::io::ErrorKind::Unsupported)
    );
}
