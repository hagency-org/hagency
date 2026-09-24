use super::*;

#[tokio::test]
async fn native_owned_turn_long_lifetime() {
    let f = Fixture::new();
    let began = std::time::Instant::now();
    let mut operation = Operation::start(
        f.domain.clone(),
        f.cap.clone(),
        f.host("quiet-long-turn", "work", false),
        Limits {
            operation_ms: 45_000,
            response_ms: 1500,
        },
    )
    .unwrap();
    let report = operation.wait().await.unwrap();
    assert!(
        began.elapsed() >= Duration::from_secs(31),
        "early exit after {:?}: protocol={:?}, failure={:?}, startup={:?}, observation={:?}, cleanup={:?}",
        began.elapsed(),
        report.protocol,
        report.failure,
        report.startup_error(),
        report.runtime_observation(),
        // Names the guardian's own stop cause: a quiet turn that ends early was
        // stopped by something, and the transport error alone never says what.
        report.cleanup
    );
    assert_eq!(
        report.protocol,
        Protocol::Completed,
        "{:?} {:?}",
        report.failure,
        report.runtime_observation()
    );
    assert_eq!(report.text.as_deref(), Some("离线管道验证完成"));
    if cfg!(any(target_os = "linux", target_os = "macos", windows)) {
        assert_eq!(report.failure, None);
        assert_eq!(report.settlement, Settlement::Completed);
        assert_eq!(f.state(), "completed");
        assert_eq!(f.count("SELECT COUNT(*) FROM resource_leases"), 0);
    } else {
        assert_eq!(report.failure, Some(Failure::CleanupUnknown));
        f.quarantined();
    }
    drop(report);
    drop(operation);
    f.domain.shutdown().await.unwrap();
}

#[tokio::test]
async fn native_owned_turn_long_cancel() {
    let f = Fixture::new();
    let mut operation = Operation::start(
        f.domain.clone(),
        f.cap.clone(),
        f.host("quiet-open", "work", false),
        Limits {
            operation_ms: hagency_core::tasks::MAX_OWNED_OPERATION_MS,
            response_ms: 1500,
        },
    )
    .unwrap();
    let until = tokio::time::Instant::now() + Duration::from_secs(4);
    while !f.work.join("owned-dispatch.quiet").is_file() {
        assert!(tokio::time::Instant::now() < until);
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let began = std::time::Instant::now();
    operation.cancel();
    let report = operation.wait().await.unwrap();
    assert!(
        began.elapsed() < Duration::from_secs(8),
        "cancellation must not wait for the long operation budget"
    );
    assert_eq!(report.failure, Some(Failure::Cancelled));
    f.quarantined();
    assert!(!report.retains_process_custody());
    drop(report);
    drop(operation);
    f.domain.shutdown().await.unwrap();
}

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
    if !cfg!(any(target_os = "linux", target_os = "macos", windows)) {
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
