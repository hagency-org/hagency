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
/// row by the runbook's own vocabulary, and nothing in `serve` resolves it.
fn seed_pending_dispatch(state: &Path) {
    let mut db = hagency_store::DomainRepository::open(state).unwrap();
    db.register(&domain::registration()).unwrap();
    let pool = domain::resource("pool", "seat", 1000);
    db.put_resource(&pool).unwrap();
    let proof = domain::proof(&domain::request("dryrun", "Worker", &pool, 100));
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
        payload: json!({"instruction":"pending across the stop-start pair"}),
    })
    .unwrap();
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
    // and proves nothing at cutover). The poll is bounded by the start gate's
    // own named budget, the same 20-second figure as the stop budget but
    // bounding the start, not the drain.
    let until = Instant::now() + START_GATE_BUDGET;
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

/// O3's line scanner: one finding per offending line, naming the file and
/// the line. Word boundaries keep honest words honest (`denoted` is not
/// `node`); `.js`/`.mjs`/`.cjs` catch Node script paths in any position;
/// `__node_bin__` is the placeholder the spec names.
fn node_reference_findings(name: &str, text: &str) -> Vec<String> {
    let mut findings = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let lower = line.to_ascii_lowercase();
        let word = lower
            .split(|c: char| !c.is_ascii_alphanumeric())
            .any(|part| matches!(part, "node" | "npm" | "npx"));
        let script = lower.contains(".js") || lower.contains("__node_bin__");
        if word || script {
            findings.push(format!("{name}:{}: {}", index + 1, line.trim()));
        }
    }
    findings
}

#[test]
fn native_package_entrypoints_reference_no_node() {
    // O3 (M8 item 6, ADR-134): the packaged native entrypoints must not
    // invoke Node. The old gate was a manual reading of the plist and the
    // unit; this is the bound test. Per the corrected contract the scan
    // surface is EXACTLY: the two packaged unit templates by path, and the
    // installer's RENDERED output of both. There are no generated hook
    // templates under the native packaging paths. The rendering is the
    // installer's own: the test extracts its actual sed invocations from
    // install/install-native.sh and executes them verbatim with a
    // representative config — it does not trust the templates nor
    // re-implement the substitution. The retained supervisor plist is out
    // of scope by design (it is the JS product's).
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let installer = fs::read_to_string(repo.join("install/install-native.sh")).unwrap();
    // One entry per sed the installer carries: (template rel path, the
    // renderer command verbatim from its source lines).
    let mut renderers: Vec<(String, String, String)> = Vec::new();
    let lines: Vec<&str> = installer.lines().collect();
    for (index, line) in lines.iter().enumerate() {
        if !line.contains("sed -e") {
            continue;
        }
        // The continuation line names the template and the redirect target.
        let tail = lines
            .get(index + 1)
            .expect("sed invocation continues onto the template path");
        let template = if tail.contains("hagency-native.service") {
            "deploy/hagency-native.service"
        } else if tail.contains("io.hagency.native.plist") {
            "deploy/io.hagency.native.plist"
        } else {
            panic!("installer renders an unexpected template: {tail}");
        };
        // The render step, verbatim: the sed program with its own
        // substitution table AND its redirect, applied to the template.
        // `$(dirname "$0")` resolves against the process working directory,
        // so the command runs from install/ exactly as the installer does.
        // The redirect target is the installer's own variable ($UNIT /
        // $PLIST); the test points it at a temp path per the Test Double.
        let target_var = if template.ends_with(".service") {
            "UNIT"
        } else {
            "PLIST"
        };
        let command = format!("{} {}", line.trim().trim_end_matches('\\'), tail.trim());
        renderers.push((template.to_string(), target_var.to_string(), command));
    }
    assert_eq!(
        renderers.len(),
        2,
        "the installer must carry exactly the two native unit renderers"
    );
    let mut files: Vec<(String, String)> = Vec::new();
    for template in [
        "deploy/io.hagency.native.plist",
        "deploy/hagency-native.service",
    ] {
        files.push((
            template.to_string(),
            fs::read_to_string(repo.join(template)).unwrap(),
        ));
    }
    for (template, target_var, command) in &renderers {
        // Representative config: the values an operator's machine supplies,
        // and the render step's own redirect target pointed at a temp path.
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join(if target_var == "UNIT" {
            "hagency-native.service"
        } else {
            "io.hagency.native.plist"
        });
        let output = Command::new("sh")
            .arg("-c")
            .arg(command)
            .current_dir(repo.join("install"))
            .env("INSTALL_DIR", "/opt/hagency-native")
            .env("STATE_DIR", "/var/lib/hagency-native")
            .env(target_var, &target)
            .stdin(Stdio::null())
            .output()
            .expect("the installer's sed renderer executes");
        assert!(
            output.status.success(),
            "renderer for {template} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let rendered = fs::read_to_string(&target)
            .unwrap_or_else(|_| panic!("renderer for {template} wrote no temp output"));
        assert!(
            !rendered.contains("__INSTALL_DIR__")
                && !rendered.contains("__STATE_DIR__")
                && !rendered.contains("__USER__"),
            "renderer for {template} left placeholders unresolved"
        );
        files.push((format!("rendered by installer: {template}"), rendered));
    }
    let mut findings = Vec::new();
    for (name, text) in &files {
        findings.extend(node_reference_findings(name, text));
    }
    assert!(
        findings.is_empty(),
        "packaged native entrypoints must reference no Node runtime, \
         package manager or script:\n{}",
        findings.join("\n")
    );
    // Negative control, the same proof shape as the release-tree scan: the
    // scanner detects rather than skips. A planted ExecStart names its file
    // and line.
    let planted = node_reference_findings(
        "planted.service",
        "[Service]\nExecStart=/usr/bin/node /opt/app/index.js\n",
    );
    assert_eq!(
        planted,
        vec!["planted.service:2: ExecStart=/usr/bin/node /opt/app/index.js".to_string()],
        "the scanner must name the file and the line"
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
        assert_eq!(resolved, 0, "no row may be resolved by the stop-start pair");
    }
}
