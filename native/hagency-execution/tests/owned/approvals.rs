use super::{approval_fixture::*, *};
use hagency_core::approvals::ApprovalChoice;

#[tokio::test]
async fn native_owned_approval_long_budget() {
    let f = Fixture::configured(true);
    let long = Limits {
        operation_ms: hagency_core::tasks::MAX_OWNED_OPERATION_MS,
        response_ms: 1500,
    };
    let policy = hagency_execution::ApprovalHost::new(4, 2, 60_000, 2000).unwrap();
    let host = f
        .host("owned-approval-mcp", "work", false)
        .with_approvals(policy)
        .unwrap();
    let mut op = Operation::start(f.domain.clone(), f.cap.clone(), host, long).unwrap();
    let mut notices = op.take_approval_requests().unwrap();
    let request = notice(&f, &mut op, &mut notices).await;
    assert!(
        request.owner_expires_at > now() + 30_000,
        "owner window must exceed the old bound"
    );
    assert_eq!(f.state(), "parked");
    choose(&f, &request.request_id, ApprovalChoice::Once).await;
    let report = op.wait().await.unwrap();
    assert_eq!(
        report.protocol,
        Protocol::Completed,
        "{:?} {:?}",
        report.failure,
        report.runtime_observation()
    );
    assert_eq!(responses(&f).len(), 1);
    unconfirmed(&f);
    drop(report);
    drop(op);
    f.domain.shutdown().await.unwrap();
}

#[tokio::test]
async fn native_owned_mcp_approval_once() {
    for choice in [ApprovalChoice::Once, ApprovalChoice::Deny] {
        let f = Fixture::configured(true);
        let (mut op, mut notices) = operation(&f, "owned-approval-mcp", policy());
        let request = notice(&f, &mut op, &mut notices).await;
        let card = f
            .domain
            .private_approval(request.request_id.clone())
            .await
            .unwrap();
        assert!(!card.summary.reusable_scope);
        assert_eq!(card.method, "mcpServer/elicitation/request");
        assert_eq!(card.params["itemId"], "file-item");
        assert_eq!(card.params["correlatedToolCall"]["toolName"], "send_file");
        assert_eq!(
            card.params["correlatedToolCall"]["arguments"],
            card.params["nativeRequest"]["_meta"]["tool_params"]
        );
        assert!(card.params["nativeRequest"].get("itemId").is_none());
        assert_eq!(f.state(), "parked");
        assert!(responses(&f).is_empty());
        for unsupported in [ApprovalChoice::Task, ApprovalChoice::Always] {
            assert!(
                f.domain
                    .observe_owner_verdict(hagency_core::approvals::OwnerVerdictObservation {
                        request_id: request.request_id.clone(),
                        request_digest: card.digest.clone(),
                        binding_generation: card.binding_generation,
                        server_name: "example.test".into(),
                        room_id: card.room_id.clone(),
                        sender_mxid: card.owner_mxid.clone(),
                        event_id: format!("$unsupported-{unsupported:?}"),
                        encrypted: true,
                        choice: unsupported,
                    })
                    .await
                    .is_err()
            );
        }
        choose(&f, &request.request_id, choice).await;
        let report = op.wait().await.unwrap();
        assert_eq!(
            report.protocol,
            Protocol::Completed,
            "{:?} {:?}",
            report.failure,
            report.runtime_observation()
        );
        marker(&f, "approval-continued").await;
        let responses = responses(&f);
        assert_eq!(responses.len(), 1);
        assert_eq!(
            responses[0]["result"],
            json!({"action":if choice==ApprovalChoice::Once {"accept"} else {"decline"},"content":null,"_meta":null})
        );
        unconfirmed(&f);
    }
}

#[tokio::test]
async fn native_owned_approval_resume() {
    for choice in [ApprovalChoice::Once, ApprovalChoice::Deny] {
        let f = Fixture::configured(true);
        let (mut op, mut notices) = operation(&f, "owned-approval", policy());
        let request = match tokio::time::timeout(Duration::from_secs(6), notices.recv()).await {
            Ok(Some(value)) => value,
            other => {
                op.cancel();
                let report = op.wait().await.unwrap();
                panic!(
                    "missing notice {:?}; protocol {:?}; failure {:?}; runtime {:?}; state {}; entered {}",
                    other.is_ok(),
                    report.protocol,
                    report.failure,
                    report.runtime_observation(),
                    f.state(),
                    f.marker().exists()
                );
            }
        };
        assert!(request.owner_expires_at > now());
        assert_eq!(f.state(), "parked");
        assert!(
            f.domain
                .runner_command(
                    f.cap.clone(),
                    RunnerCommand::Mutate {
                        id: "task".into(),
                        call_id: "parked_test".into(),
                        operation: TaskMutation::Comment {
                            text: "parked".into()
                        }
                    }
                )
                .await
                .is_err()
        );
        assert!(responses(&f).is_empty());
        choose(&f, &request.request_id, choice).await;
        let report = op.wait().await.unwrap();
        assert_eq!(
            report.protocol,
            Protocol::Completed,
            "{:?} {:?}; {}",
            report.failure,
            report.runtime_observation(),
            cancellation_trace(&f)
        );
        marker(&f, "approval-continued").await;
        let responses = responses(&f);
        assert_eq!(responses.len(), 1);
        assert_eq!(
            responses[0]["result"]["decision"],
            if choice == ApprovalChoice::Deny {
                "decline"
            } else {
                "accept"
            }
        );
        unconfirmed(&f);
    }
    let f = Fixture::configured(true);
    let (mut op, mut notices) = operation(&f, "owned-approval-reuse", policy());
    let request = notice(&f, &mut op, &mut notices).await;
    choose(&f, &request.request_id, ApprovalChoice::Always).await;
    let report = op.wait().await.unwrap();
    assert_eq!(
        report.protocol,
        Protocol::Completed,
        "{:?}; {}",
        report.failure,
        cancellation_trace(&f)
    );
    assert!(
        notices.recv().await.is_none(),
        "saved grant must not emit a private owner card request"
    );
    assert_eq!(responses(&f).len(), 2);
    unconfirmed(&f);
}

#[tokio::test]
async fn native_owned_approval_barriers() {
    let f = Fixture::configured(true);
    let (mut op, mut notices) = operation(&f, "owned-approval-barriers", policy());
    let first = notice(&f, &mut op, &mut notices).await;
    let second = notice(&f, &mut op, &mut notices).await;
    choose(&f, &first.request_id, ApprovalChoice::Once).await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert!(responses(&f).is_empty());
    assert_eq!(f.state(), "parked");
    choose(&f, &second.request_id, ApprovalChoice::Deny).await;
    let third = notice(&f, &mut op, &mut notices).await;
    let before = responses(&f).len();
    assert!((1..=2).contains(&before));
    assert_eq!(f.state(), "parked");
    choose(&f, &third.request_id, ApprovalChoice::Once).await;
    let report = op.wait().await.unwrap();
    assert_eq!(
        report.protocol,
        Protocol::Completed,
        "{:?} {:?}; {}",
        report.failure,
        report.runtime_observation(),
        cancellation_trace(&f)
    );
    assert_eq!(responses(&f).len(), 3);
    unconfirmed(&f);
}

#[tokio::test]
async fn native_owned_approval_cancellation() {
    for mode in [
        "owned-approval-resolve",
        "owned-approval-eof",
        "owned-approval",
        "owned-approval-after",
        "caller-drop",
        "retire",
    ] {
        let f = Fixture::configured(true);
        let (mut op, mut notices) = operation(
            &f,
            if matches!(mode, "caller-drop" | "retire") {
                "owned-approval"
            } else {
                mode
            },
            policy(),
        );
        let request = notice(&f, &mut op, &mut notices).await;
        match mode {
            "owned-approval-resolve" => {
                fs::write(f.work.join("owned-dispatch.approval-release"), b"release").unwrap();
            }
            "owned-approval-eof" => {
                // The notice proves the host retained the callback; releasing
                // the probe's gate only then orders its hang-up AFTER delivery
                // instead of racing the callback line against process exit.
                fs::write(f.work.join("owned-dispatch.approval-release"), b"release").unwrap();
            }
            "owned-approval-after" => choose(&f, &request.request_id, ApprovalChoice::Once).await,
            "owned-approval" => op.cancel(),
            "caller-drop" => {
                assert!(
                    tokio::time::timeout(Duration::from_millis(20), op.wait())
                        .await
                        .is_err()
                );
            }
            "retire" => {
                f.domain
                    .revoke("retire".into(), f.engagement.clone())
                    .await
                    .unwrap();
            }
            _ => {}
        }
        let report = op.wait().await.unwrap();
        if mode == "owned-approval-after" {
            assert_eq!(
                report.protocol,
                Protocol::Completed,
                "{:?} {:?}",
                report.failure,
                report.runtime_observation()
            );
            assert_eq!(responses(&f).len(), 1);
        } else {
            assert!(report.failure.is_some());
            assert!(responses(&f).is_empty());
        }
        unconfirmed(&f);
    }
    // An owner choice that is RECORDED before the cutoff but first READ after
    // it cannot use the response reserve to create new preparation or router
    // authority. The rule is unchanged by the ADR046 amendment; what changed is
    // that the mere absence of an answer no longer ends the agent, so this arm
    // now has to construct the late allow deliberately.
    //
    // Deterministic by construction, not by timing. Holding the writer's lock
    // across the cutoff does both halves at once: the owner's verdict is queued
    // AHEAD of the host's expiry deny (which cannot be enqueued before the
    // cutoff), and the coordinator's own maintenance cannot READ the choice
    // until the hold ends — so it first sees the allow on the far side of the
    // cutoff. Without the hold the two orderings differ by a few milliseconds
    // of loop cadence, and the arm passes or fails on scheduling noise.
    //
    // The hold must stay under the store's 100 ms busy timeout
    // (`database.rs:56`), or the writes it blocks fail instead of queueing. So
    // the owner wait is long enough to aim at precisely, and only a short
    // window around the cutoff is held.
    let f = Fixture::configured(true);
    let (mut op, mut notices) = operation(
        &f,
        "owned-approval",
        hagency_execution::ApprovalHost::new(2, 1, 1500, 1500).unwrap(),
    );
    let request = notice(&f, &mut op, &mut notices).await;
    let card = f
        .domain
        .private_approval(request.request_id.clone())
        .await
        .unwrap();
    assert!(card.expires_at > request.owner_expires_at);
    let observation = hagency_core::approvals::OwnerVerdictObservation {
        request_id: request.request_id.clone(),
        request_digest: card.digest,
        binding_generation: card.binding_generation,
        server_name: "example.test".into(),
        room_id: card.room_id,
        sender_mxid: card.owner_mxid,
        event_id: "$late".into(),
        encrypted: true,
        choice: ApprovalChoice::Once,
    };
    // Take the lock just before the cutoff, enqueue the verdict behind it, and
    // release just after: 70 ms of hold spanning the bound.
    tokio::time::sleep(Duration::from_millis(
        request
            .owner_expires_at
            .saturating_sub(now())
            .saturating_sub(30),
    ))
    .await;
    let connection = f.sql();
    connection.execute_batch("BEGIN IMMEDIATE").unwrap();
    let domain = f.domain.clone();
    let verdict = tokio::spawn(async move { domain.observe_owner_verdict(observation).await });
    tokio::time::sleep(Duration::from_millis(70)).await;
    connection.execute_batch("COMMIT").unwrap();
    verdict
        .await
        .unwrap()
        .expect("the owner's allow is recorded first; the expiry deny then finds a decided row");
    assert!(now() < card.expires_at, "within original response reserve");
    let report = op.wait().await.unwrap();
    // The allow is refused where it always was, and nothing is prepared, begun
    // or written for it.
    assert_eq!(
        report.failure,
        Some(Failure::Deadline),
        "a late allow must not buy preparation with the reserve: {:?}; {}",
        report.runtime_observation(),
        cancellation_trace(&f)
    );
    assert!(responses(&f).is_empty());
    assert_eq!(f.count("SELECT COUNT(*) FROM approval_responses"), 0);
    assert_eq!(f.count("SELECT COUNT(*) FROM approval_grants"), 0);
    // Two branches refuse this identically, and which one fires depends on
    // whether the coordinator's loop entered its iteration before or after the
    // cutoff: the loop-top expiry finds a decided row and is refused with
    // `State`, or the iteration started early, skipped the expiry, and the
    // guard below the choice read refuses instead. That is defence in depth, so
    // the assertion is on the OUTCOME, which is identical either way — never on
    // which branch ran.
    //
    // The owner's recorded decision is never overwritten, and no expiry reason
    // is ever written over it.
    assert_eq!(
        f.domain
            .approval_summary(request.request_id.clone())
            .await
            .unwrap()
            .choice,
        Some(ApprovalChoice::Once)
    );
    assert_eq!(expiry_reason(&f, &request.request_id), None);
}

#[tokio::test]
async fn native_owned_approval_capacity() {
    let shared = hagency_execution::ApprovalHost::new(3, 1, 20_000, 1500).unwrap();
    let first = Fixture::configured(true);
    let (mut a, mut notices) = operation(&first, "owned-approval", shared.clone());
    notice(&first, &mut a, &mut notices).await;
    let second = Fixture::configured(true);
    let (mut b, _) = operation(&second, "owned-approval", shared.clone());
    let report = b.wait().await.unwrap();
    assert_eq!(report.failure, Some(Failure::ApprovalCapacity));
    assert_eq!(second.count("SELECT COUNT(*) FROM owner_approvals"), 0);
    assert_eq!(second.count("SELECT COUNT(*) FROM resource_leases"), 1);
    let third = Fixture::configured(true);
    let (mut c, _) = operation(&third, "normal", shared.clone());
    assert_eq!(c.wait().await.unwrap().protocol, Protocol::Completed);
    a.cancel();
    a.wait().await.unwrap();
    assert_eq!(shared.live_limit(), 3);
}

#[tokio::test]
async fn native_owned_approval_usage() {
    let f = Fixture::configured(true);
    let (mut op, mut notices) = operation(&f, "owned-approval-usage", policy());
    let request = notice(&f, &mut op, &mut notices).await;
    marker(&f, "approval-ready").await;
    let connection = f.sql();
    connection.execute_batch("BEGIN IMMEDIATE").unwrap();
    fs::write(f.work.join("owned-dispatch.approval-release"), b"release").unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    connection.execute_batch("COMMIT").unwrap();
    choose(&f, &request.request_id, ApprovalChoice::Once).await;
    let report = op.wait().await.unwrap();
    assert_eq!(
        report.protocol,
        Protocol::Completed,
        "{:?} {:?}; {}",
        report.failure,
        report.runtime_observation(),
        cancellation_trace(&f)
    );
    let usage = report.usage_status();
    assert_eq!(usage.observed, 3);
    assert_eq!(usage.acknowledged, 3);
    assert!(!usage.pending);
    assert!(!usage.rejected);
    assert_eq!(usage.failure, None);
    unconfirmed(&f);
}

/// The owner wait for the expiry scenarios. Short enough that a test can
/// outlive it, with a reserve wide enough for what the expiry actually has to
/// do inside it: the durable deny, the maintenance read, authorize, begin,
/// recheck, the frame and the acceptance write. The floor
/// `hagency/src/bootstrap/config.rs::approval_host` ships (5000) is the same
/// judgement at fleet scale.
fn expiry_policy() -> hagency_execution::ApprovalHost {
    hagency_execution::ApprovalHost::new(4, 2, 1500, 8000).unwrap()
}
/// The reason `deny_for_owner_wait_expiry` records, read straight from the
/// durable receipt. `None` when the host never recorded an expiry.
fn expiry_reason(f: &Fixture, id: &str) -> Option<String> {
    use rusqlite::OptionalExtension;
    f.sql()
        .query_row(
            "SELECT denial_reason FROM approval_verdict_receipts WHERE request_id=?1",
            [id],
            |r| r.get(0),
        )
        .optional()
        .unwrap()
        .flatten()
}

/// The live shape (2026-09-19, three times): a Codex agent calls an MCP tool
/// that is not pre-approved, the owner never answers, and the owner wait runs
/// out. The agent must survive that and be able to report that approval was
/// not granted in time.
///
/// Before the ADR046 amendment this ended the agent: the session timed out at
/// the owner bound (`owned_failure=protocol`, `session_error=transport`,
/// `transport_cause=timeout`), the worker died and the `owner_approvals` row
/// stayed `pending` forever. Now the host answers with the wire's own decline
/// and the turn continues.
#[tokio::test]
async fn native_owned_approval_owner_wait_expiry_declines_and_continues() {
    let f = Fixture::configured(true);
    let (mut op, mut notices) = operation(&f, "owned-approval-mcp", expiry_policy());
    let request = notice(&f, &mut op, &mut notices).await;
    assert_eq!(f.state(), "parked");
    // Nobody answers. That is the whole scenario.
    let report = op.wait().await.unwrap();
    assert_eq!(
        report.protocol,
        Protocol::Completed,
        "the turn must continue: {:?} {:?}; {}",
        report.failure,
        report.runtime_observation(),
        cancellation_trace(&f)
    );
    assert_eq!(report.failure, None);
    assert!(
        now() >= request.owner_expires_at,
        "the owner window really elapsed"
    );

    // The wire: exactly one frame, and it is the MCP family's own decline with
    // no content and no metadata — the same bytes the owner's own Deny sends.
    marker(&f, "approval-continued").await;
    let responses = responses(&f);
    assert_eq!(responses.len(), 1);
    assert_eq!(responses[0]["result"]["action"], "decline");
    assert_eq!(responses[0]["result"]["content"], serde_json::Value::Null);
    assert_eq!(responses[0]["result"]["_meta"], serde_json::Value::Null);

    // The durable record: the same decided/deny the owner's Deny writes, with
    // the expiry named, and nothing granted.
    let summary = f
        .domain
        .approval_summary(request.request_id.clone())
        .await
        .unwrap();
    assert_eq!(summary.choice, Some(ApprovalChoice::Deny));
    assert_eq!(
        summary.state, "applying",
        "consumed once, never re-consumable"
    );
    assert_eq!(
        expiry_reason(&f, &request.request_id).as_deref(),
        Some("owner wait expired without an answer")
    );
    assert_eq!(f.count("SELECT COUNT(*) FROM approval_grants"), 0);
    assert_eq!(
        f.count("SELECT COUNT(*) FROM approval_responses WHERE write_accepted=1"),
        1
    );
    // The evidence that the host's expiry ran is the DURABLE receipt above, not
    // the phase journal: that journal is keyed by dispatch id, every fixture in
    // this binary uses the same id `dispatch`, and `operation()` resets the key
    // on each start — so under `cargo test`'s parallelism one scenario wipes
    // another's in-flight records. The label itself is pinned where it is
    // isolated, in `approval::state::trace_tests`.
    unconfirmed(&f);
}

/// The same rule for the pinned command family, whose decline is
/// `{"decision":"decline"}` rather than the elicitation's action. One rule,
/// two wire shapes, chosen by `ApprovalRequest::response(false)` alone.
#[tokio::test]
async fn native_owned_approval_owner_wait_expiry_command_family() {
    let f = Fixture::configured(true);
    let (mut op, mut notices) = operation(&f, "owned-approval", expiry_policy());
    let request = notice(&f, &mut op, &mut notices).await;
    let report = op.wait().await.unwrap();
    assert_eq!(
        report.protocol,
        Protocol::Completed,
        "{:?} {:?}; {}",
        report.failure,
        report.runtime_observation(),
        cancellation_trace(&f)
    );
    assert_eq!(report.failure, None);
    marker(&f, "approval-continued").await;
    let responses = responses(&f);
    assert_eq!(responses.len(), 1);
    assert_eq!(responses[0]["result"]["decision"], "decline");
    assert_eq!(
        f.domain
            .approval_summary(request.request_id.clone())
            .await
            .unwrap()
            .choice,
        Some(ApprovalChoice::Deny)
    );
    assert_eq!(
        expiry_reason(&f, &request.request_id).as_deref(),
        Some("owner wait expired without an answer")
    );
    unconfirmed(&f);
}

/// An ALLOW that first arrives after the owner cutoff is never applied.
///
/// The response reserve is not owner decision time: it exists so the host can
/// answer, not so a late verdict can be revived (ADR046, the control pump's
/// own rule). The host has already recorded the expiry as a deny, so the
/// owner's late tap reaches `decide_verdict` and is refused there — the stale
/// card, refused rather than applied — and the only frame on the wire stays
/// the decline. No accept can ever be written: the runtime refuses one on an
/// expired callback independently of this path.
#[tokio::test]
async fn native_owned_approval_expired_allow_is_still_refused() {
    let f = Fixture::configured(true);
    let (mut op, mut notices) = operation(&f, "owned-approval", expiry_policy());
    let request = notice(&f, &mut op, &mut notices).await;
    // Read the card while the request is still pending; it is the owner's own
    // view, and it must not become authority afterwards.
    let card = f
        .domain
        .private_approval(request.request_id.clone())
        .await
        .unwrap();
    assert!(
        card.expires_at > request.owner_expires_at,
        "the reserve really is the later bound"
    );
    // Well past the cutoff, so the host has certainly recorded the expiry: the
    // loop acts within its 100 ms wake of the bound.
    tokio::time::sleep(Duration::from_millis(
        request.owner_expires_at.saturating_sub(now()) + 1000,
    ))
    .await;
    assert!(now() < card.expires_at, "still inside the response reserve");
    let late = f
        .domain
        .observe_owner_verdict(hagency_core::approvals::OwnerVerdictObservation {
            request_id: request.request_id.clone(),
            request_digest: card.digest,
            binding_generation: card.binding_generation,
            server_name: "example.test".into(),
            room_id: card.room_id,
            sender_mxid: card.owner_mxid,
            event_id: "$late".into(),
            encrypted: true,
            choice: ApprovalChoice::Once,
        })
        .await;
    assert!(
        late.is_err(),
        "a late allow must be refused, never applied; {}",
        retained_state(&f)
    );
    let report = op.wait().await.unwrap();
    assert_eq!(report.failure, None, "the turn still continues");
    let responses = responses(&f);
    assert_eq!(responses.len(), 1, "one frame, and it is not an accept");
    assert_eq!(responses[0]["result"]["decision"], "decline");
    assert_eq!(
        f.domain
            .approval_summary(request.request_id.clone())
            .await
            .unwrap()
            .choice,
        Some(ApprovalChoice::Deny),
        "the recorded decision stays the host's deny"
    );
    // Nothing was granted to the late allow, in any scope.
    assert_eq!(f.count("SELECT COUNT(*) FROM approval_grants"), 0);
    unconfirmed(&f);
}
