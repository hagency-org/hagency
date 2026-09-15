use super::*;

/// Reopen the domain after an acknowledged shutdown. The writer drops its
/// repository before it acknowledges, so the ownership lock is normally free;
/// hosted macOS once reported `Locked` on the immediate reopen, so allow the
/// release a bounded moment rather than failing on the first attempt.
async fn reopen(state: &std::path::Path) -> DomainRepository {
    let until = tokio::time::Instant::now() + Duration::from_secs(2);
    loop {
        match DomainRepository::open(state) {
            Err(hagency_store::Error::Locked) if tokio::time::Instant::now() < until => {
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
            other => return other.unwrap(),
        }
    }
}

#[tokio::test]
async fn native_owned_usage_restart_and_capacity_keeps_execution_separate() {
    let f = Fixture::new();
    let mut operation = f.operation("usage-gate");
    let until = tokio::time::Instant::now() + Duration::from_secs(4);
    while !f.work.join("owned-dispatch.usage-ready").exists() {
        assert!(
            tokio::time::Instant::now() < until,
            "native usage gate never entered"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert_eq!(f.state(), "started");
    let mut sql = f.sql();
    let source: String = sql
        .query_row("SELECT id FROM usage_sources", [], |r| r.get(0))
        .unwrap();
    let tx = sql.transaction().unwrap();
    // Artificial historical receipts establish capacity, never proof of usage.
    for n in 0..hagency_store::MAX_SOURCE_USAGE_RECEIPTS {
        tx.execute("INSERT INTO usage_receipts(source_id,call_id,digest,observation,response) VALUES(?1,?2,'fixture','{}','{}')",rusqlite::params![source,format!("historical_{n}")]).unwrap();
    }
    tx.commit().unwrap();
    fs::write(f.work.join("owned-dispatch.usage-release"), b"release").unwrap();
    let mut report = operation.wait().await.unwrap();
    assert_eq!(report.protocol, Protocol::Completed);
    assert_eq!(report.canonical_status, Some(TaskState::InProgress));
    assert_eq!(
        report.failure,
        if cfg!(target_os = "macos") {
            Some(Failure::CleanupUnknown)
        } else {
            None
        }
    );
    let status = report.usage_status();
    assert_eq!(
        status.failure,
        Some(hagency_execution::UsageFailure::Storage)
    );
    assert!(status.pending && status.closed && status.incomplete);
    assert_eq!((status.observed, status.acknowledged), (1, 0));
    assert_eq!(
        report.retry_usage().await,
        Err(hagency_execution::UsageFailure::Storage)
    );
    assert_eq!(f.count("SELECT COUNT(*) FROM final_replies"), 0);
    assert_eq!(f.count("SELECT observations FROM usage_sources"), 0);
    assert_eq!(
        f.count("SELECT COUNT(*) FROM usage_receipts"),
        hagency_store::MAX_SOURCE_USAGE_RECEIPTS
    );
    assert_eq!(report.protocol, Protocol::Completed);
    drop(report);
    f.domain.shutdown().await.unwrap();
    let db = reopen(&f.root.path().join("state")).await;
    let source = db.restore_usage_source(&source).unwrap();
    let history = db.usage_source(&source).unwrap();
    assert_eq!(history.observations, 0);
    assert_eq!(history.latest_counts, None);
    assert!(history.latest_incomplete);
}

#[tokio::test]
async fn native_owned_usage_normalization_refusal_keeps_completion_separate() {
    let f = Fixture::new();
    let mut operation = f.operation("usage-overflow");
    let report = operation.wait().await.unwrap();
    assert_eq!(report.protocol, Protocol::Completed);
    assert_eq!(report.canonical_status, Some(TaskState::InProgress));
    assert_eq!(
        report.failure,
        if cfg!(target_os = "macos") {
            Some(Failure::CleanupUnknown)
        } else {
            None
        }
    );
    let usage = report.usage_status();
    assert_eq!(
        usage.failure,
        Some(hagency_execution::UsageFailure::Normalization)
    );
    assert!(usage.rejected && usage.closed && usage.incomplete);
    assert!(!usage.pending);
    assert_eq!((usage.observed, usage.acknowledged), (1, 0));
    assert_eq!(f.count("SELECT COUNT(*) FROM usage_receipts"), 0);
    assert_eq!(f.count("SELECT COUNT(*) FROM final_replies"), 0);
    drop(report);
    f.domain.shutdown().await.unwrap();
}

#[tokio::test]
async fn native_owned_usage_real_capture() {
    let f = Fixture::new();
    let mut operation = f.operation("usage");
    let report = operation.wait().await.unwrap();
    assert_eq!(report.protocol, Protocol::Completed);
    assert_eq!(report.canonical_status, Some(TaskState::InProgress));
    let usage = report.usage_status();
    assert!(usage.bound && usage.attached && usage.closed && usage.incomplete);
    assert_eq!(usage.observed, 4);
    assert_eq!(usage.acknowledged, 4);
    assert!(!usage.pending);
    assert_eq!(usage.failure, None);
    let summary = f.domain.usage_summary(f.engagement.clone()).await.unwrap();
    assert_eq!(summary.sources, 1);
    let water = summary.known_high_water_lower_bound.unwrap();
    assert_eq!(
        (
            water.input,
            water.output,
            water.cache_read,
            water.cache_write
        ),
        (20, 20, 45, 65)
    );
    assert_eq!(summary.regression_observations, 1);
    assert_eq!(summary.latest_incomplete_sources, 1);
    assert_eq!(summary.historically_incomplete_sources, 1);
    assert_eq!(summary.latest_counts.unwrap().input, Some(10));
    assert_eq!(f.count("SELECT COUNT(*) FROM usage_receipts"), 4);
    assert_eq!(f.count("SELECT COUNT(*) FROM usage_sources WHERE json_extract(attribution,'$.task_id')='task' AND dispatch_id='dispatch'"), 1);
    assert_eq!(
        f.count("SELECT COUNT(*) FROM usage_receipts WHERE call_id LIKE 'runtime_v1_%'"),
        4
    );
    assert_eq!(
        f.count(
            "SELECT COUNT(*) FROM canonical_tasks WHERE json_extract(config,'$.status')='done'"
        ),
        0
    );
    assert_eq!(f.count("SELECT COUNT(*) FROM final_replies"), 0);
    // Only fresh thread/start exists on the actual owned pipe path.
    let requests = fs::read_to_string(f.work.join("owned-dispatch.requests")).unwrap();
    assert!(requests.contains("thread/start"));
    assert!(!requests.contains("thread/resume"));
    let source: String = f
        .sql()
        .query_row("SELECT id FROM usage_sources", [], |r| r.get(0))
        .unwrap();
    drop(report);
    f.domain.shutdown().await.unwrap();
    let db = reopen(&f.root.path().join("state")).await;
    let source = db.restore_usage_source(&source).unwrap();
    let history = db.usage_source(&source).unwrap();
    assert_eq!(history.observations, 4);
    assert_eq!(history.high_water.input, Some(20));
    assert!(history.latest_incomplete && history.latest_regressed);
    assert!(
        history
            .latest_observation
            .unwrap()
            .get("runtime_evidence")
            .is_some()
    );
}

#[tokio::test]
async fn native_owned_usage_missing_and_binding_refusal() {
    let f = Fixture::new();
    let mut operation = f.operation("normal");
    let report = operation.wait().await.unwrap();
    assert_eq!(report.protocol, Protocol::Completed);
    assert_eq!(report.usage_status().observed, 0);
    assert!(report.usage_status().incomplete);
    assert_eq!(f.count("SELECT COUNT(*) FROM usage_sources"), 1);
    assert_eq!(f.count("SELECT COUNT(*) FROM usage_receipts"), 0);
    let summary = f.domain.usage_summary(f.engagement.clone()).await.unwrap();
    let latest = summary.latest_counts.unwrap();
    assert_eq!(
        (
            latest.input,
            latest.output,
            latest.cache_read,
            latest.cache_write
        ),
        (None, None, None, None)
    );
    assert_eq!(summary.latest_incomplete_sources, 1);
    drop(report);
    f.domain.shutdown().await.unwrap();

    let f = Fixture::new();
    // Synthetic historical identities exercise the per-engagement capacity
    // boundary only; none is claimed as execution/provenance evidence.
    let mut sql = f.sql();
    let tx = sql.transaction().unwrap();
    for n in 0..hagency_store::MAX_ENGAGEMENT_USAGE_SOURCES {
        tx.execute("INSERT INTO usage_sources(id,dispatch_id,fence,engagement_id,identity_digest,framework,attribution,high_water,latest_counts) VALUES(?1,'dispatch',?2,?3,'fixture','codex','{}','{\"input\":null,\"output\":null,\"cacheRead\":null,\"cacheWrite\":null}','null')", rusqlite::params![format!("historical_{n}"), n + 1000, f.engagement]).unwrap();
    }
    tx.commit().unwrap();
    let mut operation = f.operation("usage");
    let report = operation.wait().await.unwrap();
    assert_eq!(report.failure, Some(Failure::UsageBinding));
    assert_eq!(report.protocol, Protocol::NotStarted);
    assert!(!f.marker().exists());
    assert!(!report.usage_status().bound);
    assert_eq!(f.count("SELECT COUNT(*) FROM usage_receipts"), 0);
    f.quarantined();
    drop(report);
    f.domain.shutdown().await.unwrap();
}
