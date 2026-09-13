//! SR-2 (ADR-133): the launchd agent selectors. UNGATED file; the macOS
//! body is selected at RUNTIME (`cfg!`) so the whole harness type-checks on
//! every hosted leg, and the other OSes assert the documented
//! not-a-macOS-agent refusal (never skipping) while pinning the plist keys
//! offline. On macOS the body additionally runs the serve command the
//! plist's ProgramArguments names — the real start/stop/restart contract,
//! without launchd itself (CI has none).
use std::{
    fs,
    io::{Read, Write},
    net::TcpStream,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use hagency_core::tasks::{DispatchInput, SessionBinding};
use hagency_store::EffectOutcome;
use serde_json::json;

#[path = "../../hagency-store/tests/common/mod.rs"]
mod domain;

fn binary() -> PathBuf {
    env!("CARGO_BIN_EXE_hagency").into()
}
fn plist_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../deploy/io.hagency.native.plist")
}
/// The plist contract every leg pins (ADR-133): RunAtLoad; KeepAlive true —
/// which restarts on ANY exit, a crash and a clean exit-0 alike, and on a
/// pid-kill; the only stop that stays stopped is `launchctl bootout`, which
/// removes the job so no KeepAlive policy applies; ThrottleInterval; log
/// paths under the install dir; the FIXED loopback listen (a refusal, not a
/// default); the foreground serve argv. Keys are PARSED from the XML, never
/// matched as comment substrings.
fn assert_plist_contract() {
    let plist = fs::read_to_string(plist_path()).expect("plist present on every leg");
    let value = |key: &str| plist_value(&plist, key);
    assert_eq!(value("Label").as_deref(), Some("io.hagency.native"));
    assert_eq!(
        value("RunAtLoad").as_deref(),
        Some("true"),
        "RunAtLoad true: boot recovery"
    );
    assert_eq!(
        value("KeepAlive").as_deref(),
        Some("true"),
        "KeepAlive true restarts on ANY exit — a crash and a clean exit-0 — and on a \
         pid-kill; the only stop that stays stopped is launchctl bootout, which removes \
         the job so no KeepAlive policy applies"
    );
    assert_eq!(
        value("ThrottleInterval").as_deref(),
        Some("10"),
        "ThrottleInterval replaces RestartSec"
    );
    assert!(
        value("StandardOutPath").is_some_and(|v| v.contains("/logs/")),
        "stdout log path under the install dir's logs/ (no journald on macOS)"
    );
    assert!(value("StandardErrorPath").is_some_and(|v| v.contains("/logs/")));
    let arguments = plist_array_strings(&plist, "ProgramArguments");
    assert!(
        arguments.iter().any(|v| v.ends_with("/hagency")),
        "ProgramArguments execs the native binary"
    );
    assert!(arguments.contains(&"serve".to_string()));
    assert!(arguments.contains(&"--state-dir".to_string()));
    assert!(
        arguments.contains(&"127.0.0.1:13300".to_string()),
        "the loopback listen is FIXED in ProgramArguments — a refusal, not a default"
    );
    assert!(
        arguments.iter().any(|v| v == "__STATE_DIR__"),
        "explicit state placeholder, never a guessed default"
    );
}

/// Read one scalar plist value by key from the rendered XML — a real
/// `<key>`/value pair, never a comment substring.
fn plist_value(plist: &str, key: &str) -> Option<String> {
    let lines: Vec<&str> = plist.lines().map(str::trim).collect();
    let mut i = 0;
    while i < lines.len() {
        if lines[i] == format!("<key>{key}</key>") {
            return lines.get(i + 1).copied().and_then(plist_scalar);
        }
        i += 1;
    }
    None
}

/// Parse one scalar plist element line: `<true/>` → "true",
/// `<string>x</string>` → "x", `<integer>10</integer>` → "10".
fn plist_scalar(line: &str) -> Option<String> {
    let inner = line.strip_prefix('<')?;
    let (tag, rest) = inner.split_once('>')?;
    if let Some(close) = rest.rfind("</") {
        return Some(rest[..close].to_string());
    }
    // Self-closing element: <true/> parses as tag "true/".
    tag.strip_suffix('/').map(str::to_string)
}

/// Read the `<array>` of strings following a key (ProgramArguments).
fn plist_array_strings(plist: &str, key: &str) -> Vec<String> {
    let lines: Vec<&str> = plist.lines().map(str::trim).collect();
    let Some(start) = lines
        .iter()
        .position(|l| *l == format!("<key>{key}</key>"))
        .and_then(|k| lines.iter().skip(k + 1).position(|l| *l == "<array>"))
        .map(|a| a + 1)
    else {
        return Vec::new();
    };
    let mut values = Vec::new();
    for line in &lines[start..] {
        if *line == "</array>" {
            break;
        }
        if let Some(value) = line
            .strip_prefix("<string>")
            .and_then(|v| v.strip_suffix("</string>"))
        {
            values.push(value.to_string());
        }
    }
    values
}
/// The named refusal for the non-macOS hosted legs: never skip, never
/// pretend an agent leg ran where launchd does not exist.
fn refuse_not_macos() {
    assert_ne!(std::env::consts::OS, "macos");
    assert_plist_contract();
}
fn http_status(addr: &str, path: &str) -> Option<u16> {
    let mut stream = TcpStream::connect(addr).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n"
    )
    .ok()?;
    let mut response = String::new();
    let _ = stream.read_to_string(&mut response);
    response.split_whitespace().nth(1)?.parse().ok()
}
fn free_loopback() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    format!("127.0.0.1:{port}")
}
struct Guard(std::process::Child);
impl Drop for Guard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
struct Running {
    guard: Guard,
    addr: String,
}
fn init_state(root: &Path) -> std::path::PathBuf {
    let state = root.join("state");
    let status = Command::new(binary())
        .args(["init", "--state-dir", state.to_str().unwrap()])
        .status()
        .expect("hagency init spawns");
    assert!(status.success(), "hagency init must provision fresh state");
    state
}
/// Exactly the argv the plist's ProgramArguments renders after placeholder
/// substitution — the command the wrapper execs.
fn spawn_service(state: &Path) -> Running {
    let addr = free_loopback();
    let child = Command::new(binary())
        .args([
            "serve",
            "--state-dir",
            state.to_str().unwrap(),
            "--listen",
            &addr,
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("serve spawns");
    Running {
        guard: Guard(child),
        addr,
    }
}
fn wait_ready(running: &Running) {
    let until = Instant::now() + Duration::from_secs(20);
    loop {
        if http_status(&running.addr, "/ready") == Some(200) {
            return;
        }
        assert!(
            Instant::now() < until,
            "agent never answered ready 200 (start gate is /ready, never /health)"
        );
        thread::sleep(Duration::from_millis(100));
    }
}
fn term_then_observe_exit(running: &mut Running, budget: Duration) -> bool {
    let status = Command::new("kill")
        .args(["-s", "TERM", &running.guard.0.id().to_string()])
        .status()
        .expect("external kill helper spawns");
    assert!(status.success(), "SIGTERM delivery failed");
    let child = &mut running.guard.0;
    let until = Instant::now() + budget;
    loop {
        if let Some(code) = child.try_wait().unwrap() {
            assert!(
                code.success(),
                "agent must exit zero on the deliberate stop, got {code:?}"
            );
            return true;
        }
        if Instant::now() >= until {
            return false; // parked on an unknown close; no false success
        }
        thread::sleep(Duration::from_millis(50));
    }
}
fn macos_leg() -> bool {
    cfg!(target_os = "macos")
}
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
/// Plant the pending custody row through the store's own API — an admitted
/// and approved request, a registered session, a canonical task and an
/// enqueued (queued, never-started) dispatch — never a bare INSERT that
/// references parent rows that do not exist. A queued dispatch is a pending
/// row by the spec's own vocabulary, and nothing in `serve` resolves it.
fn seed_pending_dispatch(state: &Path) {
    let mut db = hagency_store::DomainRepository::open(&state).unwrap();
    db.register(&domain::registration()).unwrap();
    let pool = domain::resource("pool", "seat", 1000);
    db.put_resource(&pool).unwrap();
    let proof = domain::proof(&domain::request("restart", "Worker", &pool, 100));
    let engagement = db.admit(&proof, 1000).unwrap();
    db.approve("approve", &proof, 1000).unwrap();
    let effect = db.claim_effect().unwrap().unwrap();
    db.observe_effect(
        &effect.id,
        effect.fence,
        &EffectOutcome::Applied {
            receipt: "fixture account".into(),
        },
    )
    .unwrap();
    db.register_session(&SessionBinding {
        id: "session".into(),
        engagement_id: engagement.id,
        room_id: "!room:example.test".into(),
        thread_root: None,
    })
    .unwrap();
    db.create_canonical_task("task", "session", "Pending across restart", now_ms())
        .unwrap();
    db.enqueue_dispatch(&DispatchInput {
        id: "dispatch".into(),
        session_id: "session".into(),
        task_id: Some("task".into()),
        resources: vec![],
        payload: json!({"instruction":"pending across the restart pair"}),
    })
    .unwrap();
}
#[test]
fn native_launchd_agent_starts_and_reports_ready() {
    assert_plist_contract();
    if !macos_leg() {
        refuse_not_macos();
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let state = init_state(root.path());
    let running = spawn_service(&state);
    wait_ready(&running);
    assert_eq!(http_status(&running.addr, "/health"), Some(200));
}
#[test]
fn native_launchd_agent_stops_cleanly() {
    assert_plist_contract();
    if !macos_leg() {
        refuse_not_macos();
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let state = init_state(root.path());
    let mut running = spawn_service(&state);
    wait_ready(&running);
    // ThrottleInterval (10s) + the drain budget bound the stop; 20s matches
    // the systemd unit's TimeoutStopSec for cross-OS comparability.
    let clean = term_then_observe_exit(&mut running, Duration::from_secs(20));
    assert!(clean, "clean drain-and-close expected on the idle leg");
}
#[test]
fn native_launchd_restart_preserves_pending_state() {
    assert_plist_contract();
    if !macos_leg() {
        refuse_not_macos();
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let state = init_state(root.path());
    seed_pending_dispatch(&state);
    let mut first = spawn_service(&state);
    wait_ready(&first);
    term_then_observe_exit(&mut first, Duration::from_secs(20));
    let second = spawn_service(&state);
    wait_ready(&second);
    {
        let db = rusqlite::Connection::open(state.join("domain.sqlite3")).unwrap();
        // The seeded pending row survives the stop-start pair exactly as it
        // was planted: still queued, fence 0, never resolved or dropped.
        let row: (String, i64) = db
            .query_row(
                "SELECT state,fence FROM runner_dispatches WHERE id='dispatch'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(row, ("queued".into(), 0));
        let resolved: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM runner_outputs WHERE dispatch_id='dispatch'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(resolved, 0);
    }
}
