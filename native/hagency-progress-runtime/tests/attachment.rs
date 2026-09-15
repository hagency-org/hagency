mod common;
use common::*;
use hagency_progress::{AttemptOutcome, Filter, Observation as Recorded, PendingState, RunId};
use hagency_progress_runtime::{Attachment, Error, MAX_RECEIPTS, Retirement};
use hagency_runtime::codex::session::Outcome;
use serde_json::{Value, json};
use std::time::Duration;
fn run() -> RunId {
    RunId::new("host-immutable-run".into()).unwrap()
}
#[tokio::test]
async fn native_progress_attachment_usage_is_quiet() {
    let (mut s, mut p) = running().await;
    let mut a = Attachment::for_driver(run(), Filter::default(), &s, 0).unwrap();
    let before = a.summary(&run()).unwrap();
    let metrics = note(
        "thread/tokenUsage/updated",
        json!({
            "threadId":"thread-one", "turnId":"turn-one", "tokenUsage":{
                "total":{"inputTokens":1000,"outputTokens":50},
                "last":{"inputTokens":100,"outputTokens":5}, "modelContextWindow":null
            }
        }),
    );
    take(&mut a, &mut s, &mut p, metrics.clone(), 1).await;
    assert_eq!(a.summary(&run()).unwrap(), before);
    assert!(s.outcome().is_none());
    take(
        &mut a,
        &mut s,
        &mut p,
        tool("a", "commandExecution", "inProgress", false, Value::Null),
        2,
    )
    .await;
    take(&mut a, &mut s, &mut p, metrics, 3).await;
    assert_eq!(
        a.summary(&run()).unwrap().as_deref(),
        Some("1 attempt pending")
    );
    take(
        &mut a,
        &mut s,
        &mut p,
        tool("a", "commandExecution", "completed", true, json!(0)),
        4,
    )
    .await;
    take(&mut a, &mut s, &mut p, end("completed"), 5).await;
    assert_eq!(
        a.summary(&run()).unwrap().as_deref(),
        Some("finished — ran commands")
    );
}
async fn take(a: &mut Attachment, s: &mut Session, p: &mut Peer, value: Value, now: u64) {
    write(p, value).await;
    a.next_driver(&run(), s, now).await.unwrap();
}
#[tokio::test]
async fn native_progress_attachment_source_exact_replay_late_gap_and_foreign() {
    let (mut s, mut p) = running().await;
    let mut a = Attachment::for_driver(run(), Filter::default(), &s, 10).unwrap();
    write(
        &mut p,
        tool("a", "commandExecution", "inProgress", false, Value::Null),
    )
    .await;
    let (_, e) = s.next_observed_update().await.unwrap();
    assert_eq!(a.observe(&run(), &e, 11).unwrap(), Recorded::Recorded);
    assert_eq!(
        a.observe(&run(), &e.clone(), 0).unwrap(),
        Recorded::Duplicate
    );
    assert!(Attachment::for_driver(run(), Filter::default(), &s, 11).is_err());
    let (mut other, mut peer) = running().await;
    write(
        &mut peer,
        tool("a", "commandExecution", "inProgress", false, Value::Null),
    )
    .await;
    let (_, foreign) = other.next_observed_update().await.unwrap();
    assert_eq!(a.observe(&run(), &foreign, 12), Err(Error::Source));
    assert!(matches!(a.claim(&run(), 13), Err(Error::Retired)));
    let (mut s, mut p) = running().await;
    let mut a = Attachment::for_driver(run(), Filter::default(), &s, 0).unwrap();
    write(&mut p, item_event("message", "", false)).await;
    s.next_update().await.unwrap();
    write(&mut p, item_event("message", "SECRET", true)).await;
    assert!(matches!(
        a.next_driver(&run(), &mut s, 1).await,
        Err(Error::Observation)
    ));
    assert_eq!(a.retirement(), Some(Retirement::InvalidObservation));
    let (s, _) = fixture(false);
    assert!(Attachment::for_driver(run(), Filter::default(), &s, 0).is_err());
}
#[tokio::test]
async fn native_progress_attachment_source_bounds_close_and_clock() {
    let (mut s, mut p) = running().await;
    let mut a = Attachment::for_driver(run(), Filter::default(), &s, 0).unwrap();
    for i in 0..MAX_RECEIPTS {
        take(
            &mut a,
            &mut s,
            &mut p,
            note("warning", json!({"secret":"SECRET"})),
            i as u64,
        )
        .await;
    }
    write(&mut p, note("warning", json!({}))).await;
    assert!(matches!(
        a.next_driver(&run(), &mut s, 1024).await,
        Err(Error::Capacity)
    ));
    assert_eq!(a.retirement(), Some(Retirement::Capacity));
    let (mut s, mut p) = running().await;
    let mut a = Attachment::for_driver(run(), Filter::default(), &s, 10).unwrap();
    write(
        &mut p,
        tool("a", "fileChange", "inProgress", false, Value::Null),
    )
    .await;
    let (_, e) = s.next_observed_update().await.unwrap();
    assert_eq!(a.observe(&run(), &e, 9), Err(Error::Observation));
    assert!(matches!(a.claim(&run(), 11), Err(Error::Retired)));
    let (mut s, mut p) = running().await;
    let mut a = Attachment::for_driver(run(), Filter::default(), &s, 0).unwrap();
    write(&mut p, item_event("answer", "", false)).await;
    let (_, e) = s.next_observed_update().await.unwrap();
    s.close();
    assert_eq!(a.observe(&run(), &e, 1), Err(Error::Retired));
    assert_eq!(a.retirement(), Some(Retirement::Source));
}
#[tokio::test]
async fn native_progress_attachment_source_drop_retirement_and_failed_turn() {
    let (mut s, mut p) = running().await;
    let mut a = Attachment::for_driver(run(), Filter::default(), &s, 0).unwrap();
    let claim = a.claim(&run(), 0).unwrap().unwrap();
    write(
        &mut p,
        tool("a", "commandExecution", "inProgress", false, Value::Null),
    )
    .await;
    let (_, event) = s.next_observed_update().await.unwrap();
    drop(s);
    assert_eq!(a.observe(&run(), &event, 1), Err(Error::Retired));
    assert_eq!(a.pending_state(), Some(PendingState::Uncertain));
    a.settle(&run(), claim.id(), 2, AttemptOutcome::ObservedAccepted)
        .unwrap();
    assert!(matches!(a.claim(&run(), 3), Err(Error::Retired)));
    for status in ["failed", "interrupted"] {
        let (mut s, mut p) = running().await;
        let mut a = Attachment::for_driver(run(), Filter::default(), &s, 0).unwrap();
        let mut event = end(status);
        if status == "failed" {
            event["params"]["turn"]["error"] = json!({"message":"SECRET"});
        }
        write(&mut p, event).await;
        let _result = a.next_driver(&run(), &mut s, 1).await;
        assert!(a.retirement().is_some());
        assert!(matches!(a.claim(&run(), 2), Err(Error::Retired)));
        assert!(matches!(
            s.outcome(),
            Some(Outcome::Failed | Outcome::Interrupted)
        ));
    }
    let (s, _p) = running().await;
    let mut a = Attachment::for_driver(run(), Filter::default(), &s, 0).unwrap();
    assert!(matches!(
        a.claim(&RunId::new("replacement-host-run".into()).unwrap(), 1),
        Err(Error::Source)
    ));
    assert!(matches!(a.claim(&run(), 2), Err(Error::Retired)));
}

#[tokio::test]
async fn native_progress_attachment_evidence_exact_status_exit_and_filters() {
    let cases = [
        (
            "commandExecution",
            "completed",
            json!(0),
            "finished — ran commands",
        ),
        (
            "commandExecution",
            "completed",
            json!(1),
            "finished — 1 attempt unresolved",
        ),
        (
            "commandExecution",
            "completed",
            Value::Null,
            "finished — 1 attempt unresolved",
        ),
        (
            "commandExecution",
            "completed",
            json!(0.0),
            "finished — 1 attempt unresolved",
        ),
        (
            "commandExecution",
            "failed",
            json!(1),
            "finished, but nothing succeeded — 1 failed attempt",
        ),
        (
            "commandExecution",
            "declined",
            Value::Null,
            "finished, but nothing succeeded — 1 failed attempt",
        ),
        ("fileChange", "completed", Value::Null, "finished — edited"),
        (
            "fileChange",
            "failed",
            Value::Null,
            "finished, but nothing succeeded — 1 failed attempt",
        ),
        (
            "fileChange",
            "unknown",
            Value::Null,
            "finished — 1 attempt unresolved",
        ),
    ];
    for (kind, status, exit, expected) in cases {
        let (mut s, mut p) = running().await;
        let mut a = Attachment::for_driver(run(), Filter::default(), &s, 0).unwrap();
        take(
            &mut a,
            &mut s,
            &mut p,
            tool("a", kind, "inProgress", false, Value::Null),
            1,
        )
        .await;
        assert_eq!(
            a.summary(&run()).unwrap().as_deref(),
            Some("1 attempt pending")
        );
        take(
            &mut a,
            &mut s,
            &mut p,
            tool("a", kind, status, true, exit),
            2,
        )
        .await;
        take(&mut a, &mut s, &mut p, end("completed"), 3).await;
        assert_eq!(
            a.summary(&run()).unwrap().as_deref(),
            Some(expected),
            "{kind} {status}"
        );
        assert!(
            !a.claim(&run(), 4)
                .unwrap()
                .unwrap()
                .text()
                .contains("SECRET")
        );
        assert!(matches!(s.outcome(), Some(Outcome::Completed { .. })));
    }
    let (mut s, mut p) = running().await;
    let filter = Filter::parse(
        &json!({"tools":{"include":["Bash"]},"perGroup":{"scope":{"tools":{"exclude":["Bash"]}}}}),
        Some("scope"),
    )
    .unwrap();
    let mut a = Attachment::for_driver(run(), filter, &s, 0).unwrap();
    for (i, (id, kind, status, exit)) in [
        ("a", "commandExecution", "failed", json!(1)),
        ("b", "commandExecution", "unknown", Value::Null),
        ("c", "fileChange", "completed", Value::Null),
    ]
    .into_iter()
    .enumerate()
    {
        take(
            &mut a,
            &mut s,
            &mut p,
            tool(id, kind, "inProgress", false, Value::Null),
            i as u64 * 2 + 1,
        )
        .await;
        take(
            &mut a,
            &mut s,
            &mut p,
            tool(id, kind, status, true, exit),
            i as u64 * 2 + 2,
        )
        .await;
    }
    take(&mut a, &mut s, &mut p, end("completed"), 7).await;
    assert_eq!(
        a.summary(&run()).unwrap().as_deref(),
        Some("finished — edited")
    );
}
#[tokio::test]
async fn native_progress_attachment_evidence_contradictory_final_snapshot_and_gates() {
    let (mut s, mut p) = running().await;
    let mut a = Attachment::for_driver(run(), Filter::default(), &s, 0).unwrap();
    take(
        &mut a,
        &mut s,
        &mut p,
        tool("a", "commandExecution", "completed", false, json!(0)),
        1,
    )
    .await;
    assert_eq!(
        a.summary(&run()).unwrap().as_deref(),
        Some("1 attempt pending")
    );
    take(
        &mut a,
        &mut s,
        &mut p,
        tool("a", "commandExecution", "completed", true, json!(0)),
        2,
    )
    .await;
    let mut terminal = end("completed");
    terminal["params"]["turn"]["items"] =
        json!([{"id":"a","type":"commandExecution","status":"failed","exitCode":1}]);
    write(&mut p, terminal).await;
    assert!(matches!(
        a.next_driver(&run(), &mut s, 3).await,
        Err(Error::Observation)
    ));
    assert!(
        matches!(s.outcome(), Some(Outcome::Completed { .. })),
        "optional projection never rewrites protocol outcome"
    );
    assert!(matches!(a.claim(&run(), 4), Err(Error::Retired)));
    let (mut s, mut p) = running().await;
    let mut a = Attachment::for_driver(run(), Filter::default(), &s, 0).unwrap();
    for (i, kind) in ["mcpToolCall", "dynamicToolCall", "webSearch", "imageView"]
        .into_iter()
        .enumerate()
    {
        let id = format!("gated{i}");
        take(
            &mut a,
            &mut s,
            &mut p,
            tool(&id, kind, "inProgress", false, Value::Null),
            i as u64 * 2 + 1,
        )
        .await;
        take(
            &mut a,
            &mut s,
            &mut p,
            tool(&id, kind, "completed", true, json!(0)),
            i as u64 * 2 + 2,
        )
        .await;
    }
    take(&mut a, &mut s, &mut p, end("completed"), 9).await;
    assert_eq!(a.diagnostics().gated_tool_calls, 4);
    assert_eq!(a.summary(&run()).unwrap().as_deref(), Some("finished"));
}
#[tokio::test]
async fn native_progress_attachment_cancellation_preserves_custody_and_recovery() {
    let (mut s, _p) = running().await;
    let mut a = Attachment::for_driver(run(), Filter::default(), &s, 0).unwrap();
    let emission = a.claim(&run(), 0).unwrap().unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(15), a.next_driver(&run(), &mut s, 1))
            .await
            .is_err()
    );
    assert_eq!(a.retirement(), Some(Retirement::Cancelled));
    assert_eq!(a.pending_state(), Some(PendingState::Uncertain));
    assert!(matches!(s.outcome(), Some(Outcome::Unknown { .. })));
    assert!(matches!(a.claim(&run(), 2), Err(Error::Retired)));
    a.settle(
        &run(),
        emission.id(),
        2,
        AttemptOutcome::ObservedNotAccepted,
    )
    .unwrap();
    assert!(a.pending_id().is_none());
    assert!(matches!(a.claim(&run(), 3), Err(Error::Retired)));
    s.close();
}
#[tokio::test]
async fn native_progress_attachment_source_wrong_thread_error_and_duplicate_lifecycle() {
    for mode in ["scope", "duplicate", "eof"] {
        let (mut s, mut p) = running().await;
        let mut a = Attachment::for_driver(run(), Filter::default(), &s, 0).unwrap();
        take(
            &mut a,
            &mut s,
            &mut p,
            tool("a", "fileChange", "inProgress", false, Value::Null),
            1,
        )
        .await;
        if mode == "eof" {
            drop(p);
        } else {
            let mut event = tool(
                "a",
                "fileChange",
                "completed",
                mode != "duplicate",
                Value::Null,
            );
            if mode == "scope" {
                event["params"]["threadId"] = json!("foreign-thread");
            }
            write(&mut p, event).await;
        }
        assert!(matches!(
            a.next_driver(&run(), &mut s, 2).await,
            Err(Error::Runtime(_))
        ));
        assert!(a.retirement().is_some());
        assert!(matches!(a.claim(&run(), 3), Err(Error::Retired)));
    }
}

#[tokio::test]
async fn native_progress_attachment_source_quiet_clock_fences_claim_and_settlement() {
    let (mut s, mut p) = running().await;
    let mut a = Attachment::for_driver(run(), Filter::default(), &s, 0).unwrap();
    take(
        &mut a,
        &mut s,
        &mut p,
        note("warning", json!({"secret":"SECRET"})),
        100,
    )
    .await;
    assert!(matches!(
        a.claim(&run(), 99),
        Err(Error::Policy(hagency_progress::Error::Clock))
    ));
    let attempt = a.claim(&run(), 100).unwrap().unwrap();
    take(
        &mut a,
        &mut s,
        &mut p,
        tool("gated", "mcpToolCall", "inProgress", false, Value::Null),
        200,
    )
    .await;
    assert_eq!(
        a.settle(&run(), attempt.id(), 199, AttemptOutcome::ObservedAccepted),
        Err(Error::Policy(hagency_progress::Error::Clock))
    );
    assert_eq!(a.pending_state(), Some(PendingState::Attempted));
    a.settle(&run(), attempt.id(), 200, AttemptOutcome::ObservedAccepted)
        .unwrap();
    assert_eq!(
        a.settle(&run(), attempt.id(), 0, AttemptOutcome::ObservedAccepted)
            .unwrap(),
        Recorded::Duplicate
    );
    write(
        &mut p,
        tool("gated", "mcpToolCall", "completed", true, Value::Null),
    )
    .await;
    assert!(matches!(
        a.next_driver(&run(), &mut s, 199).await,
        Err(Error::Observation)
    ));
    assert_eq!(a.retirement(), Some(Retirement::InvalidObservation));
}
