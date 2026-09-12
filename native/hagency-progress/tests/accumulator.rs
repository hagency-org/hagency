use hagency_progress::*;
use serde_json::{Value, json};
fn run(name: &str) -> RunId {
    RunId::new(name.into()).unwrap()
}
fn hook(tool: &str) -> Value {
    json!({"hook_event_name":"PostToolUse","tool_name":tool,"input":"SECRET /private/.env","error":"SECRET credential","title":"SECRET customer"})
}
fn acp(id: &str, kind: &str) -> Value {
    json!({"sessionUpdate":"tool_call","kind":kind,"toolCallId":id,"title":"SECRET /private/file","rawInput":{"token":"SECRET"}})
}
fn failed(id: &str) -> Value {
    json!({"sessionUpdate":"tool_call_update","status":"failed","toolCallId":id,"error":"SECRET"})
}

#[test]
fn native_progress_scope_receipts_order_retirement() {
    let id = run("host-a");
    let other = run("host-b");
    let mut state = Accumulator::new(id.clone(), Filter::default());
    assert!(matches!(
        state.observe_hook(&other, 1, 10, &hook("Read")),
        Err(Error::Identity)
    ));
    assert!(matches!(
        state.observe_hook(&id, 2, 10, &hook("Read")),
        Err(Error::Order)
    ));
    assert_eq!(
        state.observe_hook(&id, 1, 10, &hook("Read")).unwrap(),
        Observation::Recorded
    );
    assert_eq!(
        state.observe_hook(&id, 1, 0, &hook("Read")).unwrap(),
        Observation::Duplicate
    );
    let mut changed = hook("Read");
    changed["title"] = json!("different");
    assert!(matches!(
        state.observe_hook(&id, 1, 10, &changed),
        Err(Error::Conflict)
    ));
    assert!(matches!(
        state.observe_acp(&id, 1, 10, &hook("Read")),
        Err(Error::Conflict)
    ));
    assert!(matches!(
        state.observe_hook(&id, 2, 9, &hook("Write")),
        Err(Error::Clock)
    ));
    assert_eq!(state.summary().unwrap().as_deref(), Some("read"));
    let emission = state.claim(&id, 10).unwrap().unwrap();
    assert!(!emission.text().contains("SECRET"));
    state.retire(&id, 11).unwrap();
    assert_eq!(state.pending_state(), Some(PendingState::Uncertain));
    let recovery = state.pending_id().unwrap().clone();
    assert!(&recovery == emission.id());
    assert!(matches!(state.claim(&id, 12), Err(Error::State)));
    assert!(matches!(state.summary(), Err(Error::State)));
    assert!(matches!(
        state.observe_hook(&id, 1, 12, &hook("Read")),
        Err(Error::State)
    ));
    state
        .settle(&id, &recovery, 12, AttemptOutcome::ObservedNotAccepted)
        .unwrap();
    assert!(state.pending_id().is_none());
    assert!(matches!(state.claim(&id, 13), Err(Error::State)));
    let mut fresh = Accumulator::new(other.clone(), Filter::default());
    assert!(matches!(
        fresh.settle(&other, emission.id(), 13, AttemptOutcome::ObservedAccepted),
        Err(Error::Identity)
    ));
    assert!(fresh.summary().unwrap().is_none());
    fresh.observe_hook(&other, 1, 13, &hook("Write")).unwrap();
    assert_eq!(fresh.summary().unwrap().as_deref(), Some("wrote"));
}

#[test]
fn native_progress_scope_bounds_and_atomic_rejection() {
    let id = run("bounded");
    let mut state = Accumulator::new(id.clone(), Filter::default());
    assert!(matches!(
        state.observe_hook(&id, 1, 0, &json!({"large":"x".repeat(16384)})),
        Err(Error::Capacity)
    ));
    for i in 1..=MAX_EVENTS {
        state
            .observe_hook(&id, i as u64, i as u64, &hook("UnknownPrivateTool"))
            .unwrap();
    }
    let before = state.summary().unwrap();
    assert_eq!(before.as_deref(), Some("worked ×1024"));
    assert!(matches!(
        state.observe_hook(&id, MAX_EVENTS as u64 + 1, 2048, &hook("Read")),
        Err(Error::Capacity)
    ));
    assert_eq!(state.summary().unwrap(), before);
    assert!(matches!(
        Filter::parse(&json!({"events":vec!["step";129]}), None),
        Err(Error::Capacity)
    ));
    assert!(matches!(
        Filter::parse(&json!({"events":["x".repeat(257)]}), None),
        Err(Error::Capacity)
    ));
    let mut deep = json!(null);
    for _ in 0..10 {
        deep = json!({"next":deep});
    }
    assert!(matches!(Filter::parse(&deep, None), Err(Error::Capacity)));
    let mut calls = Accumulator::new(id.clone(), Filter::default());
    for i in 1..=MAX_CALLS {
        calls
            .observe_acp(&id, i as u64, i as u64, &acp(&format!("c{i}"), "read"))
            .unwrap();
    }
    assert!(matches!(
        calls.observe_acp(&id, 257, 257, &acp("extra", "read")),
        Err(Error::Capacity)
    ));
    calls.finish(&id, 257, 257, None).unwrap();
    assert_eq!(
        calls.summary().unwrap().as_deref(),
        Some("finished — 256 attempts unresolved")
    );
    assert_eq!(
        calls.claim(&id, 257).unwrap().unwrap().text(),
        "⏳ finished — 256 attempts unresolved"
    );
    let mut counts = Counts::default();
    counts.add(Verb::Read, MAX_COUNTER).unwrap();
    assert!(matches!(counts.add(Verb::Wrote, 1), Err(Error::Capacity)));
    assert_eq!(counts.entries(), &[(Verb::Read, MAX_COUNTER)]);
}

#[test]
fn native_progress_coalescing_snapshots_failure_and_uncertainty() {
    let id = run("window");
    let mut state = Accumulator::new(id.clone(), Filter::default());
    state.observe_hook(&id, 1, 100, &hook("Read")).unwrap();
    let first = state.claim(&id, 100).unwrap().unwrap();
    assert_eq!(first.text(), "⏳ read");
    state.observe_hook(&id, 2, 101, &hook("Write")).unwrap();
    assert!(state.claim(&id, 100000).unwrap().is_none());
    state
        .settle(&id, first.id(), 100000, AttemptOutcome::Unknown)
        .unwrap();
    assert_eq!(state.pending_state(), Some(PendingState::Uncertain));
    assert!(state.claim(&id, 200000).unwrap().is_none());
    state
        .settle(&id, first.id(), 200000, AttemptOutcome::ObservedAccepted)
        .unwrap();
    assert_eq!(
        state
            .settle(&id, first.id(), 0, AttemptOutcome::ObservedAccepted)
            .unwrap(),
        Observation::Duplicate
    );
    assert!(matches!(
        state.settle(&id, first.id(), 200000, AttemptOutcome::ObservedNotAccepted),
        Err(Error::Conflict)
    ));
    let second = state.claim(&id, 200000).unwrap().unwrap();
    assert_eq!(second.text(), "⏳ wrote");
    state
        .settle(
            &id,
            second.id(),
            200000,
            AttemptOutcome::ObservedNotAccepted,
        )
        .unwrap();
    assert!(state.claim(&id, 259999).unwrap().is_none());
    assert!(matches!(state.claim(&id, 259998), Err(Error::Clock)));
    let retry = state.claim(&id, 260000).unwrap().unwrap();
    assert_eq!(retry.text(), "⏳ wrote");
    assert!(retry.id() != second.id());
    state
        .settle(&id, retry.id(), 260000, AttemptOutcome::ObservedAccepted)
        .unwrap();
    assert!(state.claim(&id, 320000).unwrap().is_none());
    assert_eq!(state.summary().unwrap().as_deref(), Some("read, wrote"));
}

#[test]
fn native_progress_summary_acp_failures_exclusion_and_lifetime() {
    let id = run("acp");
    let filter = Filter::parse(&json!({"tools":{"exclude":["Bash"]}}), None).unwrap();
    let mut state = Accumulator::new(id.clone(), filter);
    state
        .observe_acp(&id, 1, 1, &acp("hidden", "execute"))
        .unwrap();
    state.observe_acp(&id, 2, 2, &failed("hidden")).unwrap();
    assert!(state.summary().unwrap().is_none());
    assert!(matches!(
        state.observe_acp(&id, 3, 3, &failed("missing")),
        Err(Error::Order)
    ));
    let call = acp("read", "read");
    state.observe_acp(&id, 3, 3, &call).unwrap();
    state.observe_acp(&id, 4, 4, &call).unwrap();
    let notice = state.claim(&id, 4).unwrap().unwrap();
    assert_eq!(notice.text(), "⏳ 1 attempt pending");
    state.observe_acp(&id, 5, 5, &failed("read")).unwrap();
    state.observe_acp(&id, 6, 6, &failed("read")).unwrap();
    state
        .settle(&id, notice.id(), 7, AttemptOutcome::ObservedAccepted)
        .unwrap();
    let fail = state.claim(&id, 60004).unwrap().unwrap();
    assert_eq!(fail.text(), "⏳ 1 failed attempt");
    state
        .settle(&id, fail.id(), 60004, AttemptOutcome::ObservedAccepted)
        .unwrap();
    state.finish(&id, 7, 60005, None).unwrap();
    let finished = state.claim(&id, 60005).unwrap().unwrap();
    assert_eq!(
        finished.text(),
        "⏳ finished, but nothing succeeded — 1 failed attempt"
    );
    state
        .settle(&id, finished.id(), 60005, AttemptOutcome::ObservedAccepted)
        .unwrap();
    assert!(state.claim(&id, 120005).unwrap().is_none());
    assert!(matches!(
        state.observe_hook(&id, 8, 120005, &hook("Write")),
        Err(Error::State)
    ));
}

#[test]
fn native_progress_summary_host_delivery_and_fixed_text() {
    for delivery in [
        None,
        Some(AnswerDelivery::new(
            0,
            DeliveryProof::FinalReplyJournalInspection,
        )),
        Some(AnswerDelivery::new(
            1,
            DeliveryProof::MatrixTimelineInspection,
        )),
    ] {
        let id = run("delivery");
        let mut state = Accumulator::new(id.clone(), Filter::default());
        state.observe_hook(&id, 1, 1, &hook("constructor")).unwrap();
        let initial = state.claim(&id, 1).unwrap().unwrap();
        assert_eq!(initial.text(), "⏳ worked");
        state
            .settle(&id, initial.id(), 1, AttemptOutcome::ObservedAccepted)
            .unwrap();
        state.finish(&id, 2, 2, delivery).unwrap();
        let final_line = state.claim(&id, 2).unwrap().unwrap();
        assert_eq!(
            final_line.text(),
            if delivery.is_some_and(|d| d.count() == 0) {
                "⏳ finished — worked, but sent nothing"
            } else {
                "⏳ finished — worked"
            }
        );
        assert_eq!(state.answer_delivery(), delivery);
        for private in [
            "SECRET",
            "constructor",
            "token",
            "/private",
            "host",
            "Matrix",
            "task",
        ] {
            assert!(!final_line.text().contains(private));
        }
        assert!(final_line.text().len() < 512);
        if delivery.is_some() {
            assert!(matches!(
                state.finish(&id, 2, 2, None),
                Err(Error::Conflict)
            ));
        }
    }
}

#[test]
fn native_progress_scope_attempt_capacity_and_terminal_conflict() {
    let id = run("attempt-limit");
    let mut state = Accumulator::new(id.clone(), Filter::default());
    state.observe_hook(&id, 1, 0, &hook("Read")).unwrap();
    for i in 0..MAX_ATTEMPTS {
        let now = u64::from(i) * 60000;
        let emission = state.claim(&id, now).unwrap().unwrap();
        state
            .settle(&id, emission.id(), now, AttemptOutcome::ObservedNotAccepted)
            .unwrap();
    }
    assert!(matches!(
        state.claim(&id, u64::from(MAX_ATTEMPTS) * 60000),
        Err(Error::Capacity)
    ));
    assert_eq!(state.summary().unwrap().as_deref(), Some("read"));
    let mut calls = Accumulator::new(id.clone(), Filter::default());
    calls.observe_acp(&id, 1, 0, &acp("one", "read")).unwrap();
    calls
        .observe_acp(
            &id,
            2,
            1,
            &json!({"sessionUpdate":"tool_call_update","status":"completed","toolCallId":"one"}),
        )
        .unwrap();
    assert!(matches!(
        calls.observe_acp(&id, 3, 2, &failed("one")),
        Err(Error::Conflict)
    ));
    calls.finish(&id, 3, 2, None).unwrap();
    assert_eq!(calls.summary().unwrap().as_deref(), Some("finished — read"));
}

#[test]
fn native_progress_summary_initial_status_and_unresolved_outcomes() {
    // Initial ACP notices carry the current status, not necessarily a start.
    // Missing, future and malformed status do not establish success or failure.
    for completed in [false, true] {
        let id = run("mixed-outcomes");
        let mut state = Accumulator::new(id.clone(), Filter::default());
        let mut inputs = vec![];
        if completed {
            let mut initial = acp("complete", "read");
            initial["status"] = json!("completed");
            inputs.extend([initial.clone(), initial]);
        }
        for (call, status) in [
            ("pending", json!("pending")),
            ("running", json!("in_progress")),
            ("future", json!("future")),
            ("malformed", json!(["failed"])),
            ("failed", json!("failed")),
        ] {
            let mut initial = acp(call, "read");
            initial["status"] = status;
            inputs.push(initial);
        }
        inputs.push(acp("missing", "read"));
        inputs.push(failed("failed")); // Initial failure and later update count once.
        for (i, payload) in inputs.iter().enumerate() {
            state
                .observe_acp(&id, i as u64 + 1, i as u64, payload)
                .unwrap();
        }
        let next = inputs.len() as u64 + 1;
        let before = state.summary().unwrap();
        assert!(matches!(
            state.observe_acp(&id, next, next, &json!({"sessionUpdate":"tool_call_update","toolCallId":"failed","status":"completed"})),
            Err(Error::Conflict)
        ));
        assert_eq!(state.summary().unwrap(), before);
        state.finish(&id, next, next, None).unwrap();
        let expected = if completed {
            "finished — read, 1 failed, 5 attempts unresolved"
        } else {
            "finished — 1 failed attempt, 5 attempts unresolved"
        };
        assert_eq!(state.summary().unwrap().as_deref(), Some(expected));
        let final_line = state.claim(&id, next).unwrap().unwrap();
        assert_eq!(final_line.text(), format!("⏳ {expected}"));
        assert!(!final_line.text().contains("nothing succeeded"));
        assert!(!final_line.text().contains("SECRET"));
        assert!(!final_line.text().contains("sent nothing"));
    }

    // Excluded terminal and pending initial states are all equally silent.
    let id = run("excluded-initial");
    let mut state = Accumulator::new(
        id.clone(),
        Filter::parse(&json!({"tools":{"exclude":["Read"]}}), None).unwrap(),
    );
    for (i, status) in ["failed", "completed", "pending"].iter().enumerate() {
        let mut payload = acp(status, "read");
        payload["status"] = json!(status);
        state
            .observe_acp(&id, i as u64 + 1, i as u64, &payload)
            .unwrap();
    }
    state.finish(&id, 4, 4, None).unwrap();
    assert_eq!(state.claim(&id, 4).unwrap().unwrap().text(), "⏳ finished");
}

#[test]
fn native_progress_coalescing_pending_notice_does_not_ack_completion() {
    let id = run("pending-completion");
    let mut state = Accumulator::new(id.clone(), Filter::default());
    state.observe_acp(&id, 1, 0, &acp("read", "read")).unwrap();
    let pending = state.claim(&id, 0).unwrap().unwrap();
    assert_eq!(pending.text(), "⏳ 1 attempt pending");
    let completion =
        json!({"sessionUpdate":"tool_call_update","toolCallId":"read","status":"completed"});
    state.observe_acp(&id, 2, 1, &completion).unwrap();
    state
        .settle(&id, pending.id(), 2, AttemptOutcome::ObservedAccepted)
        .unwrap();
    let completed = state.claim(&id, 60000).unwrap().unwrap();
    assert_eq!(completed.text(), "⏳ read");
    state.observe_acp(&id, 3, 60001, &completion).unwrap();
    state
        .settle(&id, completed.id(), 60002, AttemptOutcome::ObservedAccepted)
        .unwrap();
    assert!(state.claim(&id, 120000).unwrap().is_none());
    state.finish(&id, 4, 120001, None).unwrap();
    assert_eq!(
        state.claim(&id, 120001).unwrap().unwrap().text(),
        "⏳ finished — read"
    );
}
