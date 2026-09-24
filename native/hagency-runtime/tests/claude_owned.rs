use hagency_platform::Launch;
use hagency_runtime::claude::session::ObservationKind;
use hagency_runtime::claude::session::{ApprovalControlPolicy, PermissionDecision, PreparedUpdate};
use hagency_runtime::{
    claude::{
        EventKind, Message,
        session::{Error, Limits, Phase},
    },
    owned::{Cleanup, OwnedClaudeSession, StartError},
};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

fn binary() -> PathBuf {
    env!("CARGO_BIN_EXE_hagency-runtime-probe").into()
}
fn launch(root: &Path, mode: &str, marker: &Path) -> Launch {
    let mut environment = BTreeMap::new();
    environment.insert("PATH".into(), "".into());
    if let Some(root) = std::env::var_os("SystemRoot") {
        environment.insert("SystemRoot".into(), root);
    }
    Launch {
        executable: binary(),
        arguments: vec!["fake-claude".into(), mode.into(), marker.into()],
        directory: root.into(),
        environment,
        require_crash_containment: false,
    }
}
fn limits() -> Limits {
    Limits {
        write_timeout_ms: 1000,
        event_wait_ms: 2000,
        lifetime_ms: 10_000,
    }
}
fn spawn(root: &Path, mode: &str, marker: &Path) -> OwnedClaudeSession {
    OwnedClaudeSession::spawn(&binary(), &launch(root, mode, marker), limits()).unwrap()
}
fn cleanup(value: Cleanup) {
    let Cleanup::Observed(report) = value else {
        panic!("fixture stop must be observed: {value:?}")
    };
    assert!(report.scope.leader_exited);
    assert!(report.scope.signals_accepted);
    assert_eq!(
        report.scope.whole_tree_stopped,
        cfg!(any(target_os = "linux", target_os = "macos", windows))
    );
}
fn pulse(marker: &Path) -> u64 {
    fs::metadata(marker.with_extension("pulse")).map_or(0, |value| value.len())
}
fn live(marker: &Path) {
    let before = pulse(marker);
    let until = Instant::now() + Duration::from_secs(2);
    while pulse(marker) <= before {
        assert!(Instant::now() < until, "offline peer must still be live");
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn stopped(marker: &Path) {
    let before = pulse(marker);
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(pulse(marker), before);
}
#[tokio::test]
async fn native_claude_owned_usage() {
    let root = tempfile::tempdir().unwrap();
    let marker = root.path().join("usage");
    let mut runner = spawn(root.path(), "usage", &marker);
    runner.initialize().await.unwrap();
    runner.prompt("offline usage").await.unwrap();
    runner.next_message().await.unwrap();
    let source = runner.observation_source().unwrap();
    assert!(runner.matches_observation_source(&source));
    for sequence in 2..=6 {
        runner.next_message().await.unwrap();
        let event = runner.last_observation().unwrap();
        assert_eq!(event.sequence(), sequence);
        assert!(event.source() == &source);
        match sequence {
            2 | 5 => assert!(
                matches!(event.kind(),ObservationKind::Usage(e) if e.counts().output().is_none())
            ),
            3 | 4 => assert!(matches!(event.kind(), ObservationKind::Ignored)),
            6 => assert!(
                matches!(event.kind(),ObservationKind::Result {usage,is_error:true} if usage.counts().input()==Some(101))
            ),
            _ => unreachable!(),
        }
    }
    assert_eq!(runner.cleanup(), Cleanup::Pending);
    assert!(!source.is_retired());
    live(&marker);
    cleanup(runner.stop());
    assert!(source.is_retired());
    stopped(&marker);
}
#[tokio::test]
async fn native_claude_owned_lifecycle() {
    let root = tempfile::tempdir().unwrap();
    let marker = root.path().join("normal");
    let mut runner = spawn(root.path(), "normal", &marker);
    assert!(runner.id() > 1);
    runner.initialize().await.unwrap();
    runner.prompt("owned stdin 中文 $(literal)").await.unwrap();
    for expected in [EventKind::System, EventKind::Assistant, EventKind::Result] {
        assert!(
            matches!(runner.next_message().await.unwrap(),Message::Event {kind,..} if kind==expected)
        );
    }
    assert_eq!(runner.phase(), Phase::ResultObserved);
    assert_eq!(runner.session_id(), Some("owned-claude"));
    assert_eq!(runner.cleanup(), Cleanup::Pending);
    assert!(runner.termination().is_none());
    live(&marker);
    let prompt: serde_json::Value =
        serde_json::from_slice(&fs::read(marker.with_extension("prompt")).unwrap()).unwrap();
    assert_eq!(prompt["message"]["content"], "owned stdin 中文 $(literal)");
    cleanup(runner.stop());
    stopped(&marker);
    assert_eq!(runner.initialize().await, Err(Error::Closed));
}

#[tokio::test]
async fn native_claude_owned_failure_and_cancel() {
    let root = tempfile::tempdir().unwrap();
    for mode in ["malformed", "stall", "cancel", "drop"] {
        let marker = root.path().join(mode);
        let mut runner = spawn(
            root.path(),
            match mode {
                "malformed" => mode,
                "drop" => "normal",
                _ => "stall",
            },
            &marker,
        );
        match mode {
            "malformed" => assert!(matches!(runner.initialize().await, Err(Error::Protocol(_)))),
            "stall" => assert_eq!(runner.initialize().await, Err(Error::Timeout)),
            "cancel" => {
                assert!(
                    tokio::time::timeout(Duration::from_millis(100), runner.initialize())
                        .await
                        .is_err()
                );
                assert_eq!(runner.termination().unwrap().cause, Error::Cancelled);
            }
            "drop" => {
                runner.initialize().await.unwrap();
                runner.prompt("offline drop witness").await.unwrap();
                for _ in 0..3 {
                    runner.next_message().await.unwrap();
                }
                live(&marker);
                // A live pulse after result proves this tests owner Drop, not
                // merely a peer that exited by itself before the assertion.
                drop(runner);
                stopped(&marker);
                continue;
            }
            _ => unreachable!(),
        }
        cleanup(runner.cleanup());
        stopped(&marker);
    }
    let marker = root.path().join("invalid");
    let request = launch(root.path(), "normal", &marker);
    assert!(matches!(
        OwnedClaudeSession::spawn(
            &binary(),
            &request,
            Limits {
                lifetime_ms: 0,
                ..limits()
            }
        ),
        Err(StartError::Settings)
    ));
    assert!(!marker.with_extension("entered").exists());
    let mut missing = launch(root.path(), "normal", &marker);
    missing.executable = root.path().join("absent.exe");
    assert!(matches!(
        OwnedClaudeSession::spawn(&binary(), &missing, limits()),
        Err(StartError::Uncertain { .. })
    ));
}

#[tokio::test]
async fn native_claude_owned_permission_roundtrip() {
    let root = tempfile::tempdir().unwrap();
    for mode in [
        "permission-allow",
        "permission-deny",
        "permission-cancel",
        "permission-hold",
    ] {
        let marker = root.path().join(mode);
        let mut runner = spawn(root.path(), mode, &marker);
        runner.initialize().await.unwrap();
        runner.prompt("offline permission test").await.unwrap();
        assert!(matches!(
            runner.next_message().await.unwrap(),
            Message::Event {
                kind: EventKind::System,
                ..
            }
        ));
        runner
            .enable_approval_control(ApprovalControlPolicy {
                owner_wait_ms: 3000,
                response_reserve_ms: 1000,
            })
            .unwrap();
        assert!(
            matches!(runner.next_message().await.unwrap(),Message::Permission {request_id,..} if request_id=="owned-permission")
        );
        if mode == "permission-hold" {
            live(&marker);
            let pending = std::future::pending::<()>();
            tokio::pin!(pending);
            assert!(
                tokio::time::timeout(
                    Duration::from_millis(30),
                    runner.next_or_control(pending.as_mut())
                )
                .await
                .is_err()
            );
            assert_eq!(runner.termination().unwrap().cause, Error::Cancelled);
            cleanup(runner.cleanup());
            stopped(&marker);
            continue;
        }
        let decision = if mode == "permission-deny" {
            PermissionDecision::Deny
        } else {
            PermissionDecision::Allow
        };
        let mut prepared = runner
            .prepare_approval("owned-permission", decision)
            .unwrap();
        if mode == "permission-cancel" {
            let until = Instant::now() + Duration::from_secs(2);
            while !marker.with_extension("cancelled").is_file() {
                assert!(
                    Instant::now() < until,
                    "offline cancellation marker must arrive"
                );
                std::thread::sleep(Duration::from_millis(5));
            }
            assert!(matches!(
                runner.send_prepared_approval(&mut prepared).await.unwrap(),
                PreparedUpdate::Message(Message::ControlCancel { .. })
            ));
            assert_eq!(runner.write_progress().unwrap().accepted_bytes, 0);
            assert!(matches!(
                runner.send_prepared_approval(&mut prepared).await,
                Err(Error::PermissionUnavailable)
            ));
            assert_eq!(
                runner
                    .termination()
                    .unwrap()
                    .unconfirmed_write
                    .unwrap()
                    .accepted_bytes,
                0
            );
            cleanup(runner.cleanup());
            stopped(&marker);
            continue;
        }
        // The peer answers the instant it has read the response, so on a fast
        // runner its next event can be observed before this host observes its own
        // flush (hosted Ubuntu: three runs of four). The send is re-entrant for
        // exactly that: it hands back what arrived and resumes the same frame.
        // Both orders are one exchange, so keep sending until the write is
        // acknowledged and keep what arrived first, in order.
        let mut early = std::collections::VecDeque::new();
        loop {
            match runner.send_prepared_approval(&mut prepared).await.unwrap() {
                PreparedUpdate::WriteAccepted(progress) => {
                    assert!(progress.flushed);
                    break;
                }
                PreparedUpdate::Message(message) => early.push_back(message),
            }
            assert!(
                early.len() <= 2,
                "only the two answer events may precede the receipt"
            );
        }
        for expected in [EventKind::Assistant, EventKind::Result] {
            let message = match early.pop_front() {
                Some(message) => message,
                None => runner.next_message().await.unwrap(),
            };
            assert!(matches!(message,Message::Event {kind,..} if kind==expected));
        }
        assert!(early.is_empty());
        let received: serde_json::Value =
            serde_json::from_slice(&fs::read(marker.with_extension("response")).unwrap()).unwrap();
        assert_eq!(received["response"]["request_id"], "owned-permission");
        assert_eq!(
            received["response"]["response"]["behavior"],
            if decision == PermissionDecision::Allow {
                "allow"
            } else {
                "deny"
            }
        );
        assert!(
            received["response"]["response"]
                .get("updatedPermissions")
                .is_none()
        );
        assert_eq!(runner.cleanup(), Cleanup::Pending);
        live(&marker);
        cleanup(runner.stop());
        stopped(&marker);
    }
}
