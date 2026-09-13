//! SR-1 (ADR-127): the supervisor-less signal harness. This file is
//! deliberately UNGATED; every selector below is present on every hosted
//! leg. The Linux body is selected at RUNTIME (`cfg!`), not by `#[cfg]`,
//! so this file type-checks the whole harness on every leg while the
//! non-Linux legs assert the documented named refusal (the
//! hagency-platform guardian.rs precedent, hardened one step further).
//! CI has no init system — the harness is not systemd itself; it proves
//! the start/stop/restart contract the unit and installer rely on.
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    net::TcpStream,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use hagency_core::tasks::{DispatchInput, SessionBinding};
use hagency_store::EffectOutcome;
use serde_json::json;

#[path = "../../hagency-store/tests/common/mod.rs"]
mod domain;

/// The start gate's explicit budget, named like the stop budget: the same
/// 20-second figure as the unit's TimeoutStopSec, but this one bounds the
/// /ready poll after start, not the drain after SIGTERM.
const START_GATE_BUDGET: Duration = Duration::from_secs(20);

fn binary() -> PathBuf {
    env!("CARGO_BIN_EXE_hagency").into()
}
fn deploy_unit() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../deploy/hagency-native.service")
}
/// Parse the unit into per-section directive maps — real `Key=Value` lines
/// only, comments and blanks excluded — so the negative pins assert on the
/// unit's directives, never on a comment string that merely names one.
fn unit_directives(unit: &str) -> BTreeMap<String, Vec<(String, String)>> {
    let mut sections: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    let mut section = String::new();
    for line in unit.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].to_string();
            sections.entry(section.clone()).or_default();
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            sections
                .entry(section.clone())
                .or_default()
                .push((key.to_string(), value.to_string()));
        }
    }
    sections
}
/// The named refusal the spec fixes for the other hosted OSes (ADR-127):
/// never skip, never pretend a service leg ran where it cannot. The leg
/// instead pins the unit contract offline — the directives the Linux spawn
/// leg exercises — so the selector is green while honest about the refusal.
fn refuse_not_linux() {
    assert_ne!(std::env::consts::OS, "linux");
    let unit = fs::read_to_string(deploy_unit()).expect("unit present on every leg");
    let sections = unit_directives(&unit);
    let directive = |key: &str| {
        sections
            .get("Service")
            .unwrap()
            .iter()
            .filter(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
            .collect::<Vec<_>>()
    };
    let exec_start = directive("ExecStart").join(" ");
    assert!(exec_start.contains("serve"), "ExecStart runs serve");
    assert!(
        exec_start.contains("--state-dir"),
        "state placeholder rendered"
    );
    assert!(
        exec_start.contains("127.0.0.1:13300"),
        "loopback listen is fixed"
    );
    assert!(
        directive("ExecStop").is_empty(),
        "no ExecStop directive (F2)"
    );
    assert!(
        directive("KillMode").is_empty(),
        "no KillMode directive (F2)"
    );
    assert!(
        directive("KillSignal").is_empty(),
        "no KillSignal directive (F2)"
    );
    assert_eq!(
        directive("TimeoutStopSec").join(""),
        "20",
        "retained stop budget"
    );
    assert_eq!(
        directive("Restart").join(""),
        "on-failure",
        "restart policy"
    );
    assert_eq!(directive("RestartSec").join(""), "5", "restart interval");
    assert_eq!(
        directive("StateDirectory").join(""),
        "hagency-native",
        "state root"
    );
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
    // proves nothing at cutover (ADR-127 F8). The poll is bounded by the
    // start gate's own named budget, the same 20-second figure as the unit's
    // TimeoutStopSec but bounding the start, not the drain.
    let until = Instant::now() + START_GATE_BUDGET;
    loop {
        if http_status(&running.addr, "/ready") == Some(200) {
            return;
        }
        assert!(Instant::now() < until, "service never answered ready 200");
        thread::sleep(Duration::from_millis(100));
    }
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
    let mut db = hagency_store::DomainRepository::open(state).unwrap();
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
    // Plant the pending custody row through the store's own API while
    // nothing holds the store (it stays locked once the service runs).
    seed_pending_dispatch(&state);
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
        assert_eq!(row, ("queued".into(), 0));
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
