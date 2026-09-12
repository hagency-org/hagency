//! Test-only receipt delivery faults after actual owned operations.
use crate::{approval::Fault, test_common::*, *};
use hagency_core::{approvals::*, replies::*, tasks::*};
use hagency_store::{DomainRepository, DomainStore, EffectOutcome};
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
fn host(root: &std::path::Path, fault: Fault, mode: &str) -> Host {
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
    let mut environment = BTreeMap::from([
        ("PATH".into(), "".into()),
        ("HAGENCY_OFFLINE_MODE".into(), mode.into()),
    ]);
    if let Some(system) = std::env::var_os("SystemRoot") {
        environment.insert("SystemRoot".into(), system);
    }
    let mut host = Host::new(
        binary.clone(),
        binary,
        environment,
        BTreeMap::from([("work".into(), root.to_owned())]),
    )
    .unwrap()
    .with_approvals(ApprovalHost::new(2, 1, 20_000, 1500).unwrap())
    .unwrap();
    crate::approval::diagnostics::reset();
    host.approval_fault = Some(fault);
    host
}
pub(crate) fn fixture(root: &std::path::Path) -> (DomainStore, RunnerCapability) {
    let mut db = DomainRepository::open(&root.join("state")).unwrap();
    db.register(&registration()).unwrap();
    let pool = resource("pool", "seat", 1000);
    db.put_resource(&pool).unwrap();
    let request = proof(&request("allocation", "Worker", &pool, 100));
    let engagement = db.admit(&request, 1000).unwrap();
    db.approve("approve", &request, 1000).unwrap();
    let effect = db.claim_effect().unwrap().unwrap();
    db.observe_effect(
        &effect.id,
        effect.fence,
        &EffectOutcome::Applied {
            receipt: "offline provisioning".into(),
        },
    )
    .unwrap();
    db.observe_matrix_transport(
        &MatrixTransportObservation {
            engagement_id: engagement.id.clone(),
            registration_generation: 1,
            generation: 1,
            sender_mxid: "@worker:example.test".into(),
            device_id: "WORKER".into(),
        },
        now(),
    )
    .unwrap();
    db.observe_matrix_room(
        &MatrixRoomObservation {
            engagement_id: engagement.id.clone(),
            registration_generation: 1,
            transport_generation: 1,
            generation: 1,
            room_id: "!project:example.test".into(),
            privacy: RoomPrivacy::Group {},
            joined: BTreeSet::from(["@worker:example.test".into(), "@owner:example.test".into()]),
            invite_only: true,
            encrypted: false,
        },
        now(),
    )
    .unwrap();
    db.observe_approval_room(
        &ApprovalRoomObservation {
            engagement_id: engagement.id.clone(),
            registration_generation: 1,
            generation: 1,
            room_id: "!private:example.test".into(),
            device_id: "BOT".into(),
            joined: BTreeSet::from([
                "@owner:example.test".into(),
                "@approval:example.test".into(),
            ]),
            invite_only: true,
            encrypted: true,
            available: true,
        },
        now(),
    )
    .unwrap();
    db.resolve_verified_matrix_session(
        &SessionBinding {
            id: "session".into(),
            engagement_id: engagement.id,
            room_id: "!project:example.test".into(),
            thread_root: None,
        },
        now(),
    )
    .unwrap();
    db.register_workspace("work").unwrap();
    db.create_canonical_task("task", "session", "Owned receipt loss", now())
        .unwrap();
    db.enqueue_dispatch(&DispatchInput {
        id: "dispatch".into(),
        session_id: "session".into(),
        task_id: Some("task".into()),
        resources: vec![ResourceLease {
            id: "work".into(),
            exclusive: true,
        }],
        payload: json!({"instruction":"offline"}),
    })
    .unwrap();
    let cap = db
        .claim_dispatch("host", now(), 60000, 60000, 1)
        .unwrap()
        .unwrap();
    (DomainStore::start(db, 16).unwrap(), cap)
}

#[tokio::test]
async fn native_owned_approval_caller_loss() {
    for fault in [
        Fault::RequestAck,
        Fault::ConsumeAck,
        Fault::BeginAck,
        Fault::WriteAck,
        Fault::SpawnPanic,
        Fault::WritePanic,
    ] {
        let root = tempfile::tempdir().unwrap();
        let work = root.path().join("work");
        hagency_store::private::directory(&work).unwrap();
        let work = work.canonicalize().unwrap();
        let (domain, cap) = fixture(root.path());
        let mut op = Operation::start(
            domain.clone(),
            cap.clone(),
            host(&work, fault, "owned-approval"),
            Limits {
                operation_ms: 25_000,
                response_ms: 1500,
            },
        )
        .unwrap();
        let mut notices = op.take_approval_requests().unwrap();
        if !matches!(fault, Fault::RequestAck | Fault::SpawnPanic) {
            let notice = tokio::time::timeout(Duration::from_secs(6), notices.recv())
                .await
                .unwrap()
                .unwrap();
            let card = domain
                .private_approval(notice.request_id.clone())
                .await
                .unwrap();
            domain
                .observe_owner_verdict(OwnerVerdictObservation {
                    request_id: notice.request_id,
                    request_digest: card.digest,
                    binding_generation: card.binding_generation,
                    server_name: "example.test".into(),
                    room_id: card.room_id,
                    sender_mxid: card.owner_mxid,
                    event_id: "$verdict".into(),
                    encrypted: true,
                    choice: ApprovalChoice::Once,
                })
                .await
                .unwrap();
        }
        let report = op.wait().await.unwrap();
        assert!(report.failure.is_some(), "{fault:?}");
        assert!(
            report.runtime_observation().is_some(),
            "original owner captured before coordinator stop: {fault:?}"
        );
        let (callbacks, frames, grants, writes) = report.approval_custody();
        if fault == Fault::SpawnPanic {
            assert_eq!((callbacks, frames, grants, writes), (0, 0, 0, 0));
        } else {
            assert_eq!(callbacks, 1, "{fault:?}");
            assert_eq!(frames, usize::from(fault != Fault::RequestAck), "{fault:?}");
            assert_eq!(
                grants,
                usize::from(matches!(
                    fault,
                    Fault::BeginAck | Fault::WriteAck | Fault::WritePanic
                )),
                "{fault:?}"
            );
            assert_eq!(
                writes,
                usize::from(matches!(fault, Fault::WriteAck | Fault::WritePanic)),
                "{fault:?}"
            );
        }
        let sql = rusqlite::Connection::open(root.path().join("state/domain.sqlite3")).unwrap();
        let applied: u64 = sql
            .query_row(
                "SELECT COUNT(*) FROM owner_approvals WHERE state='applied'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(applied, 0);
        let requests =
            std::fs::read_to_string(work.join("owned-dispatch.requests")).unwrap_or_default();
        let responses: Vec<serde_json::Value> = requests
            .lines()
            .map(|v| serde_json::from_str(v).unwrap())
            .filter(|v: &serde_json::Value| v.get("result").is_some())
            .collect();
        assert!(
            responses.len() <= writes,
            "{fault:?}: no response before real write or repeated response"
        );
        assert_eq!(
            sql.query_row("SELECT COUNT(*) FROM resource_leases", [], |r| r
                .get::<_, u64>(0))
                .unwrap(),
            1
        );
    }
}

async fn choose(domain: &DomainStore, id: String) {
    let card = domain.private_approval(id.clone()).await.unwrap();
    domain
        .observe_owner_verdict(OwnerVerdictObservation {
            request_id: id,
            request_digest: card.digest.clone(),
            binding_generation: card.binding_generation,
            server_name: "example.test".into(),
            room_id: card.room_id,
            sender_mxid: card.owner_mxid,
            event_id: format!("${}", card.digest),
            encrypted: true,
            choice: ApprovalChoice::Once,
        })
        .await
        .unwrap();
}
#[tokio::test]
async fn native_owned_approval_barriers_pending_receipt() {
    use std::sync::{Arc, atomic::Ordering};
    for fault in [Fault::BeginGate, Fault::RecheckGate] {
        let root = tempfile::tempdir().unwrap();
        let work = root.path().join("work");
        hagency_store::private::directory(&work).unwrap();
        let work = work.canonicalize().unwrap();
        let (domain, cap) = fixture(root.path());
        let gate = Arc::new(crate::approval::Gate::default());
        let mut configured = host(&work, fault, "owned-approval-queued");
        configured.approval_gate = Some(gate.clone());
        let mut op = Operation::start(
            domain.clone(),
            cap.clone(),
            configured,
            Limits {
                operation_ms: 25_000,
                response_ms: 1500,
            },
        )
        .unwrap();
        let mut notices = op.take_approval_requests().unwrap();
        let first = tokio::time::timeout(Duration::from_secs(6), notices.recv())
            .await
            .unwrap()
            .unwrap();
        choose(&domain, first.request_id).await;
        let end = tokio::time::Instant::now() + Duration::from_secs(2);
        while !gate.entered.load(Ordering::Acquire) {
            assert!(
                tokio::time::Instant::now() < end,
                "original receipt did not reach gate"
            );
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        domain
            .runner_command(
                cap.clone(),
                RunnerCommand::Mutate {
                    id: "task".into(),
                    call_id: "authorized_comment".into(),
                    operation: TaskMutation::Comment {
                        text: "same retained authorized attempt".into(),
                    },
                },
            )
            .await
            .unwrap();
        std::fs::write(work.join("owned-dispatch.approval-release"), b"release").unwrap();
        let second = tokio::time::timeout(Duration::from_secs(1), notices.recv())
            .await
            .unwrap()
            .unwrap();
        let sql = rusqlite::Connection::open(root.path().join("state/domain.sqlite3")).unwrap();
        assert_eq!(
            sql.query_row(
                "SELECT state FROM runner_dispatches WHERE id='dispatch'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            "parked"
        );
        assert!(
            !work.join("owned-dispatch.approval-bytes").exists(),
            "older exact frame must remain unsent"
        );
        assert!(
            !gate.release.load(Ordering::Acquire),
            "second request was persisted while the original receipt remained pending"
        );
        assert!(
            domain
                .runner_command(
                    cap.clone(),
                    RunnerCommand::Mutate {
                        id: "task".into(),
                        call_id: "parked_comment".into(),
                        operation: TaskMutation::Comment {
                            text: "parked must refuse".into()
                        }
                    }
                )
                .await
                .is_err()
        );
        choose(&domain, second.request_id).await;
        gate.release.store(true, Ordering::Release);
        let report = op.wait().await.unwrap();
        assert_eq!(
            report.protocol,
            Protocol::Completed,
            "{fault:?}: {:?} {:?}; {}",
            report.failure,
            report.runtime_observation(),
            crate::approval::diagnostics::last_cancellation_trace()
        );
        assert_eq!(report.approval_custody(), (2, 2, 2, 2));
        assert_eq!(
            sql.query_row(
                "SELECT COUNT(*) FROM approval_responses WHERE write_accepted=1",
                [],
                |r| r.get::<_, u64>(0)
            )
            .unwrap(),
            2
        );
        assert_eq!(
            sql.query_row(
                "SELECT COUNT(*) FROM owner_approvals WHERE state='applied'",
                [],
                |r| r.get::<_, u64>(0)
            )
            .unwrap(),
            0
        );
    }
}

#[tokio::test]
async fn native_owned_approval_usage_successful_control() {
    use std::sync::{Arc, atomic::Ordering};
    let root = tempfile::tempdir().unwrap();
    let work = root.path().join("work");
    hagency_store::private::directory(&work).unwrap();
    let work = work.canonicalize().unwrap();
    let (domain, cap) = fixture(root.path());
    let gate = Arc::new(crate::approval::Gate::default());
    let mut configured = host(&work, Fault::BeginGate, "owned-approval-queued-usage");
    configured.approval_gate = Some(gate.clone());
    let mut op = Operation::start(
        domain.clone(),
        cap,
        configured,
        Limits {
            operation_ms: 25_000,
            response_ms: 1500,
        },
    )
    .unwrap();
    let mut notices = op.take_approval_requests().unwrap();
    let first = tokio::time::timeout(Duration::from_secs(6), notices.recv())
        .await
        .unwrap()
        .unwrap();
    choose(&domain, first.request_id).await;
    let end = tokio::time::Instant::now() + Duration::from_secs(2);
    while !gate.entered.load(Ordering::Acquire) {
        assert!(tokio::time::Instant::now() < end);
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let sql = rusqlite::Connection::open(root.path().join("state/domain.sqlite3")).unwrap();
    sql.execute_batch("BEGIN IMMEDIATE").unwrap();
    std::fs::write(work.join("owned-dispatch.approval-release"), b"release").unwrap();
    failed_usage_receipt(&gate).await;
    assert!(!gate.release.load(Ordering::Acquire));
    sql.execute_batch("COMMIT").unwrap();
    // The older begin has actually committed successfully, but its original
    // receipt remains pending. It cannot conceal the failed usage slot.
    gate.release.store(true, Ordering::Release);
    let report = op.wait().await.unwrap();
    assert_eq!(
        report.approval_custody().0,
        1,
        "later callback must never be read past an unacknowledged usage slot"
    );
    assert!(notices.recv().await.is_none());
    assert_eq!(report.usage_status().observed, 1);
    assert_eq!(report.usage_status().acknowledged, 0);
    assert!(report.usage_status().pending);
    assert_eq!(report.failure, Some(Failure::SettlementUnknown));
    assert_eq!(
        sql.query_row("SELECT COUNT(*) FROM owner_approvals", [], |r| r
            .get::<_, u64>(0))
            .unwrap(),
        1
    );
}

async fn failed_usage_receipt(gate: &crate::approval::Gate) {
    use std::sync::atomic::Ordering;
    let until = tokio::time::Instant::now() + Duration::from_secs(2);
    while !gate.usage_failed.load(Ordering::Acquire) {
        assert!(
            tokio::time::Instant::now() < until,
            "actual original usage writer did not report its failed receipt"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

#[tokio::test]
async fn native_owned_approval_usage_unknown_slot() {
    use std::sync::{Arc, atomic::Ordering};
    let root = tempfile::tempdir().unwrap();
    let work = root.path().join("work");
    hagency_store::private::directory(&work).unwrap();
    let work = work.canonicalize().unwrap();
    let (domain, cap) = fixture(root.path());
    let gate = Arc::new(crate::approval::Gate::default());
    let mut configured = host(&work, Fault::MaintainGate, "owned-approval-usage");
    configured.approval_gate = Some(gate.clone());
    let mut op = Operation::start(
        domain.clone(),
        cap,
        configured,
        Limits {
            operation_ms: 25_000,
            response_ms: 1500,
        },
    )
    .unwrap();
    let mut notices = op.take_approval_requests().unwrap();
    tokio::time::timeout(Duration::from_secs(6), notices.recv())
        .await
        .unwrap()
        .expect("actual pending owner request");
    let until = tokio::time::Instant::now() + Duration::from_secs(2);
    while !gate.entered.load(Ordering::Acquire) {
        assert!(
            tokio::time::Instant::now() < until,
            "original maintenance receipt gate"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let sql = rusqlite::Connection::open(root.path().join("state/domain.sqlite3")).unwrap();
    sql.execute_batch("BEGIN IMMEDIATE").unwrap();
    std::fs::write(work.join("owned-dispatch.approval-release"), b"release").unwrap();
    failed_usage_receipt(&gate).await;
    assert!(!gate.release.load(Ordering::Acquire));
    sql.execute_batch("COMMIT").unwrap();
    gate.release.store(true, Ordering::Release);
    let report = op.wait().await.unwrap();
    assert_eq!(report.failure, Some(Failure::SettlementUnknown));
    assert_eq!(report.approval_custody().0, 1);
    let usage = report.usage_status();
    assert_eq!(
        usage.observed, 1,
        "never read past the failed original usage slot"
    );
    assert_eq!(usage.acknowledged, 0);
    assert!(usage.pending);
    assert_eq!(usage.failure, Some(UsageFailure::Storage));
    assert!(!work.join("owned-dispatch.approval-bytes").exists());
    assert_eq!(
        sql.query_row("SELECT COUNT(*) FROM approval_responses", [], |r| r
            .get::<_, u64>(0))
            .unwrap(),
        0
    );
}

/// The acceptance call succeeded and the store committed the row, but the
/// caller's reply was lost. The reconcile read must find the accepted row and
/// continue along the successful path, with no second frame and no double write.
#[tokio::test]
async fn native_owned_approval_acceptance_reconcile_accepted() {
    let root = tempfile::tempdir().unwrap();
    let work = root.path().join("work");
    hagency_store::private::directory(&work).unwrap();
    let work = work.canonicalize().unwrap();
    let (domain, cap) = fixture(root.path());
    let mut op = Operation::start(
        domain.clone(),
        cap.clone(),
        host(&work, Fault::WriteAckLost, "owned-approval"),
        Limits {
            operation_ms: 25_000,
            response_ms: 1500,
        },
    )
    .unwrap();
    let mut notices = op.take_approval_requests().unwrap();
    let notice = tokio::time::timeout(Duration::from_secs(6), notices.recv())
        .await
        .unwrap()
        .unwrap();
    choose(&domain, notice.request_id).await;
    let report = op.wait().await.unwrap();
    // The reconcile found the committed row, so the attempt completes exactly
    // as a successful acknowledgement would have.
    assert_eq!(
        report.protocol,
        Protocol::Completed,
        "{:?} {:?}",
        report.failure,
        report.runtime_observation()
    );
    // The retained owner process cleanup reports unknown on macOS, as in
    // tests/owned/usage.rs; the reconcile itself adds no failure anywhere.
    assert_eq!(
        report.failure,
        if cfg!(target_os = "macos") {
            Some(Failure::CleanupUnknown)
        } else {
            None
        },
        "{:?}",
        report.runtime_observation()
    );
    assert_eq!(report.settlement_cause, None);
    // One callback, one retained frame, one grant, one accepted write.
    assert_eq!(report.approval_custody(), (1, 1, 1, 1));
    let requests =
        std::fs::read_to_string(work.join("owned-dispatch.requests")).unwrap_or_default();
    let responses: Vec<serde_json::Value> = requests
        .lines()
        .map(|v| serde_json::from_str(v).unwrap())
        .filter(|v: &serde_json::Value| v.get("result").is_some())
        .collect();
    assert_eq!(
        responses.len(),
        1,
        "exactly one response frame may ever be written for the id"
    );
    let sql = rusqlite::Connection::open(root.path().join("state/domain.sqlite3")).unwrap();
    assert_eq!(
        sql.query_row(
            "SELECT COUNT(*) FROM approval_responses WHERE state='write_accepted'",
            [],
            |r| r.get::<_, u64>(0)
        )
        .unwrap(),
        1
    );
    assert_eq!(
        sql.query_row(
            "SELECT COUNT(*) FROM approval_responses WHERE write_accepted=1",
            [],
            |r| r.get::<_, u64>(0)
        )
        .unwrap(),
        1
    );
}

/// The acceptance call never committed: the writer refused it, so the ordered
/// reconcile read answers that no accepted row exists. That is conclusive, so
/// the operation reports `SettlementUnknown` with the `AcceptanceUnrecorded`
/// cause, one frame on the wire, and no accepted row.
#[tokio::test]
async fn native_owned_approval_acceptance_reconcile_unrecorded() {
    use std::sync::{Arc, atomic::Ordering};
    let root = tempfile::tempdir().unwrap();
    let work = root.path().join("work");
    hagency_store::private::directory(&work).unwrap();
    let work = work.canonicalize().unwrap();
    let (domain, cap) = fixture(root.path());
    let gate = Arc::new(crate::approval::Gate::default());
    let mut configured = host(&work, Fault::RecheckGate, "owned-approval");
    configured.approval_gate = Some(gate.clone());
    let mut op = Operation::start(
        domain.clone(),
        cap.clone(),
        configured,
        Limits {
            operation_ms: 25_000,
            response_ms: 1500,
        },
    )
    .unwrap();
    let mut notices = op.take_approval_requests().unwrap();
    let notice = tokio::time::timeout(Duration::from_secs(6), notices.recv())
        .await
        .unwrap()
        .unwrap();
    choose(&domain, notice.request_id).await;
    // The host is held after its recheck and before the frame is written, so the
    // next store write it attempts is the acceptance observation. Taking the
    // single writer here makes that acceptance transaction refuse after its
    // bounded busy wait rather than commit.
    let end = tokio::time::Instant::now() + Duration::from_secs(2);
    while !gate.entered.load(Ordering::Acquire) {
        assert!(
            tokio::time::Instant::now() < end,
            "the receipt did not reach the recheck gate"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let sql = rusqlite::Connection::open(root.path().join("state/domain.sqlite3")).unwrap();
    sql.execute_batch("BEGIN IMMEDIATE").unwrap();
    gate.release.store(true, Ordering::Release);
    // Bounded and generous against the writer's 100 ms busy timeout: the
    // acceptance write refuses, the ordered read then answers "unrecorded".
    tokio::time::sleep(Duration::from_millis(600)).await;
    sql.execute_batch("COMMIT").unwrap();
    let report = op.wait().await.unwrap();
    assert_eq!(report.failure, Some(Failure::SettlementUnknown));
    assert_eq!(
        report.settlement_cause,
        Some(crate::SettlementCause::AcceptanceUnrecorded),
        "a conclusive negative read names the missing record"
    );
    // The transport receipt was recorded (`entry.write = Some`) before the
    // acceptance pump, and the write itself is a transport event unaffected by
    // the database lock; the 4th slot counts that physical receipt, not the
    // durable acceptance, which is exactly what this test shows is missing.
    assert_eq!(report.approval_custody(), (1, 1, 1, 1));
    let requests =
        std::fs::read_to_string(work.join("owned-dispatch.requests")).unwrap_or_default();
    let responses: Vec<serde_json::Value> = requests
        .lines()
        .map(|v| serde_json::from_str(v).unwrap())
        .filter(|v: &serde_json::Value| v.get("result").is_some())
        .collect();
    assert_eq!(
        responses.len(),
        1,
        "the frame is written once even though its record was refused"
    );
    assert_eq!(
        sql.query_row(
            "SELECT COUNT(*) FROM approval_responses WHERE write_accepted=1",
            [],
            |r| r.get::<_, u64>(0)
        )
        .unwrap(),
        0
    );
}
