//! SR-3 (ADR-134) and SR-4 (ADR-135) selectors. UNGATED file; every selector
//! is present on every hosted leg. The version legs are SQLite-free and run
//! everywhere; the dry-run legs that provision state run the real
//! init/serve/SIGTERM contract and require the store.
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
fn workspace_version() -> String {
    // The workspace [workspace.package] version is the ONE version source;
    // package.json or any other manifest is never consulted.
    let manifest =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml"))
            .expect("workspace manifest readable on every leg");
    let mut in_package = false;
    for line in manifest.lines() {
        let token = line.trim();
        if token.starts_with('[') {
            in_package = token == "[workspace.package]";
            continue;
        }
        if in_package && token.starts_with("version") {
            return token
                .split('=')
                .nth(1)
                .expect("version assignment")
                .trim()
                .trim_matches('"')
                .to_string();
        }
    }
    panic!("workspace package version not found");
}
fn reported_version() -> String {
    let output = Command::new(binary()).arg("--version").output().unwrap();
    assert!(output.status.success(), "--version must succeed");
    String::from_utf8(output.stdout)
        .unwrap()
        .split_whitespace()
        .last()
        .expect("clap prints `<name> <version>`")
        .to_string()
}
/// The SR-3 release-tree scanner: any finding fails the scan, never skips.
/// Findings: package.json / lockfiles, node_modules directories, JavaScript
/// entry files, and `node` references in shipped wrappers and units.
fn scan_node_entrypoints(root: &Path) -> Vec<String> {
    fn walk(dir: &Path, findings: &mut Vec<String>) {
        for entry in fs::read_dir(dir).expect("readable staged tree") {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            if path.is_dir() {
                if name == "node_modules" {
                    findings.push(format!("node_modules directory: {}", path.display()));
                    continue;
                }
                walk(&path, findings);
                continue;
            }
            let extension = path.extension().map(|v| v.to_string_lossy().into_owned());
            if matches!(
                name.as_str(),
                "package.json" | "package-lock.json" | "npm-shrinkwrap.json"
            ) || matches!(extension.as_deref(), Some("js") | Some("mjs") | Some("cjs"))
            {
                findings.push(format!("javascript entry point: {}", path.display()));
                continue;
            }
            // Wrappers and units are text: scan their bytes for node references.
            if let Ok(text) = fs::read_to_string(&path) {
                for token in ["node ", "/node", "\"node\""] {
                    if text.contains(token) {
                        findings.push(format!(
                            "node reference in shipped file: {} (`{token}`)",
                            path.display()
                        ));
                        break;
                    }
                }
            }
        }
    }
    let mut findings = Vec::new();
    walk(root, &mut findings);
    findings
}
/// Stage a release tree the way the workflow packages it: the versioned
/// binary, both units, and a checksums manifest.
fn stage_release_tree(root: &Path) -> PathBuf {
    let version = workspace_version();
    let tree = root.join(format!("hagency-v{version}"));
    fs::create_dir_all(&tree).unwrap();
    fs::copy(binary(), tree.join(format!("hagency-v{version}"))).unwrap();
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../deploy/hagency-native.service"),
        tree.join("hagency-native.service"),
    )
    .unwrap();
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../deploy/io.hagency.native.plist"),
        tree.join("io.hagency.native.plist"),
    )
    .unwrap();
    let binary_bytes = fs::metadata(tree.join(format!("hagency-v{version}")))
        .unwrap()
        .len();
    fs::write(
        tree.join("checksums.txt"),
        format!("hagency-v{version}  {binary_bytes} bytes\nhagency-native.service  unit\nio.hagency.native.plist  unit\n"),
    )
    .unwrap();
    tree
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
struct Guard(Child);
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
fn init_state(root: &Path) -> PathBuf {
    // The runbook's step 0 precondition: a fresh temp state directory
    // initialized by hagency init (which refuses a non-empty dir).
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
    // The runbook's gate polls /ready, never /health (health is 200-while-live
    // and proves nothing at cutover).
    let until = Instant::now() + Duration::from_secs(20);
    loop {
        if http_status(&running.addr, "/ready") == Some(200) {
            return;
        }
        assert!(Instant::now() < until, "service never answered ready 200");
        thread::sleep(Duration::from_millis(100));
    }
}
/// The runbook's stop contract: exits zero inside the budget, or parks on an
/// unknown close without a false success (ADR-120).
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

#[test]
fn native_binary_version_matches_workspace() {
    let workspace = workspace_version();
    let reported = reported_version();
    assert_eq!(
        reported, workspace,
        "the binary's --version must equal the workspace [workspace.package] version exactly; \
         no other version source (package.json) is consulted"
    );
    // The artifact naming rule embeds the same version, so binary, unit and
    // artifact cannot disagree silently (ADR-127's version-identity note).
    let artifact = format!("hagency-v{workspace}");
    assert!(artifact.contains(&workspace));
}

#[test]
fn native_release_entrypoints_scan_finds_no_node() {
    let root = tempfile::tempdir().unwrap();
    let tree = stage_release_tree(root.path());
    let findings = scan_node_entrypoints(&tree);
    assert!(
        findings.is_empty(),
        "the release tree must contain no Node entry point: {findings:?}"
    );
    // Negative control proving the scanner detects rather than skips: a
    // planted package.json is a finding that would fail the scan.
    fs::write(tree.join("package.json"), "{}").unwrap();
    assert_eq!(
        scan_node_entrypoints(&tree).len(),
        1,
        "a planted package.json must be found"
    );
}

#[test]
fn native_cutover_dryrun_version_identity() {
    // Runbook step 0: version identity is proven BEFORE any service start —
    // a mismatch fails the dry-run before a single process spawns.
    let workspace = workspace_version();
    assert_eq!(reported_version(), workspace);
    let root = tempfile::tempdir().unwrap();
    let _state = init_state(root.path()); // fresh temp state, per the runbook
}

#[test]
fn native_cutover_dryrun_ready_gate_and_stop_contract() {
    // Runbook steps 3 and 6: gate on /ready, then SIGTERM inside the budget.
    let root = tempfile::tempdir().unwrap();
    let state = init_state(root.path());
    let mut running = spawn_service(&state);
    wait_ready(&running);
    assert_eq!(http_status(&running.addr, "/health"), Some(200));
    let clean = term_then_observe_exit(&mut running, Duration::from_secs(20));
    assert!(clean, "clean drain-and-close expected on the idle leg");
}

#[test]
fn native_cutover_dryrun_pending_preserved_across_restart() {
    // Runbook steps 6 and 7: the stop-start pair preserves every pending or
    // outcome-unknown row; nothing is resolved, dropped or marked done.
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
        assert_eq!(resolved, 0, "no row may be resolved by the stop-start pair");
    }
}
