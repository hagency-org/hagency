//! Codex launch-surface argv and stdio pins (brief 29; ADR-139).
//!
//! `native_codex_argv_is_app_server_only` — the host's REAL prepared launch
//! argv, recorded by the offline fixture child it spawns, is EXACTLY one
//! argument, `app-server`; sandbox, approval and directory policy travel in
//! the typed initialize request, never on argv. A host regression adding any
//! flag to the prepared launch turns this red.
//!
//! `native_codex_stdio_flag_matches_pinned_cli` — the pinned Codex CLI's
//! app-server transport default must remain stdio, read from the checked-in
//! captured help output (`tests/fixtures/codex-app-server-help.txt`), AND the
//! host's real argv must carry no transport flag, so that documented default
//! is the one in force. When either half drifts, this probe fails and
//! reopens ADR-139.
//!
//! Neither selector spawns a real Codex CLI: both run the host against the
//! offline fixture binary (`hagency-execution-probe`), whose `app-server`
//! entry records the argv it was spawned with in `owned-dispatch.argv`
//! inside the leased workspace; the stdio selector also reads the captured
//! metadata.
#[path = "../../hagency-store/tests/common/mod.rs"]
mod common;
use common::*;
use hagency_core::tasks::*;
use hagency_execution::{Host, Limits, Operation, Protocol};
use hagency_store::{DomainRepository, DomainStore, EffectOutcome};
use serde_json::json;
use std::{
    collections::BTreeMap,
    time::{SystemTime, UNIX_EPOCH},
};

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

struct ScopeFixture {
    _root: tempfile::TempDir,
    work: std::path::PathBuf,
    domain: DomainStore,
    cap: RunnerCapability,
}

/// A claimed dispatch whose scope the host can prepare, over the offline
/// fixture binary — the same shape `tests/owned.rs` uses, reduced to what
/// `prepare` reads.
fn scope_fixture() -> ScopeFixture {
    let root = tempfile::tempdir().unwrap();
    let mut db = DomainRepository::open(&root.path().join("state")).unwrap();
    db.register(&registration()).unwrap();
    let pool = resource("pool", "seat", 1000);
    db.put_resource(&pool).unwrap();
    let proof = proof(&request("argv", "Worker", &pool, 100));
    let engagement = db.admit(&proof, 1000).unwrap();
    db.approve("approve", &proof, 1000).unwrap();
    let effect = db.claim_effect().unwrap().unwrap();
    db.observe_effect(
        &effect.id,
        effect.fence,
        &EffectOutcome::Applied {
            receipt: "offline".into(),
        },
    )
    .unwrap();
    db.register_session(&SessionBinding {
        id: "session".into(),
        engagement_id: engagement.id,
        room_id: "!project:example.test".into(),
        thread_root: Some("$thread".into()),
    })
    .unwrap();
    db.register_workspace("work").unwrap();
    db.create_canonical_task("task", "session", "argv probe", now())
        .unwrap();
    db.enqueue_dispatch(&DispatchInput {
        id: "dispatch".into(),
        session_id: "session".into(),
        task_id: Some("task".into()),
        resources: vec![ResourceLease {
            id: "work".into(),
            exclusive: true,
        }],
        payload: json!({"instruction":"offline"}),
    })
    .unwrap();
    let cap = db
        .claim_dispatch("host", now(), 60_000, 60_000, 1)
        .unwrap()
        .unwrap();
    let domain = DomainStore::start(db, 16).unwrap();
    let work = root.path().join("workspace");
    hagency_store::private::directory(&work).unwrap();
    let work = work.canonicalize().unwrap();
    ScopeFixture {
        _root: root,
        work,
        domain,
        cap,
    }
}

impl ScopeFixture {
    fn host(&self) -> Host {
        // The REAL offline fixture binary the host installs, not the test
        // harness binary: only its `app-server` entry speaks the protocol
        // and records the argv the host spawned it with.
        let binary: std::path::PathBuf = env!("CARGO_BIN_EXE_hagency-execution-probe").into();
        let mut environment = BTreeMap::from([
            ("PATH".into(), "".into()),
            ("HAGENCY_OFFLINE_MODE".into(), "normal".into()),
        ]);
        if let Some(system) = std::env::var_os("SystemRoot") {
            environment.insert("SystemRoot".into(), system);
        }
        Host::new(
            binary.clone(),
            binary,
            environment,
            BTreeMap::from([("work".into(), self.work.clone())]),
        )
        .unwrap()
    }
}

/// Run one real operation through the offline fixture child and return the
/// argv the HOST actually spawned it with, as recorded by the child itself
/// (`owned-dispatch.argv` in the leased workspace). The test never builds a
/// `Launch` of its own; the child is spawned with exactly what `prepare`
/// built, so this is the host-side truth.
async fn recorded_argv() -> Vec<String> {
    let f = scope_fixture();
    let mut operation = Operation::start(
        f.domain.clone(),
        f.cap.clone(),
        f.host(),
        Limits {
            operation_ms: 25_000,
            response_ms: 2_000,
        },
    )
    .unwrap();
    let report = operation.wait().await.unwrap();
    assert!(
        report.protocol == Protocol::Completed,
        "fixture child did not complete: {:?}",
        report.failure
    );
    let raw = std::fs::read(f.work.join("owned-dispatch.argv"))
        .expect("fixture child records the argv it was spawned with");
    serde_json::from_slice(&raw).unwrap()
}

#[tokio::test]
async fn native_codex_argv_is_app_server_only() {
    let argv = recorded_argv().await;
    // Exact equality carries every clause of the scenario: one argument,
    // exactly `app-server`, and therefore no sandbox, approval, directory or
    // transport flag anywhere on argv. A host regression adding `--stdio`
    // or any policy flag to the prepared launch turns this red with the
    // recorded argv printed.
    assert_eq!(argv, ["app-server"], "host spawn argv drifted: {argv:?}");
}

/// The pinned CLI probe: offline, over the captured `app-server --help`
/// metadata, crossed with the host's real prepared argv. The pinned default
/// `--listen` must remain `stdio://`, and the host's argv must carry no
/// transport flag at all, so that documented default is the transport
/// actually in force.
#[tokio::test]
async fn native_codex_stdio_flag_matches_pinned_cli() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/codex-app-server-help.txt"
    );
    let help = std::fs::read_to_string(path).expect("captured pinned CLI help");
    let mut lines = help.lines();
    let version = lines
        .find(|l| l.trim_start().starts_with("codex-cli"))
        .expect("pinned version line")
        .trim()
        .to_owned();
    // The pin is the version the excerpt was ACTUALLY captured from:
    // 0.154.0 on 2026-09-13 (the protocol spec separately pins 0.153.4 for
    // wire envelopes). Strict equality — a re-captured excerpt from any
    // other version fails here and must be re-pinned together with ADR-139
    // and this spec scenario, never silently.
    assert_eq!(
        version, "codex-cli 0.154.0",
        "the captured app-server help excerpt is no longer from the pinned \
         0.154.0 surface; re-pin it together with ADR-139 and the spec"
    );
    // The pinned CLI surface, read once from the captured help.
    let listen_default = help.lines().find(|l| l.trim() == "[default: stdio://]");
    assert!(
        listen_default.is_some(),
        "pinned CLI {version} no longer defaults --listen to stdio://; ADR-139 is reopened"
    );
    // The host half: the real prepared argv carries no transport flag, so
    // the pinned default above is the one in force for every launch.
    let argv = recorded_argv().await;
    assert!(
        !argv
            .iter()
            .any(|a| a == "--stdio" || a.starts_with("--listen")),
        "host argv carries a transport flag: {argv:?}; the bare-argv stdio pin no longer holds"
    );
}
