use super::*;

#[tokio::test]
async fn native_owned_turn_quiet_notification_wait() {
    let f = Fixture::new();
    let at = std::time::Instant::now();
    let mut operation = f.operation("quiet-turn");
    let report = operation.wait().await.unwrap();
    assert!(f.work.join("owned-dispatch.quiet").is_file());
    assert_eq!(report.protocol, Protocol::Completed);
    assert_eq!(report.text.as_deref(), Some("离线管道验证完成"));
    assert!(at.elapsed() >= Duration::from_millis(2200));
    assert_ne!(report.failure, Some(Failure::Protocol));
    if cfg!(target_os = "macos") {
        // Preserve this generic fixture's existing unqualified descendant scope.
        assert_eq!(report.failure, Some(Failure::CleanupUnknown));
        f.quarantined();
    } else {
        assert_eq!(report.failure, None);
        assert_eq!(report.settlement, Settlement::Completed);
        assert_eq!(f.state(), "completed");
    }
    drop(report);
    f.domain.shutdown().await.unwrap();
}

#[tokio::test]
async fn native_owned_turn_quiet_limits() {
    for cancel in [true, false] {
        let f = Fixture::new();
        let started = std::time::Instant::now();
        let mut operation = Operation::start(
            f.domain.clone(),
            f.cap.clone(),
            f.host("quiet-open", "work", false),
            Limits {
                operation_ms: 5000,
                response_ms: 1500,
            },
        )
        .unwrap();
        let until = tokio::time::Instant::now() + Duration::from_secs(4);
        while !f.work.join("owned-dispatch.quiet").is_file() {
            assert!(
                tokio::time::Instant::now() < until,
                "original turn did not acknowledge start"
            );
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        if cancel {
            operation.cancel();
        }
        let report = operation.wait().await.unwrap();
        assert_eq!(report.protocol, Protocol::Unknown);
        assert!(report.text.is_none());
        if cancel {
            assert_eq!(report.failure, Some(Failure::Cancelled));
        } else {
            // The original operation or same-lifetime transport may win the
            // deadline race; either must be negative, never a completed turn.
            assert!(matches!(
                report.failure,
                Some(Failure::Deadline | Failure::Protocol)
            ));
            assert!(
                started.elapsed() >= Duration::from_secs(5),
                "quiet turn must reach the original operation bound, not the RPC interval"
            );
        }
        f.quarantined();
        drop(report);
        let marker = f.work.join("owned-dispatch.pulse");
        let bytes = fs::metadata(&marker).map_or(0, |m| m.len());
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(fs::metadata(&marker).map_or(0, |m| m.len()), bytes);
        f.domain.shutdown().await.unwrap();
    }
}
