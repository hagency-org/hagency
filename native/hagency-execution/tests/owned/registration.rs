use super::*;
use hagency_execution::{WorkspaceError, WorkspaceRegistration};

async fn take(operation: &mut Operation) -> WorkspaceRegistration {
    let until = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(value) = operation.take_workspace_registration() {
            return value;
        }
        assert!(
            !operation.is_finished(),
            "operation ended before actual registration"
        );
        assert!(
            tokio::time::Instant::now() < until,
            "registration was never published"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}
fn operation(f: &Fixture) -> Operation {
    Operation::start_requiring_workspace(
        f.domain.clone(),
        f.cap.clone(),
        f.host("usage-gate", "work", false),
        limits(),
    )
    .unwrap()
}
#[tokio::test]
async fn native_workspace_registration_gate() {
    let f = Fixture::new();
    let mut operation = operation(&f);
    let (binding, ack) = take(&mut operation).await.into_parts();
    assert_eq!(f.state(), "started");
    assert!(!f.marker().exists());
    assert!(operation.take_workspace_binding().is_none());
    assert!(operation.take_workspace_registration().is_none());
    binding.validate_current(&f.cap).await.unwrap();
    // Queueing ACK is not launch truth; wait for the actual native child marker.
    ack.registered().unwrap();
    f.entered().await;
    operation.cancel();
    let report = operation.wait().await.unwrap();
    assert_eq!(report.failure, Some(Failure::Cancelled));
    assert_eq!(
        binding.validate_current(&f.cap).await,
        Err(WorkspaceError::Retired)
    );
    drop(report);
    f.domain.shutdown().await.unwrap();
}
#[tokio::test]
async fn native_workspace_registration_gate_drop_and_late_ack() {
    for cancel in [false, true] {
        let f = Fixture::new();
        let mut operation = operation(&f);
        let (binding, ack) = take(&mut operation).await.into_parts();
        if cancel {
            operation.cancel();
            let report = operation.wait().await.unwrap();
            assert!(ack.registered().is_err());
            assert_eq!(report.failure, Some(Failure::Cancelled));
            drop(report);
        } else {
            drop(ack);
            let report = operation.wait().await.unwrap();
            assert_eq!(report.failure, Some(Failure::Admission));
            drop(report);
        }
        assert!(!f.marker().exists());
        assert_eq!(
            binding.validate_current(&f.cap).await,
            Err(WorkspaceError::Retired)
        );
        assert_eq!(
            f.count("SELECT COUNT(*) FROM runner_attempts WHERE outcome='outcome_unknown'"),
            1
        );
        f.domain.shutdown().await.unwrap();
    }
}
#[tokio::test]
async fn native_workspace_registration_gate_revoked_before_ack() {
    let f = Fixture::new();
    let mut operation = operation(&f);
    let (_binding, ack) = take(&mut operation).await.into_parts();
    f.domain
        .revoke("revoke".into(), f.engagement.clone())
        .await
        .unwrap();
    let _ = ack.registered();
    let report = operation.wait().await.unwrap();
    assert!(!f.marker().exists());
    assert!(report.failure.is_some());
    drop(report);
    f.domain.shutdown().await.unwrap();
}
