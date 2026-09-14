//! The CLI `account login --id` production route (G4 step 1 & 2).
//!
//! Drives the REAL binary — `hagency account login` — with the fake login
//! binary (`hagency-login-probe`), so the test exercises the production
//! path (begin -> prepare_login/apply_codex_environment -> spawn -> settle),
//! never the store's own receipt helper. The readiness answer is read back
//! through `DomainRepository::account_readiness` on the SAME state dir.
use hagency_store::{AccountReadinessMode, DomainRepository};
use std::{
    fs,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_hagency"))
        .args(args)
        .env("PATH", "")
        .env("HOME", "/untrusted-fixture-home")
        .env("CODEX_HOME", "/untrusted-fixture-codex")
        .env("OPENAI_API_KEY", "offline-fixture-ambient-key")
        .output()
        .unwrap()
}

fn prepare(state: &Path) -> String {
    let output = run(&[
        "init",
        "--state-dir",
        state.to_str().unwrap(),
    ]);
    assert!(output.status.success(), "{:?}", output);
    let output = run(&[
        "account",
        "prepare",
        "--state-dir",
        state.to_str().unwrap(),
    ]);
    assert!(output.status.success(), "{:?}", output);
    let choices: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    choices[0]["id"].as_str().unwrap().to_owned()
}

fn readiness(state: &Path, id: &str) -> AccountReadinessMode {
    let db = DomainRepository::open(state).unwrap();
    db.account_readiness(id, now()).unwrap().mode
}

#[test]
fn native_account_login_route_records_ready() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    let id = prepare(&state);
    let output = run(&[
        "account",
        "login",
        "--state-dir",
        state.to_str().unwrap(),
        "--id",
        &id,
        "--login-binary",
        env!("CARGO_BIN_EXE_hagency-login-probe"),
    ]);
    assert!(
        output.status.success(),
        "login route failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    // The fake binary proved it ran in the retained namespace (HOME set from
    // the binding) and left its observation marker there.
    assert!(state.join(&id).join("login-observed.json").is_file());
    // The settled receipt is the observed mode, so the account reads ready.
    assert_eq!(readiness(&state, &id), AccountReadinessMode::Subscription);
}

#[test]
fn native_account_login_route_refused_never_ready() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    let id = prepare(&state);
    // The fake binary refuses when the retained namespace carries this marker,
    // exactly as a provider that declines the login would.
    fs::write(state.join(&id).join("login-outcome"), "refuse").unwrap();
    let output = run(&[
        "account",
        "login",
        "--state-dir",
        state.to_str().unwrap(),
        "--id",
        &id,
        "--login-binary",
        env!("CARGO_BIN_EXE_hagency-login-probe"),
    ]);
    assert!(
        output.status.success(),
        "login route failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    // A refused login records `refused`, which never reads as ready.
    assert_eq!(readiness(&state, &id), AccountReadinessMode::Unknown);
}
