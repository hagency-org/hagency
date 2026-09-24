use super::*;
use hagency_execution::StopInspectionStatus;
use sha2::{Digest, Sha256};

#[tokio::test]
async fn native_owned_stopped_capacity_real_process() {
    for missing in [false, true] {
        let f = Fixture::new();
        let mut original = Operation::start(
            f.domain.clone(),
            f.cap.clone(),
            f.host("eof", "work", missing),
            limits(),
        )
        .unwrap();
        let original_report = original.wait().await.unwrap();
        let proven = !missing && cfg!(any(target_os = "linux", target_os = "macos", windows));
        assert_eq!(
            original_report.stop_inspection_status() == StopInspectionStatus::Recorded,
            proven
        );
        f.quarantined();
        f.domain
            .register_workspace("independent_work".into())
            .await
            .unwrap();
        f.domain
            .register_session(SessionBinding {
                id: "independent_session".into(),
                engagement_id: f.engagement.clone(),
                room_id: "!independent:example.test".into(),
                thread_root: None,
            })
            .await
            .unwrap();
        f.domain
            .create_canonical_task(
                "independent_task".into(),
                "independent_session".into(),
                "Independent offline task".into(),
                now(),
            )
            .await
            .unwrap();
        f.domain
            .enqueue_dispatch(DispatchInput {
                id: "independent_dispatch".into(),
                session_id: "independent_session".into(),
                task_id: Some("independent_task".into()),
                resources: vec![ResourceLease {
                    id: "independent_work".into(),
                    exclusive: true,
                }],
                payload: json!({"instruction":"independent offline task"}),
            })
            .await
            .unwrap();
        let next = f
            .domain
            .claim_dispatch("next_host".into(), now(), 60_000, 60_000, 1)
            .await
            .unwrap();
        assert_eq!(next.is_some(), proven);
        if let Some(next) = next {
            let path = f.root.path().join("independent-work");
            hagency_store::private::directory(&path).unwrap();
            let path = path.canonicalize().unwrap();
            let mut environment = BTreeMap::from([
                ("PATH".into(), "".into()),
                ("HAGENCY_OFFLINE_MODE".into(), "normal".into()),
                (
                    "HAGENCY_OPERATION_BUDGET_MS".into(),
                    limits().operation_ms.to_string().into(),
                ),
            ]);
            if let Some(system) = std::env::var_os("SystemRoot") {
                environment.insert("SystemRoot".into(), system);
            }
            let host = Host::new(
                binary(),
                binary(),
                environment,
                BTreeMap::from([("independent_work".into(), path.clone())]),
            )
            .unwrap();
            let mut operation = Operation::start(f.domain.clone(), next, host, limits()).unwrap();
            let report = operation.wait().await.unwrap();
            assert_eq!(report.protocol, Protocol::Completed);
            assert_eq!(report.settlement, Settlement::Completed);
            assert!(
                path.join("owned-dispatch.entered").is_file(),
                "second actual process must enter"
            );
            assert_eq!(original_report.failure, Some(Failure::Protocol));
            drop(report);
            drop(operation);
        }
        // Assert the original attempt, not the second process's accepted output.
        assert_eq!(f.state(), "outcome_unknown");
        assert_eq!(
            f.count("SELECT dirty FROM workspace_resources WHERE id='work'"),
            1
        );
        assert_eq!(
            f.count("SELECT quarantined FROM runner_sessions WHERE id='session'"),
            1
        );
        assert_eq!(
            f.count("SELECT COUNT(*) FROM resource_leases WHERE dispatch_id='dispatch'"),
            1
        );
        assert_eq!(f.count("SELECT COUNT(*) FROM dispatch_stops WHERE dispatch_id='dispatch' AND settled_at IS NULL"),1);
        assert_eq!(
            f.count(
                "SELECT COUNT(*) FROM runner_outputs WHERE dispatch_id='dispatch' AND accepted=1"
            ),
            0
        );
        assert_eq!(f.count("SELECT COUNT(*) FROM canonical_tasks WHERE id='task' AND json_extract(config,'$.status')='in_progress'"),1);
        drop(original_report);
        drop(original);
        f.domain.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn native_owned_stopped_inspection() {
    let f = Fixture::new();
    fs::write(f.work.join("retained.txt"), b"original stopped contents").unwrap();
    let mut operation = f.operation("eof");
    let mut report = operation.wait().await.unwrap();
    assert!(f.marker().is_file(), "must inspect after an actual process");
    assert_eq!(report.failure, Some(Failure::Protocol));
    f.quarantined();
    if cfg!(any(target_os = "linux", target_os = "macos", windows)) {
        assert_eq!(
            report.stop_inspection_status(),
            StopInspectionStatus::Recorded
        );
        let receipt = f
            .domain
            .owned_stop_inspection(f.cap.dispatch_id.clone(), f.cap.fence)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(receipt["dispatch_id"], f.cap.dispatch_id);
        assert_eq!(receipt["fence"], f.cap.fence);
        let entries = receipt["observation"]["inventory"]["entries"]
            .as_array()
            .unwrap();
        let file = entries
            .iter()
            .find(|entry| entry["path"] == "retained.txt")
            .unwrap();
        assert_eq!(
            file["sha256"],
            format!("{:x}", Sha256::digest(b"original stopped contents"))
        );
        // Caller changes cannot rescan or replace the original observation.
        fs::write(f.work.join("retained.txt"), b"later operator edit").unwrap();
        report.cleanup = Cleanup::Pending;
        report.settlement = Settlement::Completed;
        assert_eq!(
            report.retry_stop_inspection().await,
            StopInspectionStatus::Recorded
        );
        assert_eq!(
            f.domain
                .owned_stop_inspection(f.cap.dispatch_id.clone(), f.cap.fence)
                .await
                .unwrap(),
            Some(receipt.clone())
        );
        f.quarantined();
        drop(report);
        drop(operation);
        f.domain.shutdown().await.unwrap();
        let db = DomainRepository::open(&f.root.path().join("state")).unwrap();
        assert_eq!(
            db.owned_stop_inspection(&f.cap.dispatch_id, f.cap.fence)
                .unwrap(),
            Some(receipt)
        );
        f.quarantined();
    } else {
        assert_eq!(
            report.stop_inspection_status(),
            StopInspectionStatus::Unavailable
        );
        drop(report);
        drop(operation);
        f.domain.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn native_owned_stopped_inspection_refusals() {
    for mode in ["missing", "too-large"] {
        let f = Fixture::new();
        if mode == "too-large" {
            fs::File::create(f.work.join("large-file"))
                .unwrap()
                .set_len(64 * 1024 * 1024 + 1)
                .unwrap();
        }
        let mut operation = Operation::start(
            f.domain.clone(),
            f.cap.clone(),
            f.host("eof", "work", mode == "missing"),
            limits(),
        )
        .unwrap();
        let mut report = operation.wait().await.unwrap();
        let expected = if mode == "too-large"
            && cfg!(any(target_os = "linux", target_os = "macos", windows))
        {
            StopInspectionStatus::Refused
        } else {
            StopInspectionStatus::Unavailable
        };
        assert_eq!(report.stop_inspection_status(), expected);
        // Mutable report diagnostics do not authorize any fresh inspection.
        report.failure = None;
        report.settlement = Settlement::Completed;
        assert_eq!(report.retry_stop_inspection().await, expected);
        assert!(
            f.domain
                .owned_stop_inspection(f.cap.dispatch_id.clone(), f.cap.fence)
                .await
                .unwrap()
                .is_none()
        );
        f.quarantined();
        drop(report);
        drop(operation);
        f.domain.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn native_owned_stopped_continuation_real_process() {
    let f = Fixture::new();
    let mut original = f.operation("eof");
    let report = original.wait().await.unwrap();
    assert_eq!(report.failure, Some(Failure::Protocol));
    assert_eq!(
        report.stop_inspection_status(),
        StopInspectionStatus::Recorded
    );
    f.quarantined();
    let receipt = f
        .domain
        .stopped_dispatch_inspection(f.engagement.clone(), f.cap.dispatch_id.clone())
        .await
        .unwrap();
    let digest = receipt["digest"].as_str().unwrap().to_owned();
    let next = DispatchInput {
        id: "inspected_replacement".into(),
        session_id: "session".into(),
        task_id: Some("task".into()),
        resources: vec![ResourceLease {
            id: "work".into(),
            exclusive: true,
        }],
        payload: json!({"instruction":"After reviewing the stopped fixture, complete the remaining offline work"}),
    };
    f.domain
        .continue_stopped_dispatch(
            f.engagement.clone(),
            f.cap.dispatch_id.clone(),
            (f.cap.fence, digest.clone()),
            next.clone(),
            "Reviewed original stopped fixture and workspace".into(),
        )
        .await
        .unwrap();
    let cap = f
        .domain
        .claim_dispatch("replacement_host".into(), now(), 60_000, 60_000, 1)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(cap.dispatch_id, next.id);
    // The existing marker was produced by the FAILED process; remove it so
    // this assertion proves the replacement really enters the same workspace.
    fs::remove_file(f.marker()).unwrap();
    let mut operation = Operation::start(
        f.domain.clone(),
        cap,
        f.host("normal", "work", false),
        limits(),
    )
    .unwrap();
    let done = operation.wait().await.unwrap();
    assert_eq!(done.protocol, Protocol::Completed);
    assert_eq!(done.settlement, Settlement::Completed);
    assert!(f.marker().is_file());
    assert_eq!(f.state(), "outcome_unknown");
    assert_eq!(f.count("SELECT COUNT(*) FROM resource_leases"), 0);
    assert_eq!(f.count("SELECT COUNT(*) FROM dispatch_stops WHERE dispatch_id='dispatch' AND settled_at IS NOT NULL"),1);
    assert_eq!(
        f.count("SELECT COUNT(*) FROM runner_outputs WHERE dispatch_id='dispatch' AND accepted=1"),
        0
    );
    assert_eq!(f.count("SELECT COUNT(*) FROM runner_dispatches WHERE id='inspected_replacement' AND state='completed'"),1);
    f.domain
        .continue_stopped_dispatch(
            f.engagement.clone(),
            f.cap.dispatch_id.clone(),
            (f.cap.fence, digest),
            next,
            "Reviewed original stopped fixture and workspace".into(),
        )
        .await
        .unwrap();
    drop(done);
    drop(operation);
    drop(report);
    drop(original);
    f.domain.shutdown().await.unwrap();
}

#[tokio::test]
async fn native_owned_outcome_resolution_real_process() {
    use hagency_store::{OutcomeAction, OutcomeResolution};
    for action in [
        OutcomeAction::Continue,
        OutcomeAction::AcceptCompleted,
        OutcomeAction::KeepBlocked,
    ] {
        let f = Fixture::new();
        let mut original = f.operation("eof");
        let report = original.wait().await.unwrap();
        assert_eq!(
            report.failure,
            Some(Failure::Protocol),
            "action={}, startup={:?}, runtime={:?}",
            serde_json::to_value(action).unwrap(),
            report.startup_error(),
            report.runtime_observation()
        );
        f.quarantined();
        if !cfg!(any(target_os = "linux", target_os = "macos", windows)) {
            assert!(
                f.domain
                    .begin_outcome_inspection(
                        f.engagement.clone(),
                        f.cap.dispatch_id.clone(),
                        60_000
                    )
                    .await
                    .is_err()
            );
            drop(report);
            drop(original);
            f.domain.shutdown().await.unwrap();
            continue;
        }
        assert_eq!(
            report.stop_inspection_status(),
            StopInspectionStatus::Recorded
        );
        let inspection = f
            .domain
            .begin_outcome_inspection(f.engagement.clone(), f.cap.dispatch_id.clone(), 60_000)
            .await
            .unwrap();
        let next = DispatchInput {
            id: "token_replacement".into(),
            session_id: "session".into(),
            task_id: Some("task".into()),
            resources: vec![ResourceLease {
                id: "work".into(),
                exclusive: true,
            }],
            payload: json!({"instruction":"Finish the remaining inspected offline work"}),
        };
        let input = OutcomeResolution {
            original: f.cap.dispatch_id.clone(),
            request_id: "reviewed_decision".into(),
            inspection_id: inspection["inspectionId"].as_str().unwrap().into(),
            inspection_token: inspection["inspectionToken"].as_str().unwrap().into(),
            action,
            operator_note: "Reviewed actual stopped process and its retained workspace".into(),
            replacement: (action == OutcomeAction::Continue).then_some(next),
        };
        let response = f
            .domain
            .resolve_stopped_dispatch(f.engagement.clone(), input.clone())
            .await
            .unwrap();
        // The failed process and its original report remain negative regardless
        // of the operator's independent canonical-task decision.
        assert_eq!(report.failure, Some(Failure::Protocol));
        assert_eq!(f.state(), "outcome_unknown");
        assert_eq!(
            f.count(
                "SELECT COUNT(*) FROM runner_outputs WHERE dispatch_id='dispatch' AND accepted=1"
            ),
            0
        );
        assert_eq!(f.count("SELECT COUNT(*) FROM final_replies"), 0);
        assert_eq!(
            f.count("SELECT COUNT(*) FROM resource_leases WHERE dispatch_id='dispatch'"),
            0
        );
        if action == OutcomeAction::Continue {
            let cap = f
                .domain
                .claim_dispatch("token_runner".into(), now(), 60_000, 60_000, 1)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(cap.dispatch_id, "token_replacement");
            fs::remove_file(f.marker()).unwrap();
            let mut next = Operation::start(
                f.domain.clone(),
                cap,
                f.host("normal", "work", false),
                limits(),
            )
            .unwrap();
            let done = next.wait().await.unwrap();
            assert_eq!(done.protocol, Protocol::Completed);
            assert_eq!(done.settlement, Settlement::Completed);
            assert!(f.marker().is_file());
            drop(done);
            drop(next);
        } else {
            assert!(
                f.domain
                    .claim_dispatch("unrequested_runner".into(), now(), 60_000, 60_000, 1)
                    .await
                    .unwrap()
                    .is_none()
            );
            assert_eq!(
                response["task"]["status"],
                if action == OutcomeAction::AcceptCompleted {
                    "done"
                } else {
                    "blocked"
                }
            );
        }
        assert_eq!(
            f.domain
                .resolve_stopped_dispatch(f.engagement.clone(), input)
                .await
                .unwrap(),
            response
        );
        drop(report);
        drop(original);
        f.domain.shutdown().await.unwrap();
    }
}
