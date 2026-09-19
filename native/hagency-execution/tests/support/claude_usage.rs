use super::*;
use hagency_platform::Launch;
use hagency_runtime::{
    claude::session::{Limits, Phase},
    owned::{Cleanup, OwnedClaudeSession},
};
use std::{collections::BTreeMap, path::Path};

async fn owned(root: &Path, name: &str, mode: &str) -> OwnedClaudeSession {
    let binary = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join(format!(
            "hagency-execution-probe{}",
            std::env::consts::EXE_SUFFIX
        ));
    assert!(
        binary.is_file(),
        "build the original execution probe before the owned selector"
    );
    let mut environment = BTreeMap::from([("PATH".into(), "".into())]);
    if let Some(system) = std::env::var_os("SystemRoot") {
        environment.insert("SystemRoot".into(), system);
    }
    let launch = Launch {
        executable: binary.clone(),
        arguments: vec!["fake-claude".into(), mode.into(), root.join(name).into()],
        directory: root.into(),
        environment,
        require_crash_containment: false,
    };
    let mut runner = OwnedClaudeSession::spawn(
        &binary,
        &launch,
        Limits {
            write_timeout_ms: 1000,
            event_wait_ms: 3000,
            lifetime_ms: 30_000,
        },
    )
    .unwrap();
    runner.initialize().await.unwrap();
    runner.prompt("offline usage attribution").await.unwrap();
    runner.next_message().await.unwrap();
    runner
}

#[tokio::test]
async fn native_owned_claude_usage_capture() {
    for committed in [false, true] {
        let mut f = Fixture::family("claude").await;
        let mut runner = owned(f.root.path(), "original", "usage").await;
        f.run.attach(&runner);
        assert!(f.run.status().attached);
        assert_eq!(f.run.sequence, 1);
        while runner.phase() == Phase::Running {
            runner.next_message().await.unwrap();
            let retained = f.run.observe(runner.last_observation().unwrap());
            if retained && runner.phase() == Phase::Running {
                f.run.record_pending().await.unwrap();
            }
        }
        assert_eq!(runner.cleanup(), Cleanup::Pending);
        assert!(f.run.status().closed && f.run.status().pending);
        assert_eq!(
            (f.run.status().observed, f.run.status().acknowledged),
            (3, 2)
        );
        let (entered, ready) = tokio::sync::oneshot::channel();
        let domain = f.domain.clone();
        let mut attempt = Box::pin(f.run.record_with(
            move |source, call, observation| async move {
                if committed {
                    let receipt = domain
                        .record_usage_observation(source, call, observation)
                        .await?;
                    let _ = entered.send(());
                    std::future::pending::<()>().await;
                    Ok(receipt)
                } else {
                    let _ = entered.send(());
                    std::future::pending::<()>().await;
                    domain
                        .record_usage_observation(source, call, observation)
                        .await
                }
            },
        ));
        tokio::select! {result=&mut attempt=>panic!("gate unexpectedly returned: {result:?}"),result=ready=>result.unwrap()};
        drop(attempt);
        assert!(f.run.status().pending);
        assert_eq!(f.observations().await, 2 + u64::from(committed));
        let source = runner.last_observation().unwrap().source().clone();
        runner.stop();
        assert!(source.is_retired());
        f.run.record_pending().await.unwrap();
        assert_eq!(f.observations().await, 3);
        assert!(!f.run.status().pending);
        assert!(f.run.status().closed && f.run.status().incomplete);
        assert_eq!(f.run.status().failure, None);
        let view = f.domain.usage_source(f.run.source.clone()).await.unwrap();
        assert_eq!(view.high_water.input, Some(101));
        assert_eq!(view.high_water.output, Some(202));
        assert_eq!(view.high_water.cache_read, Some(303));
        assert_eq!(view.high_water.cache_write, Some(404));
        assert!(view.latest_incomplete && view.historical_incomplete);
        let sql = f.sql();
        let state: String = sql
            .query_row(
                "SELECT json_extract(config,'$.status') FROM canonical_tasks WHERE id='task'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(state, "in_progress");
        let rows: Vec<String> = sql
            .prepare("SELECT observation FROM usage_receipts ORDER BY call_id")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(rows.len(), 3);
        for row in rows {
            assert!(!row.contains("private-model"));
            assert!(row.contains("claude_runtime_evidence"));
        }
        let source_id = f.run.source.id().to_owned();
        f.domain.shutdown().await.unwrap();
        let db = DomainRepository::open(&f.root.path().join("state")).unwrap();
        let source = db.restore_usage_source(&source_id).unwrap();
        let history = db.usage_source(&source).unwrap();
        assert_eq!(history.observations, 3);
        assert_eq!(history.high_water.input, Some(101));
    }
    for mode in [
        "foreign",
        "retired",
        "skipped",
        "replay",
        "reattach",
        "family",
        "pending",
        "invalidated",
    ] {
        let mut f = Fixture::family(if mode == "family" { "codex" } else { "claude" }).await;
        let mut runner = owned(
            f.root.path(),
            "original",
            if mode == "invalidated" {
                "usage-conflict"
            } else {
                "usage"
            },
        )
        .await;
        f.run.attach(&runner);
        let expected = match mode {
            "family" => UsageFailure::Source,
            "reattach" => {
                f.run.attach(&runner);
                UsageFailure::Source
            }
            "foreign" => {
                let mut other = owned(f.root.path(), "foreign", "usage").await;
                other.next_message().await.unwrap();
                assert!(!f.run.observe(other.last_observation().unwrap()));
                other.stop();
                UsageFailure::Source
            }
            _ => {
                runner.next_message().await.unwrap();
                let event = runner.last_observation().unwrap().clone();
                match mode {
                    "retired" => {
                        runner.stop();
                        assert!(!f.run.observe(&event));
                        UsageFailure::Retired
                    }
                    "skipped" => {
                        runner.next_message().await.unwrap();
                        assert!(!f.run.observe(runner.last_observation().unwrap()));
                        UsageFailure::Sequence
                    }
                    "replay" => {
                        assert!(f.run.observe(&event));
                        f.run.record_pending().await.unwrap();
                        assert!(!f.run.observe(&event));
                        UsageFailure::Sequence
                    }
                    "pending" | "invalidated" => {
                        assert!(f.run.observe(&event));
                        if mode == "invalidated" {
                            f.run.record_pending().await.unwrap();
                        }
                        runner.next_message().await.unwrap();
                        assert!(!f.run.observe(runner.last_observation().unwrap()));
                        if mode == "pending" {
                            UsageFailure::Storage
                        } else {
                            UsageFailure::Invalidated
                        }
                    }
                    _ => unreachable!(),
                }
            }
        };
        assert_eq!(f.run.status().failure, Some(expected), "{mode}");
        assert!(f.run.status().closed);
        runner.stop();
        f.domain.shutdown().await.unwrap();
    }
}
