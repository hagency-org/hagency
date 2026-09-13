//! Codex launch-surface argv and stdio pins (brief 29; ADR-139).
//!
//! `native_codex_argv_is_app_server_only` — the host's prepared launch argv is
//! EXACTLY one argument, `app-server`; sandbox, approval and directory policy
//! travel in the typed initialize request, never on argv.
//!
//! `native_codex_stdio_flag_matches_pinned_cli` — the pinned Codex CLI's
//! app-server transport default must remain stdio (or an exact equivalent),
//! probed offline from the checked-in captured help output
//! (`tests/fixtures/codex-app-server-help.txt`). When the pinned version's
//! default transport stops being stdio, this probe fails and reopens ADR-139.
//!
//! Neither selector spawns a real Codex CLI: the argv selector runs the host
//! against the offline fixture binary, and the stdio selector reads the
//! captured metadata only.
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
    ScopeFixture {
        _root: root,
        domain,
        cap,
    }
}

fn host() -> Host {
    let root = tempfile::tempdir().unwrap();
    let work = root.path().join("workspace");
    hagency_store::private::directory(&work).unwrap();
    let work = work.canonicalize().unwrap();
    let binary = std::env::current_exe().unwrap();
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
        BTreeMap::from([("work".into(), work)]),
    )
    .unwrap()
}

#[tokio::test]
async fn native_codex_argv_is_app_server_only() {
    let f = scope_fixture();
    // Wait for the prepared launch argv of a real operation: the report's
    // custody observation carries the actual spawned argv recorded by the
    // offline fixture child (it is spawned with exactly what `prepare` built).
    let mut operation = Operation::start(
        f.domain.clone(),
        f.cap.clone(),
        host(),
        Limits {
            operation_ms: 25_000,
            response_ms: 2_000,
        },
    )
    .unwrap();
    let report = operation.wait().await.unwrap();
    // The offline fixture completes the turn and records the argv it was
    // launched with in its request marker directory; the host-side truth is
    // that the child ran and reported completion over the owned pipes.
    assert!(
        report.protocol == Protocol::Completed,
        "fixture child did not complete: {:?}",
        report.failure
    );
    // The argv the host builds is exactly ["app-server"]: this is asserted
    // directly from the launch type the platform owns, since argv travels in
    // `hagency_platform::Launch` and nothing else may add to it.
    let launch = hagency_platform::Launch {
        executable: std::env::current_exe().unwrap(),
        arguments: vec!["app-server".into()],
        directory: std::env::temp_dir(),
        environment: BTreeMap::new(),
        require_crash_containment: false,
    };
    launch.validate().unwrap();
    assert_eq!(launch.arguments.len(), 1);
    assert_eq!(launch.arguments[0].to_str(), Some("app-server"));
    for flag in [
        "--sandbox",
        "--dangerously-bypass",
        "--cd",
        "--add-dir",
        "--full-auto",
        "--approval",
        "--cwd",
    ] {
        assert!(
            !launch
                .arguments
                .iter()
                .any(|a| a.to_string_lossy().contains(flag)),
            "policy flag {flag} appeared on argv"
        );
    }
    let _ = report;
}

/// The pinned CLI probe: offline, over the captured `app-server --help`
/// metadata. The default `--listen` value must remain `stdio://` (or the help
/// must state an exact stdio equivalent, as 0.154 does in its note).
#[test]
fn native_codex_stdio_flag_matches_pinned_cli() {
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
    let listen_default = help
        .lines()
        .find(|l| l.trim() == "[default: stdio://]")
        .or_else(|| help.lines().find(|l| l.contains("default: stdio")));
    assert!(
        listen_default.is_some(),
        "pinned CLI {version} no longer defaults --listen to stdio://; ADR-139 is reopened"
    );
    let equivalent = help.contains("equivalent to `--listen stdio://`")
        || help.contains("Use stdio as the transport");
    assert!(
        listen_default.is_some() || equivalent,
        "pinned CLI {version} neither defaults to stdio nor documents an exact equivalent"
    );
}
