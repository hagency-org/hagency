//! MA-M8a (ADR-144), third selector: a task handed across the shared room
//! settles on its own engagement and the usage ledger attributes the spend
//! to that engagement only. The two-engagement `PairFixture` provides both
//! engagements; the owned-dispatch + usage-bind sequence is the store's own
//! proven harness shape (`hagency-execution/tests/support/usage.rs`).
#[path = "../../hagency-matrix/tests/common/mod.rs"]
mod common;

use common::pair::{PairFixture, SHARED_ROOM};
use hagency_core::tasks::{DispatchInput, ResourceLease, SessionBinding, TaskMutation, TaskState};
use hagency_metering::Framework;
use hagency_metering::observation::UsageObservation;
use serde_json::json;
use std::time::{SystemTime, UNIX_EPOCH};

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
/// The proven Codex usage snapshot from the domain worker's own tests: an
/// honest parsed observation, never a hand-built count.
const SNAPSHOT: &str = r#"{"payload":{"info":{"total_token_usage":{"input_tokens":10,"output_tokens":2,"cached_input_tokens":3,"reasoning_output_tokens":0,"total_tokens":12}}}}"#;

#[tokio::test]
async fn native_two_agent_task_handoff_observes_usage_on_the_right_engagement() {
    let pair = PairFixture::new_pair();
    let store = &pair.store;
    // B carries REAL prior activity of its own (review F3): its own session,
    // task, dispatch and usage row exist before A's handoff runs, so the
    // cross-engagement negative asserts "B's rows are unchanged", not "B
    // never had anything".
    store
        .register_session(SessionBinding {
            id: "b-session".into(),
            engagement_id: pair.b.transport.engagement_id.clone(),
            room_id: SHARED_ROOM.into(),
            thread_root: None,
        })
        .await
        .unwrap();
    store.register_workspace("work-b".into()).await.unwrap();
    store
        .create_canonical_task(
            "b-task".into(),
            "b-session".into(),
            "B's own prior task".into(),
            now(),
        )
        .await
        .unwrap();
    store
        .enqueue_dispatch(DispatchInput {
            id: "b-dispatch".into(),
            session_id: "b-session".into(),
            task_id: Some("b-task".into()),
            resources: vec![ResourceLease {
                id: "work-b".into(),
                exclusive: true,
            }],
            payload: json!({"instruction":"B's own prior spend"}),
        })
        .await
        .unwrap();
    let b_cap = store
        .claim_dispatch("owned_host_b".into(), now(), 60_000, 60_000, 1)
        .await
        .unwrap()
        .unwrap();
    let b_scope = store.owned_dispatch_scope(b_cap.clone()).await.unwrap();
    let b_started = store
        .start_owned_dispatch(b_cap.clone(), b_scope.fingerprint().to_owned())
        .await
        .unwrap();
    let b_source = store
        .bind_usage_source(b_cap.clone(), b_started.clone())
        .await
        .unwrap();
    let b_prior = UsageObservation::parse(Framework::Codex, SNAPSHOT).unwrap();
    store
        .record_usage_observation(b_source, "b_prior".into(), b_prior)
        .await
        .unwrap();
    let b_before = store
        .usage_summary(pair.b.transport.engagement_id.clone())
        .await
        .unwrap();
    // The handoff lands on A's engagement: its session is bound to the ONE
    // shared delivery room, and the task is created for that session.
    store
        .register_session(SessionBinding {
            id: "handoff".into(),
            engagement_id: pair.a.transport.engagement_id.clone(),
            room_id: SHARED_ROOM.into(),
            thread_root: None,
        })
        .await
        .unwrap();
    store.register_workspace("work".into()).await.unwrap();
    store
        .create_canonical_task(
            "task".into(),
            "handoff".into(),
            "Handoff task 中文".into(),
            now(),
        )
        .await
        .unwrap();
    store
        .enqueue_dispatch(DispatchInput {
            id: "dispatch".into(),
            session_id: "handoff".into(),
            task_id: Some("task".into()),
            resources: vec![ResourceLease {
                id: "work".into(),
                exclusive: true,
            }],
            payload: json!({"instruction":"handed across the shared room"}),
        })
        .await
        .unwrap();
    // The owned-dispatch run: claim, scope, start, then bind the usage
    // source while the dispatch is Started — the exact sequence the usage
    // harness pins.
    let cap = store
        .claim_dispatch("owned_host".into(), now(), 60_000, 60_000, 1)
        .await
        .unwrap()
        .unwrap();
    let scope = store.owned_dispatch_scope(cap.clone()).await.unwrap();
    let started = store
        .start_owned_dispatch(cap.clone(), scope.fingerprint().to_owned())
        .await
        .unwrap();
    let source = store
        .bind_usage_source(cap.clone(), started.clone())
        .await
        .unwrap();
    let observation = UsageObservation::parse(Framework::Codex, SNAPSHOT).unwrap();
    store
        .record_usage_observation(source, "handoff_usage".into(), observation)
        .await
        .unwrap();
    // Settle: the task transitions Done and the dispatch completes on A's
    // own engagement.
    store
        .mutate_task(
            cap.clone(),
            "task".into(),
            "done".into(),
            TaskMutation::Transition {
                status: TaskState::Done,
                waiting_reason: None,
                waiting_until: None,
            },
            now(),
        )
        .await
        .unwrap();
    store
        .complete_dispatch(cap, json!({"observed":"handoff completed"}), now())
        .await
        .unwrap();
    // The dispatch settled, on the session the shared room handed it to.
    let connection =
        rusqlite::Connection::open(pair.root.path().join("domain/domain.sqlite3")).unwrap();
    let settled: (String, String) = connection
        .query_row(
            "SELECT d.state,s.engagement_id FROM runner_dispatches d \
             JOIN runner_sessions s ON s.id=d.session_id WHERE d.id='dispatch'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(settled.0, "completed");
    assert_eq!(settled.1, pair.a.transport.engagement_id);
    // The spend is attributed to A's engagement and to no other row.
    let attributed: Vec<String> = connection
        .prepare("SELECT engagement_id FROM usage_sources WHERE dispatch_id='dispatch'")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(attributed, vec![pair.a.transport.engagement_id.clone()]);
    drop(connection);
    // The ledger's own rollup: one source with the parsed counts on A.
    let summary_a = store
        .usage_summary(pair.a.transport.engagement_id.clone())
        .await
        .unwrap();
    assert_eq!(summary_a.sources, 1);
    let counts = summary_a.latest_counts.expect("parsed counts recorded");
    assert_eq!(counts.input, Some(10));
    assert_eq!(counts.output, Some(2));
    assert_eq!(counts.cache_read, Some(3));
    // B's prior rows are UNCHANGED by A's handoff (review F3): the same one
    // source, the same counts it recorded before A's dispatch ran — real
    // prior traffic, not an engagement that never existed.
    let summary_b = store
        .usage_summary(pair.b.transport.engagement_id.clone())
        .await
        .unwrap();
    assert_eq!(summary_b.sources, b_before.sources);
    assert_eq!(summary_b.latest_counts, b_before.latest_counts);
    assert_eq!(summary_b.sources, 1, "B carries its own prior usage source");
    let b_counts = summary_b.latest_counts.expect("B's prior counts stand");
    assert_eq!(b_counts.input, Some(10));
    assert_eq!(b_counts.output, Some(2));
    pair.store.shutdown().await.unwrap();
}
