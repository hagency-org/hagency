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
    time::{Duration, Instant},
};

fn binary() -> PathBuf {
    env!("CARGO_BIN_EXE_hagency").into()
}
fn plist_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../deploy/io.hagency.native.plist")
}
/// The plist contract every leg pins (ADR-133): RunAtLoad, KeepAlive TRUE
/// (not {SuccessfulExit: false} — a clean SIGTERM exit must not restart),
/// ThrottleInterval, log paths under the install dir, the FIXED loopback
/// listen (a refusal, not a default), and the foreground serve argv.
fn assert_plist_contract() {
    let plist = fs::read_to_string(plist_path()).expect("plist present on every leg");
    assert!(plist.contains("<key>Label</key>"), "label present");
    assert!(
        plist.contains("<string>io.hagency.native</string>"),
        "label is io.hagency.native"
    );
    assert!(plist.contains("<key>RunAtLoad</key>"), "RunAtLoad present");
    assert!(plist.contains("<key>KeepAlive</key>"), "KeepAlive present");
    assert!(
        plist.contains("<key>KeepAlive</key>\n  <true/>"),
        "KeepAlive is true, not SuccessfulExit-keyed: a clean SIGTERM exit must not restart"
    );
    assert!(
        plist.contains("<key>ThrottleInterval</key>"),
        "ThrottleInterval present"
    );
    assert!(
        plist.contains("<key>StandardOutPath</key>")
            && plist.contains("<key>StandardErrorPath</key>"),
        "both log paths present (no journald on macOS)"
    );
    assert!(
        plist.contains("<string>127.0.0.1:13300</string>"),
        "loopback listen is FIXED in the plist — no knob can make it non-loopback"
    );
    assert!(
        plist.contains("__STATE_DIR__") && plist.contains("__INSTALL_DIR__"),
        "explicit placeholders, never guessed defaults"
    );
    assert!(
        plist.contains("<string>serve</string>") && plist.contains("<string>--state-dir</string>"),
        "ProgramArguments is the foreground serve command"
    );
    assert!(
        plist.contains("bootout"),
        "the deliberate stop is documented as launchctl bootout, not kill-by-pid"
    );
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
    {
        let db = rusqlite::Connection::open(state.join("domain.sqlite3")).unwrap();
        db.execute_batch(
            "INSERT INTO projects(fleet_id,id,generation,room_id,owner_mxid,owner_room_id)
             VALUES('fleet','project_one',1,'!room:example.test','@owner:example.test','!dm:example.test');
             INSERT INTO engagements(id,fleet_id,generation,request_id,digest,context,evidence,project_id,name,resource_id,tokens,state,projection)
             VALUES('eng','fleet',1,'req','digest','{}','{}','project_one','agent','res',100,'active','{}');
             INSERT INTO runner_sessions(id,engagement_id,binding,quarantined)
             VALUES('session','eng','{}',0);
             INSERT INTO runner_dispatches(id,session_id,task_id,input,digest,state,fence)
             VALUES('dispatch','session',NULL,'{\"instruction\":\"pending\"}','digest','outcome_unknown',1);",
        )
        .unwrap();
    }
    let mut first = spawn_service(&state);
    wait_ready(&first);
    term_then_observe_exit(&mut first, Duration::from_secs(20));
    let second = spawn_service(&state);
    wait_ready(&second);
    {
        let db = rusqlite::Connection::open(state.join("domain.sqlite3")).unwrap();
        let row: (String, i64) = db
            .query_row(
                "SELECT state,fence FROM runner_dispatches WHERE id='dispatch'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(row, ("outcome_unknown".into(), 1));
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
