use super::*;
use hagency_runtime::codex::session::{Observation, ObservationKind, UsageEvidence};

fn metrics() -> Value {
    json!({
        "threadId":"thread-one", "turnId":"turn-one",
        "tokenUsage": {
            "total": {"totalTokens": 150, "inputTokens": 100, "cachedInputTokens": 40,
                "cacheWriteInputTokens": 10, "outputTokens": 50, "reasoningOutputTokens": 20},
            "last": {"totalTokens": 15, "inputTokens": 10, "cachedInputTokens": 4,
                "cacheWriteInputTokens": 1, "outputTokens": 5, "reasoningOutputTokens": 2},
            "modelContextWindow": 200_000
        }
    })
}
async fn observe(s: &mut Session, p: &mut Peer, params: Value) -> Observation {
    let (result, ()) = tokio::join!(
        s.next_observed_update(),
        write(p, note("thread/tokenUsage/updated", params))
    );
    let (update, observation) = result.unwrap();
    assert!(matches!(update, Update::Progress));
    assert_eq!(s.phase(), Phase::Running);
    assert!(s.outcome().is_none());
    observation
}
fn usage(observation: &Observation) -> &UsageEvidence {
    let ObservationKind::Usage(usage) = observation.kind() else {
        panic!("scoped usage observation missing")
    };
    usage
}

#[tokio::test]
async fn native_codex_usage_observation_counters_are_separate_and_exact() {
    let (mut s, mut p) = running().await;
    let source = s.observation_source().unwrap();
    let observation = observe(&mut s, &mut p, metrics()).await;
    assert!(observation.source() == &source);
    assert_eq!(observation.sequence(), 1);
    let evidence = usage(&observation);
    for (counters, values) in [
        (evidence.total(), [150, 100, 40, 10, 50, 20]),
        (evidence.last(), [15, 10, 4, 1, 5, 2]),
    ] {
        assert_eq!(
            [
                counters.total_tokens(),
                counters.input_tokens(),
                counters.cached_input_tokens(),
                counters.cache_write_input_tokens(),
                counters.output_tokens(),
                counters.reasoning_output_tokens()
            ],
            values.map(Some)
        );
    }
    assert_eq!(evidence.model_context_window(), Some(200_000));
    let d = evidence.diagnostics();
    assert!(!d.has_missing_fields() && !d.has_invalid_fields() && !d.has_unsupported_fields());

    // Zero and the largest exact integer remain received values, not absence.
    let mut fields = metrics();
    fields["tokenUsage"]["total"]["totalTokens"] = json!(0);
    fields["tokenUsage"]["last"]["inputTokens"] = json!(9_007_199_254_740_991u64);
    let observation = observe(&mut s, &mut p, fields).await;
    assert_eq!(usage(&observation).total().total_tokens(), Some(0));
    assert_eq!(
        usage(&observation).last().input_tokens(),
        Some(9_007_199_254_740_991)
    );
    // This layer preserves contradictory arithmetic for the policy adapter.
    assert!(!usage(&observation).diagnostics().has_invalid_fields());
}

#[tokio::test]
async fn native_codex_usage_observation_uncertainty_preserves_independent_fields() {
    let (mut s, mut p) = running().await;
    for absent in [true, false] {
        let mut fields = metrics();
        if absent {
            fields["tokenUsage"]["total"]
                .as_object_mut()
                .unwrap()
                .remove("cacheWriteInputTokens");
        } else {
            fields["tokenUsage"]["total"]["cacheWriteInputTokens"] = Value::Null;
        }
        let event = observe(&mut s, &mut p, fields).await;
        let e = usage(&event);
        assert_eq!(e.total().cache_write_input_tokens(), None);
        assert_eq!(e.total().input_tokens(), Some(100));
        assert!(e.diagnostics().has_missing_fields());
        assert!(!e.diagnostics().has_invalid_fields());
    }
    for invalid in [
        json!(-1),
        json!(1.0),
        json!(1.5),
        json!(true),
        json!("10"),
        json!(9_007_199_254_740_992u64),
        json!({"secret":"not retained"}),
        json!([]),
    ] {
        for path in ["total", "last"] {
            let mut fields = metrics();
            fields["tokenUsage"][path]["inputTokens"] = invalid.clone();
            fields["tokenUsage"]["modelContextWindow"] = invalid.clone();
            let event = observe(&mut s, &mut p, fields).await;
            let e = usage(&event);
            let part = if path == "total" { e.total() } else { e.last() };
            assert_eq!(part.input_tokens(), None);
            assert!(part.output_tokens().is_some());
            assert_eq!(e.model_context_window(), None);
            assert!(e.diagnostics().has_invalid_fields());
        }
    }
    for path in ["/tokenUsage", "/tokenUsage/total", "/tokenUsage/last"] {
        for value in [Value::Null, json!(false), json!("secret"), json!([])] {
            let mut fields = metrics();
            *fields.pointer_mut(path).unwrap() = value.clone();
            let event = observe(&mut s, &mut p, fields).await;
            let d = usage(&event).diagnostics();
            assert!(d.has_missing_fields());
            assert_eq!(d.has_invalid_fields(), !value.is_null());
        }
    }
    for path in ["", "/tokenUsage", "/tokenUsage/total", "/tokenUsage/last"] {
        let mut fields = metrics();
        fields
            .pointer_mut(path)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert(
                "futurePrivateField".into(),
                json!({"path":"PRIVATE","text":"SECRET"}),
            );
        let event = observe(&mut s, &mut p, fields).await;
        assert_eq!(usage(&event).total().total_tokens(), Some(150));
        assert!(usage(&event).diagnostics().has_unsupported_fields());
    }
    let event = observe(
        &mut s,
        &mut p,
        json!({"threadId":"thread-one","turnId":"turn-one"}),
    )
    .await;
    assert_eq!(usage(&event).total().total_tokens(), None);
    assert!(usage(&event).diagnostics().has_missing_fields());
}

#[tokio::test]
async fn native_codex_usage_observation_scope_sequence_and_retirement() {
    let (mut s, mut p) = running().await;
    let (mut other, mut peer) = running().await;
    let first = observe(&mut s, &mut p, metrics()).await;
    let foreign = observe(&mut other, &mut peer, metrics()).await;
    assert!(first.source() != foreign.source());
    assert!(first == first.clone());
    update(&mut s, &mut p, note("warning", json!({})))
        .await
        .unwrap();
    let third = observe(&mut s, &mut p, metrics()).await;
    assert_eq!(third.sequence(), 3);
    assert!(first.source() == third.source());
    s.close();
    assert!(first.source().is_retired());
    assert!(!foreign.source().is_retired());
    assert!(s.next_observed_update().await.is_err());
    for key in ["threadId", "turnId"] {
        let (mut s, mut p) = running().await;
        let source = s.observation_source().unwrap();
        let mut fields = metrics();
        fields[key] = json!("other");
        fields["tokenUsage"] = json!("malformed metrics do not hide scope");
        let (result, ()) = tokio::join!(
            s.next_observed_update(),
            write(&mut p, note("thread/tokenUsage/updated", fields))
        );
        assert!(matches!(result, Err(Error::Scope)));
        assert!(source.is_retired());
        unknown(&s, Error::Scope);
    }
}
