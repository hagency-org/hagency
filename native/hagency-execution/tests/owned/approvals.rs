use super::{approval_fixture::*, *};
use hagency_core::approvals::ApprovalChoice;

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
            cancellation_trace()
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
        cancellation_trace()
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
        cancellation_trace()
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
    // An owner choice first delivered after the original owner cutoff cannot
    // use the response reserve to create new preparation or router authority.
    let f = Fixture::configured(true);
    let (mut op, mut notices) = operation(
        &f,
        "owned-approval",
        hagency_execution::ApprovalHost::new(2, 1, 300, 1500).unwrap(),
    );
    let request = notice(&f, &mut op, &mut notices).await;
    let card = f
        .domain
        .private_approval(request.request_id.clone())
        .await
        .unwrap();
    assert!(card.expires_at > request.owner_expires_at);
    tokio::time::sleep(Duration::from_millis(
        request.owner_expires_at.saturating_sub(now()) + 10,
    ))
    .await;
    assert!(now() < card.expires_at, "within original response reserve");
    let _ = f
        .domain
        .observe_owner_verdict(hagency_core::approvals::OwnerVerdictObservation {
            request_id: request.request_id,
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
    let report = op.wait().await.unwrap();
    assert!(report.failure.is_some());
    assert!(responses(&f).is_empty());
    assert_eq!(f.count("SELECT COUNT(*) FROM approval_responses"), 0);
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
        cancellation_trace()
    );
    let usage = report.usage_status();
    assert_eq!(usage.observed, 3);
    assert_eq!(usage.acknowledged, 3);
    assert!(!usage.pending);
    assert!(!usage.rejected);
    assert_eq!(usage.failure, None);
    unconfirmed(&f);
}
