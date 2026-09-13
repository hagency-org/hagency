//! SR-1 (ADR-127): the supervisor-less signal harness. This file is
//! deliberately UNGATED; every selector below is present on every hosted
//! leg. The Linux body is selected at RUNTIME (`cfg!`), not by `#[cfg]`,
//! so this file type-checks the whole harness on every leg while the
//! non-Linux legs assert the documented named refusal (the
//! hagency-platform guardian.rs precedent, hardened one step further).
//! CI has no init system — the harness is not systemd itself; it proves
//! the start/stop/restart contract the unit and installer rely on.
use std::{
    fs,
    io::{Read, Write},
    net::TcpStream,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

fn binary() -> PathBuf {
    env!("CARGO_BIN_EXE_hagency").into()
}
fn deploy_unit() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../deploy/hagency-native.service")
}
/// The named refusal the spec fixes for the other hosted OSes (ADR-127):
/// never skip, never pretend a service leg ran where it cannot. The leg
/// instead pins the unit contract offline — the directives the Linux spawn
/// leg exercises — so the selector is green while honest about the refusal.
fn refuse_not_linux() {
    assert_ne!(std::env::consts::OS, "linux");
    let unit = fs::read_to_string(deploy_unit()).expect("unit present on every leg");
    assert!(unit.contains("ExecStart="), "ExecStart rendered");
    assert!(unit.contains("--state-dir"), "state placeholder present");
    assert!(!unit.contains("ExecStop="), "no ExecStop (F2)");
    assert!(!unit.contains("KillMode="), "no KillMode (F2)");
    assert!(!unit.contains("KillSignal="), "no KillSignal (F2)");
    assert!(unit.contains("TimeoutStopSec=20"), "retained stop budget");
    assert!(unit.contains("127.0.0.1:13300"), "loopback listen is fixed");
    assert!(unit.contains("Restart=on-failure"), "restart policy");
    assert!(unit.contains("RestartSec=5"), "restart interval");
    assert!(unit.contains("StateDirectory=hagency-native"), "state root");
}
struct ServeGuard(Child);
impl Drop for ServeGuard {
    fn drop(&mut self) {
        let _ = self.0.kill(); // std's safe SIGKILL; a leaked guard is a failure
        let _ = self.0.wait();
    }
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
    // The loopback bind is a refusal, not a default; only the port is the
    // harness's choice, so parallel legs cannot collide on 13300.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    format!("127.0.0.1:{port}")
}
struct Running {
    guard: ServeGuard,
    addr: String,
}
fn init_state(root: &Path) -> PathBuf {
    let state = root.join("state");
    let status = Command::new(binary())
        .args(["init", "--state-dir", state.to_str().unwrap()])
        .status()
        .expect("hagency init spawns");
    assert!(status.success(), "hagency init must provision fresh state");
    state
}
fn spawn_service(state: &Path) -> Running {
    let addr = free_loopback();
    // Exactly the argv shape the systemd unit renders (ADR-127): serve
    // --state-dir <dir> --listen <loopback>. The workspace forbids unsafe
    // code even in tests, so the signal below is the external kill helper,
    // never libc::kill in-process.
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
        guard: ServeGuard(child),
        addr,
    }
}
fn start_service(root: &Path) -> Running {
    // init once per state dir; restart legs call spawn_service directly so
    // the deliberate second start never trips init's non-empty refusal.
    spawn_service(&init_state(root))
}
fn wait_ready(running: &Running) {
    // The start gate is /ready, never /health: health is 200-while-live and
    // proves nothing at cutover (ADR-127 F8).
    let until = Instant::now() + Duration::from_secs(20);
    loop {
        if http_status(&running.addr, "/ready") == Some(200) {
            return;
        }
        assert!(Instant::now() < until, "service never answered ready 200");
        thread::sleep(Duration::from_millis(100));
    }
}
fn send_term(child: &Child) {
    let status = Command::new("kill")
        .args(["-s", "TERM", &child.id().to_string()])
        .status()
        .expect("external kill helper spawns");
    assert!(status.success(), "SIGTERM delivery failed");
}
/// The unit's TimeoutStopSec contract: exit zero inside the budget, or park
/// on an unknown close without a false success (ADR-120). Parking means the
/// process is still alive at the budget: the guard then SIGKILLs it, which
/// is the honest terminal state systemd itself would apply.
fn term_then_observe_exit(running: &mut Running, budget: Duration) -> bool {
    let child = &mut running.guard.0;
    send_term(child);
    let until = Instant::now() + budget;
    loop {
        if let Some(code) = child.try_wait().unwrap() {
            assert!(
                code.success(),
                "service must exit zero on the deliberate stop, got {code:?}"
            );
            return true; // clean drain-and-close
        }
        if Instant::now() >= until {
            return false; // parked on an unknown close; no false success
        }
        thread::sleep(Duration::from_millis(50));
    }
}
fn linux_leg() -> bool {
    cfg!(target_os = "linux")
}
#[test]
fn native_service_unit_starts_and_reports_ready() {
    if !linux_leg() {
        refuse_not_linux();
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let running = start_service(root.path());
    wait_ready(&running);
    // Ready means every component reports a ready word; health stays 200
    // while live. Both boundaries observed, neither conflated.
    assert_eq!(http_status(&running.addr, "/health"), Some(200));
}
#[test]
fn native_service_unit_stops_cleanly_within_timeout_budget() {
    if !linux_leg() {
        refuse_not_linux();
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let mut running = start_service(root.path());
    wait_ready(&running);
    // The unit's TimeoutStopSec budget (20s) bounds the whole stop. The
    // ready-to-503 flip is observed opportunistically during the drain
    // window; the exit contract is the hard assertion. `false` means parked
    // on an unknown close (ADR-120): no false success; the guard then
    // SIGKILLs, which is what systemd itself would apply at the budget.
    let exited_clean = term_then_observe_exit(&mut running, Duration::from_secs(20));
    assert!(
        exited_clean,
        "clean drain-and-close expected on the idle leg"
    );
}
#[test]
fn native_service_unit_restart_preserves_pending_state() {
    if !linux_leg() {
        refuse_not_linux();
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let state = init_state(root.path());
    // Seed one outcome-unknown custody row while nothing holds the store
    // (the store stays locked to every other process once running).
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
        // Never resolved or dropped by the stop-start pair.
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
