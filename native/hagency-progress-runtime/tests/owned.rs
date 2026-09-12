use hagency_platform::Launch;
use hagency_progress::{Filter, PendingState, RunId};
use hagency_progress_runtime::{Attachment, Error, Retirement};
use hagency_runtime::{
    codex::{
        session::{Outcome, Settings, Update},
        transport::Limits,
    },
    owned::{Cleanup, OwnedSession},
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Duration,
};
fn run() -> RunId {
    RunId::new("owned-host-run".into()).unwrap()
}
fn spawn(root: &Path, mode: &str) -> OwnedSession {
    let binary: PathBuf = env!("CARGO_BIN_EXE_hagency-progress-probe").into();
    let mut environment = BTreeMap::new();
    environment.insert("PATH".into(), "".into());
    if let Some(value) = std::env::var_os("SystemRoot") {
        environment.insert("SystemRoot".into(), value);
    }
    OwnedSession::spawn(
        &binary,
        &Launch {
            executable: binary.clone(),
            arguments: vec![mode.into(), root.join("pulse").into_os_string()],
            directory: root.into(),
            environment,
            require_crash_containment: false,
        },
        Settings::new(root.into(), "offline-fixture".into(), "medium".into()).unwrap(),
        Limits {
            write_timeout_ms: 500,
            event_wait_ms: 1500,
            lifetime_ms: 10_000,
        },
        1500,
    )
    .unwrap()
}
async fn running(owner: &mut OwnedSession) {
    owner.initialize().await.unwrap();
    owner.start_thread().await.unwrap();
    owner.start_turn("host fixture".into()).await.unwrap();
}
fn stopped(owner: &OwnedSession, root: &Path) {
    match owner.cleanup() {
        Cleanup::Observed(report) => {
            assert!(report.scope.leader_exited);
            assert_eq!(
                report.scope.whole_tree_stopped,
                cfg!(any(target_os = "linux", windows))
            );
        }
        other => panic!("cleanup evidence missing: {other:?}"),
    }
    std::thread::sleep(Duration::from_millis(50));
    let before = std::fs::metadata(root.join("pulse")).map_or(0, |m| m.len());
    std::thread::sleep(Duration::from_millis(80));
    assert_eq!(
        std::fs::metadata(root.join("pulse")).map_or(0, |m| m.len()),
        before
    );
}
#[tokio::test]
async fn native_progress_attachment_owned_pipes_redaction_and_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("原生 工作区");
    std::fs::create_dir(&root).unwrap();
    let mut owner = spawn(&root, "normal");
    running(&mut owner).await;
    let mut a = Attachment::for_owned(run(), Filter::default(), &owner, 0).unwrap();
    let mut ended = false;
    for now in 1..=16 {
        if matches!(
            a.next_owned(&run(), &mut owner, now).await.unwrap(),
            Update::TurnEnded
        ) {
            ended = true;
            break;
        }
    }
    assert!(ended);
    assert_eq!(
        a.summary(&run()).unwrap().as_deref(),
        Some("finished — ran commands, 1 failed, 1 attempt unresolved")
    );
    assert_eq!(a.diagnostics().gated_tool_calls, 1);
    let emission = a.claim(&run(), 20).unwrap().unwrap();
    assert_eq!(
        emission.text(),
        "⏳ finished — ran commands, 1 failed, 1 attempt unresolved"
    );
    assert!(!emission.text().contains("SECRET"));
    assert!(
        matches!(owner.protocol_outcome(),Some(Outcome::Completed{text}) if text=="SECRET model answer")
    );
    assert!(Attachment::for_owned(run(), Filter::default(), &owner, 20).is_err());
    stopped(&owner, &root);
}
#[tokio::test]
async fn native_progress_attachment_cancellation_owned_process_remains_inspectable() {
    let root = tempfile::tempdir().unwrap();
    let mut owner = spawn(root.path(), "silent");
    running(&mut owner).await;
    let id = owner.id();
    let mut a = Attachment::for_owned(run(), Filter::default(), &owner, 0).unwrap();
    let _emission = a.claim(&run(), 0).unwrap().unwrap();
    assert!(
        tokio::time::timeout(
            Duration::from_millis(30),
            a.next_owned(&run(), &mut owner, 1)
        )
        .await
        .is_err()
    );
    assert_eq!(owner.id(), id);
    assert_eq!(a.retirement(), Some(Retirement::Cancelled));
    assert_eq!(a.pending_state(), Some(PendingState::Uncertain));
    assert!(matches!(a.claim(&run(), 2), Err(Error::Retired)));
    assert!(matches!(
        owner.protocol_outcome(),
        Some(Outcome::Unknown { .. })
    ));
    owner.stop();
    stopped(&owner, root.path());
}
