//! Offline native fixture, never a model or a public runtime-launch endpoint.
#[path = "approval_probe/mod.rs"]
mod approval_probe;
use serde_json::{Value, json};
use std::{
    fs::{self, OpenOptions},
    io::{self, BufRead, Read, Write},
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

fn pulse(marker: &Path) -> io::Result<()> {
    // F3: the terminal lifetime is driven by the HOST's stdin close
    // (ownership stop), with the full budget (plus half again) as the outer
    // ceiling — never a fraction of the budget. `harness_wait() * 4` was
    // ten seconds of a twenty-five second operation, so a loaded host could
    // still reach its first write after the probe had left (the hosted
    // `Io("stdin write")` + `accepted_bytes: 0` class). Callers that hold
    // the `StdinLock` must drop it before calling — the reading thread
    // takes its own lock.
    hold_until_stdin_closed(marker, operation_budget_ms())
}
/// The operation budget the host grants, in ms. The host builders pass it on
/// the same env channel as `HAGENCY_OFFLINE_MODE`; the 25 s default matches
/// `Limits::operation_ms` so a probe run outside the harness is still
/// bounded.
fn operation_budget_ms() -> u64 {
    std::env::var("HAGENCY_OPERATION_BUDGET_MS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(25_000)
}
/// A derived wait: one tenth of the operation budget. A literal here is what
/// let the probe exit before a loaded host's first byte (`Io("stdin write")`,
/// accepted 0 of 51).
fn harness_wait() -> Duration {
    Duration::from_millis(operation_budget_ms() / 10)
}
fn hold_until_stdin_closed(marker: &Path, budget_ms: u64) -> io::Result<()> {
    // `StdinLock` holds a `MutexGuard` and is `!Send`: it cannot move into
    // the reading thread, so the lock must be TAKEN there. The caller
    // (`fake`) drops its own lock before calling here, so this lock observes
    // the host's close instead of deadlocking on a guard that never
    // releases.
    let (closed, host_closed) = std::sync::mpsc::channel::<()>();
    std::thread::spawn(move || {
        let mut stdin = io::stdin().lock();
        let mut byte = [0u8; 1];
        // Block until the host closes our stdin (EOF) or the stream errors.
        while stdin.read(&mut byte).unwrap_or(0) != 0 {}
        let _ = closed.send(());
    });
    hold(marker, budget_ms, host_closed)
}
/// The hold itself, over any blocking reader: it ends when the reader
/// reports EOF (the write end of the stream was closed — for the real probe,
/// the host stopping ownership) or when the derived ceiling expires, with
/// the reason printed. Generic so a unit test can prove the EOF path with a
/// synthetic reader instead of a spawned child (the sandbox walls spawning).
/// Test-only: the real probe takes the `stdin` path directly.
#[cfg(test)]
fn hold_until_closed<R: Read + Send + 'static>(
    marker: &Path,
    budget_ms: u64,
    mut stream: R,
) -> io::Result<()> {
    let (closed, host_closed) = std::sync::mpsc::channel::<()>();
    std::thread::spawn(move || {
        let mut byte = [0u8; 1];
        // Block until the stream closes (EOF) or errors.
        while stream.read(&mut byte).unwrap_or(0) != 0 {}
        let _ = closed.send(());
    });
    hold(marker, budget_ms, host_closed)
}
/// Pulse for evidence while waiting; the budget (plus half again) is the
/// outer ceiling, and expiry names what was being waited for.
fn hold(
    marker: &Path,
    budget_ms: u64,
    host_closed: std::sync::mpsc::Receiver<()>,
) -> io::Result<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(marker.with_extension("pulse"))?;
    let until = Instant::now() + Duration::from_millis(budget_ms + budget_ms / 2);
    loop {
        if host_closed.try_recv().is_ok() {
            return Ok(());
        }
        if Instant::now() >= until {
            return Err(io::Error::other(format!(
                "held {budget_ms}ms past the operation budget waiting for the host to close stdin"
            )));
        }
        file.write_all(b"x")?;
        file.flush()?;
        std::thread::sleep(Duration::from_millis(20));
    }
}
fn gated_pulse(marker: &Path) -> io::Result<()> {
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(marker.with_extension("pulse"))?;
    file.write_all(b"xxx")?;
    file.flush()?;
    // The gate keeps its derived bound — a fifth of the operation budget,
    // the harness's `harness_wait() * 2` shape doubled: the TEST must
    // release us, and a test that cannot observe our pulse within a fifth
    // of the whole operation is itself broken — while the LIFETIME after
    // it is the host's close, like every terminal path.
    let until = Instant::now() + harness_wait() * 2;
    while !marker.with_extension("release").is_file() {
        if Instant::now() >= until {
            return Err(io::Error::other(
                "gated keepalive was never released by the test",
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    hold_until_stdin_closed(marker, operation_budget_ms())
}
fn read(reader: &mut impl BufRead, marker: &Path) -> io::Result<Value> {
    let mut bytes = Vec::new();
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        let count = available
            .iter()
            .position(|&c| c == b'\n')
            .map_or(available.len(), |n| n + 1);
        if bytes.len() + count > 1_048_577 {
            return Err(io::ErrorKind::InvalidData.into());
        }
        bytes.extend_from_slice(&available[..count]);
        reader.consume(count);
        if bytes.last() == Some(&b'\n') {
            break;
        }
    }
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(marker.with_extension("requests"))?
        .write_all(&bytes)?;
    serde_json::from_slice(&bytes).map_err(io::Error::other)
}
fn send(value: Value) -> io::Result<()> {
    let mut bytes = serde_json::to_vec(&value)?;
    bytes.push(b'\n');
    let mut stdout = io::stdout().lock();
    stdout.write_all(&bytes)?;
    stdout.flush()
}
fn result(request: &Value, value: Value) -> io::Result<()> {
    send(json!({ "id": request["id"], "result": value }))
}
fn note(method: &str, params: Value) -> io::Result<()> {
    send(json!({ "method": method, "params": params }))
}
fn method(request: &Value, expected: &str) -> io::Result<()> {
    if request["method"] == expected {
        Ok(())
    } else {
        Err(io::Error::other("unexpected fixture method"))
    }
}
fn fake(mode: &str, marker: &Path) -> io::Result<()> {
    fs::write(marker.with_extension("entered"), b"entered")?;
    // The mode echo (stale-binary proof): the probe records WHICH mode it
    // executed before dispatching, so a VM run whose markers imply one path
    // can never masquerade as a logic miss in another — tests can assert it.
    fs::write(marker.with_extension("mode"), mode)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt;
        let mut sockets = 0;
        for entry in fs::read_dir("/dev/fd")? {
            if fs::metadata(entry?.path()).is_ok_and(|v| v.file_type().is_socket()) {
                sockets += 1;
            }
        }
        fs::write(marker.with_extension("sockets"), sockets.to_string())?;
    }
    if mode == "gated-keepalive" {
        return gated_pulse(marker);
    }
    if mode == "keepalive" || mode == "silent" {
        return pulse(marker);
    }
    #[cfg(windows)]
    if mode == "breakaway" {
        use std::os::windows::process::CommandExt;
        let attempted = Command::new(std::env::current_exe()?)
            .arg("pulse")
            .arg(marker.with_file_name("escape"))
            .creation_flags(windows_sys::Win32::System::Threading::CREATE_BREAKAWAY_FROM_JOB)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        match attempted {
            Err(error) if error.raw_os_error() == Some(5) => {
                fs::write(marker.with_extension("breakaway"), b"access-denied")?;
            }
            Err(error) => return Err(error),
            Ok(mut escaped) => {
                let _ = escaped.kill();
                let _ = escaped.wait();
                return Err(io::Error::other("fixture escaped its job"));
            }
        }
    }
    let mut child = if mode == "descendant" || mode == "detached" {
        let child_marker = marker.with_file_name(format!("{mode}-child"));
        let mut command = Command::new(std::env::current_exe()?);
        command
            .arg("pulse")
            .arg(&child_marker)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(unix)]
        if mode == "detached" {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        #[cfg(windows)]
        if mode == "detached" {
            use std::os::windows::process::CommandExt;
            command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NEW_PROCESS_GROUP);
        }
        let mut child = command.spawn()?;
        // Observe a distinct child's output before protocol completion can cause
        // the owner to stop us. This prevents a never-scheduled child from being
        // mistaken for a successfully exercised descendant termination path.
        let until = Instant::now() + harness_wait();
        while fs::metadata(child_marker.with_extension("pulse")).map_or(0, |m| m.len()) < 2 {
            if Instant::now() >= until {
                let _ = child.kill();
                let _ = child.wait();
                return Err(io::Error::other("fixture descendant did not start"));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        Some(child)
    } else {
        None
    };
    let mut stdin = io::stdin().lock();
    let request = read(&mut stdin, marker)?;
    method(&request, "initialize")?;
    if mode == "eof" {
        return Ok(());
    }
    if mode == "noisy" {
        io::stderr().write_all(&vec![b'e'; 256 * 1024])?;
        io::stderr().flush()?;
    }
    result(
        &request,
        json!({ "userAgent": "offline-fixture/0.153.4", "platformFamily": "unix", "platformOs": "fixture", "codexHome": "/fixture" }),
    )?;
    method(&read(&mut stdin, marker)?, "initialized")?;
    let request = read(&mut stdin, marker)?;
    method(&request, "thread/start")?;
    let params = &request["params"];
    if params["approvalPolicy"] != "on-request"
        || params["approvalsReviewer"] != "user"
        || params["sandbox"] != "workspace-write"
    {
        return Err(io::Error::other("fixture policy mismatch"));
    }
    result(
        &request,
        json!({ "thread": { "id": "owned-thread", "cwd": params["cwd"], "status": { "type": "idle" }, "turns": [] },
        "cwd": params["cwd"], "model": params["model"], "modelProvider": "offline", "approvalPolicy": "on-request", "approvalsReviewer": "user", "sandbox": { "type": "workspaceWrite" } }),
    )?;
    if mode == "blocked" {
        #[cfg(windows)]
        {
            use std::io::Read;
            let mut partial = [0u8; 4096];
            stdin.read_exact(&mut partial)?;
            fs::write(marker.with_extension("partial"), partial)?;
        }
        drop(stdin);
        return pulse(marker);
    }
    let request = read(&mut stdin, marker)?;
    method(&request, "turn/start")?;
    if request["params"]["threadId"] != "owned-thread"
        || request["params"]["sandboxPolicy"]["networkAccess"] != false
    {
        return Err(io::Error::other("fixture turn mismatch"));
    }
    result(
        &request,
        json!({ "turn": { "id": "owned-turn", "status": "inProgress", "items": [] } }),
    )?;
    if matches!(mode, "quiet-turn" | "quiet-open") {
        fs::write(marker.with_extension("quiet"), b"turn-start-acknowledged")?;
        if mode == "quiet-open" {
            drop(stdin);
            return pulse(marker); // filesystem evidence only; no protocol keepalive
        }
        // A real acknowledged turn can run a tool without another app-server
        // event during the shorter RPC response interval.
        std::thread::sleep(harness_wait());
    }
    if mode.starts_with("owned-approval") && !approval_probe::run(mode, &mut stdin, marker)? {
        return Ok(());
    }
    if mode == "approval" {
        send(
            json!({"id":"approval","method":"item/commandExecution/requestApproval","params":{"threadId":"owned-thread","turnId":"owned-turn","itemId":"command","command":"fixture","cwd":std::env::current_dir()?.to_string_lossy()}}),
        )?;
        drop(stdin);
        return pulse(marker);
    }
    if mode == "wrong-scope" {
        note(
            "thread/status/changed",
            json!({"threadId":"impostor-thread","status":{"type":"idle"}}),
        )?;
        drop(stdin);
        return pulse(marker);
    }
    if mode == "usage-gate" {
        fs::write(marker.with_extension("usage-ready"), b"ready")?;
        let until = Instant::now() + harness_wait() * 2;
        while !marker.with_extension("usage-release").is_file() {
            if Instant::now() >= until {
                return Err(io::Error::other("offline usage gate expired"));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    if matches!(mode, "usage" | "usage-overflow" | "usage-gate") {
        // Pinned cumulative usage categories, emitted through actual child
        // stdout. A repeated snapshot must not be summed as fresh consumption.
        let snapshots: &[(u64, u64, u64, u64)] = if mode == "usage-overflow" {
            &[(9_007_199_254_740_991, 1, 0, 0)]
        } else {
            &[
                (100, 10, 40, 60),
                (100, 10, 40, 60),
                (130, 20, 45, 65),
                (20, 5, 5, 5),
            ]
        };
        for &(input, output, cached, write) in snapshots {
            note(
                "thread/tokenUsage/updated",
                json!({
                    "threadId":"owned-thread", "turnId":"owned-turn",
                    "tokenUsage": {
                        "total": {"totalTokens":input + output,"inputTokens":input,"cachedInputTokens":cached,
                            "cacheWriteInputTokens":write,"outputTokens":output,"reasoningOutputTokens":0},
                        "last": {"totalTokens":1,"inputTokens":0,"cachedInputTokens":0,
                            "cacheWriteInputTokens":0,"outputTokens":1,"reasoningOutputTokens":0},
                        "modelContextWindow":200000
                    }
                }),
            )?;
        }
    }
    note(
        "item/started",
        json!({ "threadId": "owned-thread", "turnId": "owned-turn", "startedAtMs": 1, "item": { "id": "answer", "type": "agentMessage", "phase": "final_answer", "text": "" } }),
    )?;
    let answer = "离线管道验证完成";
    note(
        "item/agentMessage/delta",
        json!({ "threadId": "owned-thread", "turnId": "owned-turn", "itemId": "answer", "delta": answer }),
    )?;
    note(
        "item/completed",
        json!({ "threadId": "owned-thread", "turnId": "owned-turn", "completedAtMs": 2, "item": { "id": "answer", "type": "agentMessage", "phase": "final_answer", "text": answer } }),
    )?;
    note(
        "turn/completed",
        json!({ "threadId": "owned-thread", "turn": { "id": "owned-turn", "status": "completed", "items": [] } }),
    )?;
    // Stay alive after writing terminal output until the HOST stops ownership
    // (our stdin closes). The old fixed 8 s pulse expired while a loaded host
    // was still inside its operation budget, and the host's next write failed
    // as `Io` with zero bytes accepted — the fixture, not the product, caused
    // it. The close drives the hold; the budget (plus half again) is only the
    // outer ceiling. `stdin` is still held here — drop it first, or the
    // reading thread's lock would block on this guard forever.
    drop(stdin);
    let outcome = hold_until_stdin_closed(marker, operation_budget_ms());
    if let Some(child) = &mut child {
        let _ = child.kill();
        let _ = child.wait();
    }
    outcome
}
fn main() -> io::Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    #[cfg(unix)]
    if args.first().is_some_and(|v| v == "guardian") {
        return hagency_platform::run_guardian();
    }
    match args.as_slice() {
        [command] if command == "app-server" => {
            // Fixed host installation entrypoint for offline dispatch fixtures.
            // The environment is explicitly supplied by that test host only.
            let mode = std::env::var("HAGENCY_OFFLINE_MODE").map_err(io::Error::other)?;
            if mode == "account" {
                let home = std::env::var_os("HOME").ok_or(io::ErrorKind::InvalidInput)?;
                let codex = std::env::var_os("CODEX_HOME").ok_or(io::ErrorKind::InvalidInput)?;
                if home != codex
                    || std::env::var_os("OPENAI_API_KEY").is_some()
                    || std::env::var_os("CODEX_API_KEY").is_some()
                {
                    return Err(io::ErrorKind::InvalidInput.into());
                }
                let marker = fs::read_to_string(Path::new(&home).join("fixture-account-marker"))?;
                if marker != "selected-A" {
                    return Err(io::ErrorKind::InvalidInput.into());
                }
                fs::write(
                    "account-observed.json",
                    serde_json::to_vec(
                        &json!({"marker":marker,"same_home":true,"ambient_key":false}),
                    )?,
                )?;
            }
            fake(
                if mode == "account" { "normal" } else { &mode },
                &std::env::current_dir()?.join("owned-dispatch"),
            )
        }
        #[cfg(windows)]
        [mode, marker] if mode == "owner-crash" => owner_crash(Path::new(marker)),
        [mode, marker] if mode == "pulse" => pulse(Path::new(marker)),
        [command, mode, marker] if command == "fake-server" => fake(
            mode.to_str().ok_or(io::ErrorKind::InvalidInput)?,
            Path::new(marker),
        ),
        _ => Err(io::Error::other("offline fixture mode required")),
    }
}

#[cfg(windows)]
fn owner_crash(marker: &Path) -> io::Result<()> {
    let binary = std::env::current_exe()?;
    let mut environment = std::collections::BTreeMap::new();
    environment.insert("PATH".into(), "".into());
    if let Some(value) = std::env::var_os("SystemRoot") {
        environment.insert("SystemRoot".into(), value);
    }
    let launch = hagency_platform::Launch {
        executable: binary.clone(),
        arguments: vec![
            "fake-server".into(),
            "descendant".into(),
            marker.as_os_str().into(),
        ],
        directory: marker.parent().ok_or(io::ErrorKind::InvalidInput)?.into(),
        environment,
        require_crash_containment: true,
    };
    let (_owner, _pipes) = hagency_platform::SupervisedProcess::spawn_piped(&binary, &launch)?;
    let until = Instant::now() + harness_wait();
    while fs::metadata(marker.with_file_name("descendant-child.pulse")).map_or(0, |m| m.len()) < 3 {
        if Instant::now() >= until {
            return Err(io::Error::other("owned descendant did not start"));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    // Deliberately bypass Drop: only kill-on-close job ownership can stop the
    // already-running descendant when the controller's process exits.
    std::process::exit(0);
}

#[cfg(test)]
mod hold_tests {
    use super::hold_until_closed;
    use std::time::{Duration, Instant};

    /// The observable-hold proof (brief 20's H1): a stream whose write end
    /// is closed (an empty reader reports EOF immediately) ends the hold
    /// promptly — the hold must observe the close, not run to its ceiling.
    /// The old broken shape (a second `io::stdin().lock()` deadlocking on
    /// the main thread's guard) never observes the close and holds to the
    /// ceiling, so this test discriminates the mechanism, not the constants.
    #[test]
    fn native_probe_hold_ends_when_the_stream_closes() {
        let marker =
            std::env::temp_dir().join(format!("hagency-probe-hold-{}-close", std::process::id()));
        let started = Instant::now();
        // An empty reader IS a closed stream: its write end is gone.
        let closed_stream: &[u8] = &[];
        let budget_ms = 10_000u64;
        let outcome = hold_until_closed(&marker, budget_ms, closed_stream);
        let _ = std::fs::remove_file(marker.with_extension("pulse"));
        outcome.expect("a closed stream must end the hold, not the ceiling");
        // Derived from the budget passed above, not a literal: half the
        // hold's own budget is the generous local slack for "an empty
        // reader reports EOF immediately" (F5) — far under the hold's
        // 1.5×-budget ceiling, and it scales if the test budget changes.
        let local_slack = Duration::from_millis(budget_ms / 2);
        assert!(
            started.elapsed() < local_slack,
            "the hold ran toward its ceiling instead of observing the close"
        );
    }
}
