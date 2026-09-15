//! ADR-140: the real-app-server qualification gate. The two evidence tests
//! read the tracked evidence file and FAIL — never skip — while it is a
//! placeholder, missing, stale, or records a non-passing verdict. The probe
//! echo test drives the offline fixture peer directly (no store) and asserts
//! the typed policy echo; it is protocol evidence only — no echo here is
//! evidence of effective OS sandboxing.
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const PINNED_CODEX: &str = "0.153.4";
const EVIDENCE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/qualification/codex-sandbox.json"
);

fn load_evidence() -> Value {
    let path = Path::new(EVIDENCE);
    let text = std::fs::read_to_string(path).unwrap_or_else(|error| {
        panic!(
            "qualification evidence file is missing at {EVIDENCE} (refusal: unreadable, {error}); \
             an operator must run `cargo run --locked -p hagency-execution --example codex_qualify` \
             with HAGENCY_CODEX_QUALIFY_BIN pointing at the pinned Codex {PINNED_CODEX} executable \
             and commit the rewritten evidence in the same commit"
        )
    });
    serde_json::from_str(&text)
        .unwrap_or_else(|error| panic!("qualification evidence is not valid JSON: {error}"))
}

fn refuse_placeholder(evidence: &Value) {
    assert_eq!(
        evidence["schema"], "hagency-codex-sandbox-qualification-v1",
        "qualification evidence schema is not the recorded shape"
    );
    if evidence["placeholder"] == json!(true) {
        panic!(
            "qualification evidence is a checked-in placeholder (refusal: no operator \
             qualification run recorded); reason recorded in the file: {}",
            evidence["placeholder_reason"].as_str().unwrap_or("unnamed")
        );
    }
}

fn validate_common(evidence: &Value) {
    refuse_placeholder(evidence);
    let pin = evidence["pinned_codex_version"]
        .as_str()
        .unwrap_or_default();
    assert_eq!(
        pin, PINNED_CODEX,
        "evidence pin moved without re-qualification"
    );
    let version = evidence["codex_version"].as_str().unwrap_or_default();
    assert!(
        version.contains(PINNED_CODEX),
        "qualified codex version {version:?} does not match the spec pin {PINNED_CODEX:?}; \
         re-run the codex_qualify example against the pinned binary"
    );
    let commit = evidence["commit"].as_str().unwrap_or_default();
    assert_eq!(
        commit.len(),
        40,
        "evidence records no full commit hash; freshness is unprovable"
    );
    assert!(
        commit.chars().all(|c| c.is_ascii_hexdigit()),
        "evidence commit {commit:?} is not a hex object name"
    );
    let recorded = evidence["recorded_at_ms"].as_u64().unwrap_or_default();
    assert!(recorded > 0, "evidence records no capture time");
    assert!(
        recorded < 4_102_444_800_000,
        "evidence capture time is not a plausible millisecond epoch"
    );
    let log = evidence["log_path"].as_str().unwrap_or_default();
    assert!(!log.is_empty(), "evidence records no log path");
    let os = evidence["host"]["os"].as_str().unwrap_or_default();
    let arch = evidence["host"]["arch"].as_str().unwrap_or_default();
    assert!(
        !os.is_empty() && !arch.is_empty(),
        "evidence records no host"
    );
}

#[test]
fn native_codex_real_app_server_sandbox_write_inside() {
    let evidence = load_evidence();
    validate_common(&evidence);
    let verdict = &evidence["verdicts"]["write_inside"];
    assert!(
        verdict["pass"] == json!(true),
        "write_inside verdict is not passing: {verdict}"
    );
    assert_eq!(
        verdict["outcome"], "completed",
        "write_inside turn did not complete"
    );
    assert_eq!(
        verdict["file_created"],
        json!(true),
        "the write-inside target file was not created inside the workspace"
    );
    assert!(
        !verdict["error"].as_str().is_some_and(str::is_empty),
        "write_inside records an empty error string"
    );
}

#[test]
fn native_codex_real_app_server_sandbox_refuses_outside() {
    let evidence = load_evidence();
    validate_common(&evidence);
    let verdict = &evidence["verdicts"]["refuses_outside"];
    assert!(
        verdict["pass"] == json!(true),
        "refuses_outside verdict is not passing: {verdict}"
    );
    assert_eq!(
        verdict["file_created"],
        json!(false),
        "a file appeared outside the workspace: the sandbox did not refuse"
    );
    assert!(
        verdict["approval_seen"] == json!(true)
            || verdict["error"].as_str().is_some_and(|v| !v.is_empty())
            || verdict["outcome"] != "completed",
        "the outside write neither requested approval nor failed: the refusal \
         mechanism is unnamed"
    );
}

/// Protocol-level policy echo through the offline fixture peer. This is the
/// probe class (ADR-140): the typed request the host builds carries the
/// default `workspace-write` sandbox and `on-request` approval policy, and
/// the peer echoes what it observed. A matching echo proves the wire shape
/// only — it is never evidence of effective OS sandboxing, which is exactly
/// why the two evidence tests above exist.
#[test]
fn native_codex_probe_sandbox_policy_echo() {
    let root = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_hagency-execution-probe"))
        .arg("app-server")
        .current_dir(root.path())
        .env_clear()
        .env("HAGENCY_OFFLINE_MODE", "normal")
        .env("HAGENCY_OPERATION_BUDGET_MS", "1000")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("offline fixture peer spawn");
    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);
    // The typed request exactly as `Settings::thread_request` builds it
    // (native/hagency-runtime/src/codex/session.rs): policy travels here,
    // never on argv (ADR-139).
    let thread_request = json!({
        "jsonrpc": "2.0", "id": 3, "method": "thread/start",
        "params": {
            "cwd": root.path().to_str().unwrap(),
            "sandbox": "workspace-write",
            "approvalPolicy": "on-request",
            "approvalsReviewer": "user",
            "ephemeral": true,
            "serviceName": "hagency"
        }
    });
    let requests = [
        json!({"jsonrpc":"2.0","id":1,"method":"initialize",
               "params":{"clientInfo":{"name":"hagency","title":"Hagency runner","version":"test"}}}),
        json!({"jsonrpc":"2.0","method":"initialized","params":{}}),
        thread_request,
    ];
    let sent = std::thread::spawn(move || {
        for request in requests {
            let mut bytes = serde_json::to_vec(&request).unwrap();
            bytes.push(b'\n');
            stdin.write_all(&bytes).unwrap();
            stdin.flush().unwrap();
        }
        // Dropping the writer closes the peer's stdin: its bounded hold ends
        // and the process exits instead of outliving the test.
    });
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut echo = None;
    while Instant::now() < deadline {
        let mut line = String::new();
        let read = reader.read_line(&mut line).unwrap_or(0);
        if read == 0 {
            break;
        }
        if let Ok(message) = serde_json::from_str::<Value>(&line)
            && message["id"] == json!(3)
            && message.get("result").is_some()
        {
            echo = Some(message);
            break;
        }
    }
    sent.join().unwrap();
    let _ = child.kill();
    let _ = child.wait();
    let echo = echo.unwrap_or_else(|| {
        panic!("fixture peer never echoed the thread/start result: policy echo is absent")
    });
    let result = &echo["result"];
    assert_eq!(
        result["approvalPolicy"], "on-request",
        "peer echoed a different approval policy"
    );
    assert_eq!(
        result["approvalsReviewer"], "user",
        "peer echoed a different approvals reviewer"
    );
    let sandbox = &result["sandbox"];
    assert_eq!(
        sandbox["type"], "workspaceWrite",
        "peer echoed a different sandbox mode"
    );
    // The echo is a wire-shape check only. It says nothing about the OS
    // enforcing anything — that proof belongs to the evidence-file tests.
}
