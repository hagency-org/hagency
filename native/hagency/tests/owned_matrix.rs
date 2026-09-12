#[path = "owned_matrix/support.rs"]
mod support;
use hagency_core::tasks::{RunnerCommand, TaskState};
use hagency_execution::{Failure, Limits, Operation, Protocol, Settlement};
use hagency_matrix::{CancellationToken, HostIntakePlan, OutgoingState};
use hagency_runtime::owned::Cleanup;
use serde_json::{Value, json};
use support::*;

#[tokio::test]
async fn native_matrix_owned_complete_workflow() {
    let (w, mut fake) = Workflow::ready(false).await;
    let cancel = CancellationToken::new();
    let (intent, notice, seq) = w.pending(&mut fake).await;
    let (result, body) =
        common::scripted(w.collector.send_notice(notice.clone(), &cancel), async {
            let request = outgoing(&mut fake, &notice.claim.notice.transaction_id).await;
            assert_eq!(
                w.f.store
                    .verified_notice_receipt(notice.claim.notice.id.clone())
                    .await
                    .unwrap()
                    .state,
                "sending"
            );
            w.assert_inactive(&intent, seq).await;
            let body: Value = serde_json::from_slice(&request.body).unwrap();
            request.json(200, json!({"event_id":"$notice"}));
            body
        })
        .await;
    assert_eq!(result.unwrap().state, OutgoingState::Delivered);
    assert_eq!(body["msgtype"], "m.notice");
    assert_eq!(body["m.relates_to"]["rel_type"], "m.thread");
    assert_eq!(body["m.relates_to"]["event_id"], "$question");
    assert_eq!(
        w.intent_state(&intent.task_id),
        ("active".into(), Some("$notice".into()))
    );
    assert!(
        w.collector
            .send_notice(notice, &cancel)
            .await
            .unwrap()
            .replayed
    );
    fake.no_request().await;

    let active =
        w.f.store
            .inbox(intent.session_id.clone(), 0, 10, None)
            .await
            .unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].message.sequence, seq);
    assert_eq!(active[0].message.body, INPUT);
    assert_eq!(active[0].message.event_id, "$question");
    w.f.store
        .enqueue_inbox_dispatch(w.input(&intent), vec![seq])
        .await
        .unwrap();
    let cap =
        w.f.store
            .claim_dispatch("owned_host".into(), now(), 60_000, 60_000, 1)
            .await
            .unwrap()
            .unwrap();
    let scope = w.f.store.owned_dispatch_scope(cap.clone()).await.unwrap();
    assert_eq!(scope.task().id, intent.task_id);
    assert_eq!(scope.input().session_id, intent.session_id);
    assert_eq!(
        scope.input().payload["inbox"],
        serde_json::to_value(&active).unwrap()
    );
    let api = RunnerApi::start(&w).await;
    let mut operation = Operation::start(
        w.f.store.clone(),
        cap.clone(),
        w.host(api.address),
        Limits {
            operation_ms: 30_000,
            response_ms: 2000,
        },
    )
    .unwrap();
    let report = operation.wait().await.unwrap();
    // One event, N witnesses: when a shutdown stalled anywhere in this
    // process, this mismatch is a symptom of that stall, so the stall is the
    // reported reason. The test still fails either way.
    let observed = w.task(&intent.task_id);
    let observed_status = observed.status;
    let observed_epoch = observed.execution_epoch;
    if observed_status != TaskState::Done || observed_epoch != 1 {
        let stalled = common::stall::witness("owned_matrix complete workflow status");
        // Diagnostic only. Hosted Windows once saw `wait()` return Ok while this
        // raw read-only connection still read InProgress; print the writer's own
        // verdict and the observed row before asserting so the two views are
        // distinguishable in the artifact.
        eprintln!(
            "writer_view={:?} protocol={:?} settlement={:?} epoch={}",
            report.canonical_status, report.protocol, report.settlement, observed.execution_epoch,
        );
        if stalled {
            panic!(
                "task status {observed_status:?} != Done in a process with a recorded \
                 shutdown stall; see [shutdown-stall witness] above"
            );
        }
    }
    assert_eq!(
        observed.status,
        TaskState::Done,
        "writer_view={:?} protocol={:?} settlement={:?} epoch={}",
        report.canonical_status,
        report.protocol,
        report.settlement,
        observed.execution_epoch,
    );
    assert_eq!(observed.execution_epoch, 1);
    assert_eq!(report.canonical_status, Some(TaskState::Done));
    // This fixture intentionally emits no terminal Codex turn after the helper
    // finish. Writer completion, retained cleanup and delivery stay independent.
    assert_eq!(report.protocol, Protocol::Unknown);
    assert!(report.text.is_none());
    assert_eq!(w.count("SELECT COUNT(*) FROM owned_task_completions"), 1);
    assert!(
        w.f.store
            .runner_command(cap.clone(), RunnerCommand::Check)
            .await
            .is_err()
    );
    let Cleanup::Observed(cleanup) = &report.cleanup else {
        panic!("actual owner stop required")
    };
    assert!(cleanup.scope.leader_exited);
    let requests = std::fs::read_to_string(w.work.join("owned-mcp.requests")).unwrap();
    assert!(!requests.contains(&cap.secret));
    let requests: Vec<Value> = requests
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    let thread = requests
        .iter()
        .find(|v| v["method"] == "thread/start")
        .unwrap();
    assert_eq!(thread["params"]["approvalPolicy"], "on-request");
    assert_eq!(thread["params"]["sandbox"], "workspace-write");
    let turn = requests
        .iter()
        .find(|v| v["method"] == "turn/start")
        .unwrap();
    assert_eq!(turn["params"]["sandboxPolicy"]["networkAccess"], false);
    assert!(
        turn["params"]["input"][0]["text"]
            .as_str()
            .unwrap()
            .contains(INPUT)
    );
    assert_eq!(
        w.count("SELECT COUNT(*) FROM final_replies WHERE state='delivered'"),
        0
    );

    if cfg!(target_os = "macos") {
        assert!(!cleanup.scope.whole_tree_stopped);
        assert_eq!(report.failure, Some(Failure::CleanupUnknown));
        assert_eq!(w.count("SELECT COUNT(*) FROM resource_leases"), 1);
        assert!(w.f.store.claim_final_reply(60_000).await.unwrap().is_none());
        fake.no_request().await;
    } else {
        assert!(cleanup.scope.whole_tree_stopped && cleanup.scope.signals_accepted);
        assert_eq!(report.failure, None);
        // No store refusal was observed on this clean settlement.
        assert_eq!(report.settlement_cause, None);
        assert_eq!(report.settlement, Settlement::CanonicalReplyReady);
        assert_eq!(w.count("SELECT COUNT(*) FROM resource_leases"), 0);
        let claim = w.f.store.claim_final_reply(60_000).await.unwrap().unwrap();
        let send = w.f.store.preview_final_reply(claim.clone()).await.unwrap();
        assert_eq!(send.body, FINAL);
        assert_eq!(send.route.room_id, ROOM);
        assert_eq!(send.route.thread_root, Some("$question".into()));
        let (result, body) =
            common::scripted(w.collector.send_final(claim.clone(), &cancel), async {
                let request = outgoing(&mut fake, &send.transaction_id).await;
                assert_eq!(w.reply_state(&claim.id), "sending");
                assert_eq!(w.task(&intent.task_id).status, TaskState::Done);
                let body: Value = serde_json::from_slice(&request.body).unwrap();
                assert!(!String::from_utf8_lossy(&request.body).contains(&cap.secret));
                request.json(200, json!({"event_id":"$final"}));
                body
            })
            .await;
        assert_eq!(result.unwrap().state, OutgoingState::Delivered);
        assert_eq!(w.reply_state(&claim.id), "delivered");
        assert_eq!(body["body"], FINAL);
        assert!(
            body["formatted_body"]
                .as_str()
                .unwrap()
                .contains("<strong>native MCP final result</strong>")
        );
        assert_eq!(body["m.relates_to"]["rel_type"], "m.thread");
        assert_eq!(body["m.relates_to"]["event_id"], "$question");
        assert!(
            w.collector
                .send_final(claim, &cancel)
                .await
                .unwrap()
                .replayed
        );
        fake.no_request().await;
    }
    drop(report);
    drop(operation);
    api.close().await;
    w.close().await;
    fake.close().await;
}

#[tokio::test]
async fn native_matrix_owned_notice_failure() {
    for lost in [false, true] {
        let (w, mut fake) = Workflow::ready(false).await;
        let (intent, notice, seq) = w.pending(&mut fake).await;
        let cancel = CancellationToken::new();
        let (result, ()) =
            common::scripted(w.collector.send_notice(notice.clone(), &cancel), async {
                let request = outgoing(&mut fake, &notice.claim.notice.transaction_id).await;
                if lost {
                    drop(request)
                } else {
                    request.json(403, json!({"errcode":"M_FORBIDDEN"}));
                }
            })
            .await;
        assert!(result.is_err());
        assert_eq!(
            w.collector
                .resume_outgoing_custody(&cancel)
                .await
                .unwrap()
                .state,
            OutgoingState::Uncertain
        );
        w.assert_inactive(&intent, seq).await;
        assert!(w.collector.send_notice(notice, &cancel).await.is_err());
        fake.no_request().await;
        w.close().await;
        fake.close().await;
    }
}

#[tokio::test]
async fn native_matrix_owned_private_plaintext_refused() {
    let (w, mut fake) = Workflow::ready(true).await;
    let cancel = CancellationToken::new();
    let mut event = event();
    event["content"]
        .as_object_mut()
        .unwrap()
        .remove("m.mentions");
    event["encryption_info"] = json!({"verification_state":"verified","sender":"@owner:example.test","sender_device":"HUMAN"});
    let (result, ()) = common::scripted(
        w.collector
            .intake(HostIntakePlan::new(vec!["root".into()]).unwrap(), &cancel),
        async {
            fake.next().await.json(200, common::who());
            fake.next().await.json(200, sync(event));
            fake.next().await.json(200, room(true));
        },
    )
    .await;
    let result = result.unwrap();
    assert_eq!((result.admitted, result.rejected), (0, 1));
    assert!(w.f.available().await);
    assert_eq!(w.count("SELECT COUNT(*) FROM admitted_messages"), 0);
    assert_eq!(w.count("SELECT COUNT(*) FROM canonical_tasks"), 0);
    w.assert_no_execution();
    fake.no_request().await;
    w.close().await;
    fake.close().await;
}
