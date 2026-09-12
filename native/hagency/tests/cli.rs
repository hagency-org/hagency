use std::{
    fs,
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::Path,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

struct Running(Child);
impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn launch(state: &Path, address: SocketAddr) -> Running {
    let child = Command::new(env!("CARGO_BIN_EXE_hagency"))
        .args(["serve", "--state-dir"])
        .arg(state)
        .args(["--listen", &address.to_string()])
        .env("PATH", "")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut running = Running(child);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = running.0.try_wait().unwrap() {
            let mut error = String::new();
            if let Some(mut stderr) = running.0.stderr.take() {
                stderr.read_to_string(&mut error).unwrap();
            }
            panic!("native service exited before health ({status}): {error}");
        }
        if let Ok(mut stream) = TcpStream::connect_timeout(&address, Duration::from_millis(100)) {
            stream
                .set_read_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            write!(
                stream,
                "GET /health HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
            )
            .unwrap();
            let mut response = String::new();
            stream.read_to_string(&mut response).unwrap();
            if response.starts_with("HTTP/1.1 200") {
                return running;
            }
        }
        assert!(
            Instant::now() < deadline,
            "native service startup timed out"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}
fn submit(address: SocketAddr, token: &str) -> String {
    let mut stream = TcpStream::connect(address).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let body = r#"{"binding":"fixture","generation":1,"id":"restart_request","lane":"work","kind":"request","payload":{"name":"小白"}}"#;
    write!(stream, "POST /api/native/v1/custody HTTP/1.1\r\nHost: {address}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    assert!(
        response.starts_with("HTTP/1.1 202"),
        "custody was not accepted"
    );
    let body = response.split("\r\n\r\n").nth(1).unwrap();
    serde_json::from_str::<serde_json::Value>(body).expect("native JSON response");
    body.to_owned()
}
fn resource_call(address: SocketAddr, token: &str, create: bool) -> serde_json::Value {
    let mut stream = TcpStream::connect(address).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let body = if create {
        r#"{"presetId":"restart_pool","seatId":"fixture_seat","framework":"codex","model":"gpt-5.6-sol","reasoning":"medium","ceiling":{"tokens":100}}"#
    } else {
        ""
    };
    let method = if create { "POST" } else { "GET" };
    write!(stream,"{method} /api/native/v1/resources HTTP/1.1\r\nHost: {address}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "native domain resource request failed"
    );
    serde_json::from_str(response.split("\r\n\r\n").nth(1).unwrap()).unwrap()
}
#[test]
fn native_binary_survives_crash_without_node() {
    let directory = tempfile::tempdir().unwrap();
    let state = directory.path().join("中文 state");
    let init = Command::new(env!("CARGO_BIN_EXE_hagency"))
        .args(["init", "--state-dir"])
        .arg(&state)
        .env("PATH", "")
        .output()
        .unwrap();
    assert!(
        init.status.success(),
        "init failed: {}",
        String::from_utf8_lossy(&init.stderr)
    );
    // Fresh Unicode initialization remains an actual binary/database check,
    // independent of the guardian's bounded terminal-observation fixture.
    assert!(state.join("operator.token").is_file());
    assert!(state.join("domain.sqlite3").is_file());
    let token = fs::read_to_string(state.join("operator.token")).unwrap();
    assert!(!String::from_utf8_lossy(&init.stdout).contains(&token));
    let before = fs::read(state.join("operator.token")).unwrap();
    assert!(
        !Command::new(env!("CARGO_BIN_EXE_hagency"))
            .args(["init", "--state-dir"])
            .arg(&state)
            .output()
            .unwrap()
            .status
            .success()
    );
    assert_eq!(fs::read(state.join("operator.token")).unwrap(), before);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let running = launch(&state, address);
    let receipt = submit(address, &token);
    let resource = resource_call(address, &token, true);
    drop(running); // Unclean process loss, not an in-memory reopen.
    let _restarted = launch(&state, address);
    assert_eq!(submit(address, &token), receipt);
    assert_eq!(
        resource_call(address, &token, false),
        serde_json::json!([resource])
    );
}

#[tokio::test]
async fn native_guardian_cli_entry() {
    use hagency_platform::{Launch, StopCause, SupervisedProcess};
    use tokio::io::AsyncReadExt;
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("监管 CLI 工作目录");
    fs::create_dir(&directory).unwrap();
    let executable = std::path::PathBuf::from(env!("CARGO_BIN_EXE_hagency"));
    let mut environment = std::collections::BTreeMap::new();
    environment.insert("PATH".into(), "".into());
    if let Some(value) = std::env::var_os("SystemRoot") {
        environment.insert("SystemRoot".into(), value);
    }
    let (mut process, pipes) = SupervisedProcess::spawn_piped(
        &executable,
        &Launch {
            executable: executable.clone(),
            // Exercise native CLI dispatch/exit, without coupling the guardian
            // deadline to schema initialization and filesystem throughput.
            arguments: vec!["--version".into()],
            directory,
            environment,
            require_crash_containment: cfg!(windows),
        },
    )
    .unwrap();
    #[cfg(unix)]
    let (stdout, stderr) = {
        let (stdin, stdout, stderr) = pipes.into_parts();
        drop(stdin);
        (
            tokio::net::unix::pipe::Receiver::from_owned_fd(stdout).unwrap(),
            tokio::net::unix::pipe::Receiver::from_owned_fd(stderr).unwrap(),
        )
    };
    #[cfg(windows)]
    let (stdout, stderr) = {
        let (stdin, stdout, stderr) = pipes.into_async_parts().unwrap();
        drop(stdin);
        (stdout, stderr)
    };
    let report = process
        .wait(Duration::from_secs(5))
        .unwrap()
        .expect("native guardian did not report leader exit");
    assert_eq!(report.cause, StopCause::LeaderExited);
    assert!(report.scope.leader_exited);
    assert_eq!(
        report.scope.whole_tree_stopped,
        cfg!(any(windows, target_os = "linux"))
    );
    // StopReport has no exit code: exact version bytes plus stderr EOF prove
    // that the actual CLI ran, rather than accepting any leader termination.
    let mut output = Vec::new();
    tokio::time::timeout(
        Duration::from_secs(2),
        stdout.take(256).read_to_end(&mut output),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(
        output,
        format!("hagency {}\n", env!("CARGO_PKG_VERSION")).as_bytes()
    );
    let mut error = Vec::new();
    tokio::time::timeout(
        Duration::from_secs(2),
        stderr.take(256).read_to_end(&mut error),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(error.is_empty(), "unexpected CLI stderr: {error:?}");
    assert_eq!(
        process.wait(Duration::from_millis(1)).unwrap(),
        Some(report)
    );
}

#[test]
fn native_account_cli() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    let invoke = |args: &[&str]| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_hagency"));
        command
            .args(args)
            .arg("--state-dir")
            .arg(&state)
            .env("PATH", "")
            .env("HOME", "/untrusted-fixture-home")
            .env("CODEX_HOME", "/untrusted-fixture-codex")
            .env("OPENAI_API_KEY", "offline-fixture-key");
        command.output().unwrap()
    };
    assert!(invoke(&["init"]).status.success());
    let prepared = invoke(&["account", "prepare"]);
    assert!(
        prepared.status.success(),
        "{}",
        String::from_utf8_lossy(&prepared.stderr)
    );
    let choices: serde_json::Value = serde_json::from_slice(&prepared.stdout).unwrap();
    let id = choices[0]["id"].as_str().unwrap();
    assert_eq!(choices[0]["authentication"], "unknown");
    assert!(choices[0]["quota"].is_null());
    assert!(fs::read_dir(state.join(id)).unwrap().next().is_none());
    let inspect = invoke(&["account", "inspect"]);
    assert!(inspect.status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&inspect.stdout).unwrap(),
        choices
    );
    let owned = hagency_store::Repository::open(&state).unwrap();
    let busy = invoke(&["account", "prepare"]);
    assert!(!busy.status.success());
    drop(owned);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&invoke(&["account", "inspect"]).stdout)
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let retired = invoke(&["account", "retire", "--id", id]);
    assert!(
        retired.status.success(),
        "{}",
        String::from_utf8_lossy(&retired.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&retired.stdout).unwrap()[0]["state"],
        "retired"
    );
    assert!(state.join(id).is_dir());
    let output = String::from_utf8(prepared.stdout).unwrap();
    for private in [
        "seat_native_",
        "fixture-key",
        "CODEX_HOME",
        state.to_str().unwrap(),
    ] {
        assert!(!output.contains(private));
    }
}
