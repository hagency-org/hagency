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

/// Brief 22: the inspection subcommands are clients of the operator routes —
/// the `--json` output is the route's body verbatim, and the table's columns
/// are the route's own keys. The alerts envelope carries the read clock
/// (`at_ms`), so its two fetches differ in that field alone; the rows are
/// compared parsed, the other two commands byte-for-byte.
fn operator_get(address: SocketAddr, token: &str, path: &str) -> String {
    let mut stream = TcpStream::connect(address).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: {address}\r\nAuthorization: Bearer {token}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "operator read failed: {path}"
    );
    response.split("\r\n\r\n").nth(1).unwrap().to_owned()
}

fn inspect(state: &Path, address: SocketAddr, verb: &str, extra: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_hagency"))
        .arg(verb)
        .arg("--state-dir")
        .arg(state)
        .arg("--listen")
        .arg(address.to_string())
        .args(extra)
        .env("PATH", "")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap()
}

#[test]
fn native_cli_inspection_matches_operator_routes() {
    let directory = tempfile::tempdir().unwrap();
    let state = directory.path().join("inspect state");
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
    let token = fs::read_to_string(state.join("operator.token")).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let running = launch(&state, address);
    // One catalog row so the table and the passthrough are non-empty.
    let _resource = resource_call(address, &token, true);

    // Resources: the passthrough equals the route body byte-for-byte.
    let route = operator_get(address, &token, "/api/native/v1/resources?limit=100");
    let output = inspect(&state, address, "resources", &["--json"]);
    assert!(
        output.status.success(),
        "resources --json failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        route.trim(),
        "the passthrough must equal the route body"
    );
    // E2 of the CLI review: the table is compared CELL BY CELL against the
    // route's own values, and the header is EXACT per column (order and
    // membership — `contains("id")` would accept `resource_id`).
    let output = inspect(&state, address, "resources", &[]);
    assert!(output.status.success());
    let table = String::from_utf8_lossy(&output.stdout);
    let rows: Vec<serde_json::Value> = serde_json::from_str(route.trim()).unwrap();
    let header: Vec<&str> = table.lines().next().unwrap().split_whitespace().collect();
    assert_eq!(
        header,
        vec!["id", "framework", "model", "tier", "ceiling"],
        "the header row is exactly the route's keys, in order"
    );
    assert_eq!(
        table.lines().count(),
        rows.len() + 1,
        "one header plus one line per route row"
    );
    // The expected cell, computed the way the renderer computes it: null
    // renders "-", a string renders itself, anything else its JSON text.
    let expected = |row: &serde_json::Value, key: &str| -> String {
        match &row[key] {
            serde_json::Value::Null => "-".into(),
            serde_json::Value::String(text) => text.clone(),
            other => other.to_string(),
        }
    };
    for (index, row) in rows.iter().enumerate() {
        let line = table.lines().nth(index + 1).unwrap();
        for column in ["id", "framework", "model", "tier", "ceiling"] {
            let cell = expected(row, column);
            assert!(
                line.contains(&cell),
                "row {index} column {column} must carry the route's value {cell}: {line}"
            );
        }
    }

    // Engagements: empty state, still byte-for-byte.
    let route = operator_get(address, &token, "/api/native/v1/engagements?limit=100");
    let output = inspect(&state, address, "engagements", &["--json"]);
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), route.trim());
    let output = inspect(&state, address, "engagements", &[]);
    assert!(output.status.success());
    let table = String::from_utf8_lossy(&output.stdout);
    // E2: the exact header, not a prefix — `starts_with("id")` would accept
    // any leading word containing it and any column set after.
    let header: Vec<&str> = table.lines().next().unwrap().split_whitespace().collect();
    assert_eq!(
        header,
        vec![
            "id",
            "agentName",
            "projectId",
            "role",
            "requestedTokens",
            "state"
        ],
        "the engagements header is exactly the route's keys, in order"
    );
    let engagements: Vec<serde_json::Value> = serde_json::from_str(route.trim()).unwrap();
    assert_eq!(
        table.lines().count(),
        engagements.len() + 1,
        "one header plus one line per route row (empty here)"
    );

    // Alerts: `at_ms` is the read clock, so the rows compare parsed.
    let route = operator_get(address, &token, "/api/native/v1/alerts?limit=100");
    let output = inspect(&state, address, "alerts", &["--json"]);
    assert!(output.status.success());
    let passed: serde_json::Value =
        serde_json::from_str(String::from_utf8_lossy(&output.stdout).trim()).unwrap();
    let served: serde_json::Value = serde_json::from_str(route.trim()).unwrap();
    assert_eq!(passed["alerts"], served["alerts"]);
    assert!(passed["at_ms"].is_u64() && served["at_ms"].is_u64());
    let output = inspect(&state, address, "alerts", &[]);
    assert!(output.status.success());
    let table = String::from_utf8_lossy(&output.stdout);
    assert!(table.contains("at_ms"), "the envelope's clock key renders");
    drop(running);
}

#[test]
fn native_cli_inspection_exit_codes_name_refusals() {
    let directory = tempfile::tempdir().unwrap();
    let state = directory.path().join("refusal state");
    let init = Command::new(env!("CARGO_BIN_EXE_hagency"))
        .args(["init", "--state-dir"])
        .arg(&state)
        .env("PATH", "")
        .output()
        .unwrap();
    assert!(init.status.success());
    let token = fs::read_to_string(state.join("operator.token")).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let running = launch(&state, address);

    // Unreachable (3): a port nothing answers.
    let free = TcpListener::bind("127.0.0.1:0").unwrap();
    let dead = free.local_addr().unwrap();
    drop(free);
    let output = inspect(&state, dead, "resources", &["--limit", "5"]);
    assert_eq!(output.status.code(), Some(3), "unreachable exits 3");
    assert!(String::from_utf8_lossy(&output.stderr).contains("unreachable"));

    // Refused (4): a state directory whose operator credential is not the
    // running service's — the local read or the route's 401, either path.
    let wrong = tempfile::tempdir().unwrap();
    fs::write(wrong.path().join("operator.token"), "a".repeat(64)).unwrap();
    let output = inspect(wrong.path(), address, "resources", &[]);
    assert_eq!(output.status.code(), Some(4), "refused exits 4");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("refused"),
        "the refusal is named"
    );

    // Invalid (5): limits the CLI FORWARDS and the route refuses — E3 of
    // the CLI review: with the local short-circuit gone, 0 and 101 both
    // reach the route, whose own 400 maps to the invalid class. The
    // outcome is the documented one: the CLI never clamps, the route
    // refuses.
    for forwarded in ["0", "101"] {
        let output = inspect(&state, address, "engagements", &["--limit", forwarded]);
        assert_eq!(
            output.status.code(),
            Some(5),
            "forwarded limit {forwarded} refused by the route exits 5"
        );
        assert!(String::from_utf8_lossy(&output.stderr).contains("invalid"));
        // §4.3 of the review: no refusal path ever echoes the token.
        assert!(
            !String::from_utf8_lossy(&output.stderr).contains(&token),
            "stderr must never carry the operator token"
        );
    }

    // Missing route (7): a server that answers 404 — E4 of the CLI
    // review: "route not present" is its own class, never a malformed
    // request (the alerts route is the one contributed by another slice,
    // so a branch without it must report this, not exit 5).
    let absent = TcpListener::bind("127.0.0.1:0").unwrap();
    let absent_address = absent.local_addr().unwrap();
    let responder = std::thread::spawn(move || {
        if let Ok((mut stream, _)) = absent.accept() {
            use std::io::{Read as _, Write as _};
            let mut scratch = [0u8; 1024];
            let _ = stream.read(&mut scratch);
            let _ = stream.write_all(
                b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
        }
    });
    let output = inspect(&state, absent_address, "alerts", &[]);
    assert_eq!(output.status.code(), Some(7), "a 404 exits 7");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("route not present"),
        "the missing route is named"
    );
    let _ = responder.join();

    // Unavailable (6): a server that answers 503.
    let busy = TcpListener::bind("127.0.0.1:0").unwrap();
    let busy_address = busy.local_addr().unwrap();
    let responder = std::thread::spawn(move || {
        if let Ok((mut stream, _)) = busy.accept() {
            use std::io::{Read as _, Write as _};
            let mut scratch = [0u8; 1024];
            let _ = stream.read(&mut scratch);
            let _ = stream.write_all(
                b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
        }
    });
    let output = inspect(&state, busy_address, "alerts", &[]);
    assert_eq!(output.status.code(), Some(6), "unavailable exits 6");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("unavailable"),
        "the unavailability is named"
    );
    let _ = responder.join();
    drop(running);
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

/// The three console-access management flags are pairwise mutually exclusive:
/// each pair is refused before any ticket is issued (MA-S3a's CLI selector).
#[test]
fn native_console_account_grant_is_mutually_exclusive() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    let refuse = |flags: &[&str]| {
        let output = Command::new(env!("CARGO_BIN_EXE_hagency"))
            .arg("console-access")
            .args(flags)
            .arg("--state-dir")
            .arg(&state)
            .env("PATH", "")
            .env("HOME", "/untrusted-fixture-home")
            .output()
            .unwrap();
        assert!(
            !output.status.success(),
            "combination {flags:?} must be refused"
        );
        let stderr = String::from_utf8_lossy(&output.stderr).to_lowercase();
        assert!(
            stderr.contains("cannot be used with"),
            "refusal must name the conflict: {flags:?}"
        );
    };
    refuse(&[
        "--manage-account-enrollment",
        "--manage-resource-publication",
    ]);
    refuse(&[
        "--manage-account-enrollment",
        "--manage-resource-configuration",
    ]);
    refuse(&[
        "--manage-resource-publication",
        "--manage-resource-configuration",
    ]);
}

/// The native service and MCP helper construct no file log sink: every
/// diagnostic arrives on stderr. Both refusals below happen before any store
/// open (missing operator.token for serve; missing context env for the helper),
/// so the assertion needs no SQLite and runs on every OS.
#[test]
fn native_logs_to_stderr_with_no_file_sink() {
    let directory = tempfile::tempdir().unwrap();
    let state = directory.path().join("fresh state");

    // Serve refuses startup at the missing credential, before any store open.
    let serve = Command::new(env!("CARGO_BIN_EXE_hagency"))
        .args(["serve", "--state-dir"])
        .arg(&state)
        .env("PATH", "")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    assert!(!serve.status.success(), "missing token must refuse startup");
    assert!(
        serve.stdout.is_empty(),
        "native service must not write diagnostics to stdout"
    );
    assert!(
        !serve.stderr.is_empty(),
        "the startup refusal is reported on stderr"
    );

    // The MCP helper refuses at context load (no HAGENCY_* env): same posture.
    let mut helper = Command::new(env!("CARGO_BIN_EXE_hagency"));
    helper
        .arg("mcp")
        .env_clear()
        .env("PATH", "")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    helper.env("SystemRoot", std::env::var_os("SystemRoot").unwrap());
    let mcp = helper.output().unwrap();
    assert!(
        !mcp.status.success(),
        "missing context must refuse the helper"
    );
    assert!(
        mcp.stdout.is_empty(),
        "the helper must not write diagnostics to stdout"
    );
    assert!(
        !mcp.stderr.is_empty(),
        "the helper's refusal is reported on stderr"
    );

    // No file sink: nothing under the temp root carries a log-shaped name.
    fn collect_log_names(root: &Path, found: &mut Vec<String>) {
        if let Ok(entries) = fs::read_dir(root) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.ends_with(".log") || name.contains("jsonl") || name == "logs" {
                    found.push(entry.path().display().to_string());
                }
                collect_log_names(&entry.path(), found);
            }
        }
    }
    let mut found = Vec::new();
    collect_log_names(directory.path(), &mut found);
    assert!(found.is_empty(), "a file log sink appeared: {found:?}");
}
