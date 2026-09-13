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
        // The operation budget every scenario's `Limits::operation_ms`
        // grants (`Gate::OPERATION_BUDGET_MS`, 25 s) — the probe derives
        // every one of its waits from this value, never its own literal.
        (
            "HAGENCY_OPERATION_BUDGET_MS".into(),
            crate::approval::Gate::OPERATION_BUDGET_MS
                .to_string()
                .into(),
        ),
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
    crate::approval::diagnostics::reset("dispatch");
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
            let notice = tokio::time::timeout(harness_wait() * 3, notices.recv())
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
                "{fault:?}: expected an accepted write on the wire; host wrote {} frame(s), probe read {}, probe markers present: {}",
                host_response_frames(&work).len(),
                probe_read_frames(&work).len(),
                markers_present(&work),
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
        let first = tokio::time::timeout(harness_wait() * 3, notices.recv())
            .await
            .unwrap()
            .unwrap();
        choose(&domain, first.request_id).await;
        let end = tokio::time::Instant::now() + harness_wait();
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
        let second = tokio::time::timeout(harness_wait(), notices.recv())
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
            crate::approval::diagnostics::last_cancellation_trace(&cap.dispatch_id)
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
    let first = tokio::time::timeout(harness_wait() * 3, notices.recv())
        .await
        .unwrap()
        .unwrap();
    choose(&domain, first.request_id).await;
    let end = tokio::time::Instant::now() + harness_wait();
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
    let until = tokio::time::Instant::now() + harness_wait();
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
    tokio::time::timeout(harness_wait() * 3, notices.recv())
        .await
        .unwrap()
        .expect("actual pending owner request");
    let until = tokio::time::Instant::now() + harness_wait();
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
    let notice = tokio::time::timeout(harness_wait() * 3, notices.recv())
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
        "{:?} {:?}; host wrote {} frame(s), probe read {}",
        report.failure,
        report.runtime_observation(),
        host_response_frames(&work).len(),
        probe_read_frames(&work).len()
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
    // Negative control: this fixture drives approval acceptance only and never
    // records a completion row, so the completion block is skipped on every
    // platform and the successful drive completes plainly. The two branches
    // below differ in which cleanup gate refuses, not in custody: on Linux the
    // owner's cleanup is proven; on macOS the retained owner's cleanup stays
    // unproven, so the second gate refuses with `CleanupUnknown` and the
    // failure finalization records the store's observation beside it, and the
    // host asserts no unobserved Done.
    if cfg!(target_os = "macos") {
        // The worker's failure finalization records the store's observation
        // of the cleanup failure as the settlement: the lease is fenced for
        // reconciliation, never published.
        assert_eq!(report.failure, Some(Failure::CleanupUnknown));
        assert_eq!(
            report.settlement,
            Settlement::Negative(hagency_store::OwnedObservation::Fenced)
        );
        assert_ne!(
            report.canonical_status,
            Some(TaskState::Done),
            "an unproven cleanup must not assert an unobserved Done"
        );
    } else {
        // No completion row exists in this fixture, so there is nothing to
        // publish: the successful drive completes plainly.
        assert_eq!(report.settlement, Settlement::Completed);
    }
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
    let notice = tokio::time::timeout(harness_wait() * 3, notices.recv())
        .await
        .unwrap()
        .unwrap();
    choose(&domain, notice.request_id).await;
    // The host is held after its recheck and before the frame is written, so the
    // next store write it attempts is the acceptance observation. Taking the
    // single writer here makes that acceptance transaction refuse after its
    // bounded busy wait rather than commit.
    let end = tokio::time::Instant::now() + harness_wait();
    while !gate.entered.load(Ordering::Acquire) {
        assert!(
            tokio::time::Instant::now() < end,
            "the receipt did not reach the recheck gate; probe markers present: {}",
            markers_present(&work)
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let sql = rusqlite::Connection::open(root.path().join("state/domain.sqlite3")).unwrap();
    sql.execute_batch("BEGIN IMMEDIATE").unwrap();
    gate.release.store(true, Ordering::Release);
    // Hold the write lock until the operation has produced its verdict. The
    // acceptance write is refused by the 100 ms busy timeout; the reconcile read
    // is a WAL read and answers while the lock is still held, so the scenario no
    // longer races the host's scheduling. Bounded by the harness budget, not
    // widened: a premise that no longer holds must fail as a timeout, never pass
    // by waiting longer.
    let report = tokio::time::timeout(harness_wait() * 3, op.wait())
        .await
        .expect("the acceptance verdict must arrive while the lock is held")
        .unwrap();
    sql.execute_batch("COMMIT").unwrap();
    assert_eq!(report.failure, Some(Failure::SettlementUnknown));
    assert_eq!(
        report.settlement_cause,
        Some(crate::SettlementCause::AcceptanceUnrecorded),
        "a conclusive negative read names the missing record"
    );
    // The precedence rule: a settlement verdict is never replaced by the
    // completion path. This drive did not succeed, so no completion custody was
    // consulted and `canonical_status` keeps the last value the drive actually
    // observed (never an asserted Done), and nothing was published.
    assert_ne!(
        report.canonical_status,
        Some(TaskState::Done),
        "a failed drive must not assert an unobserved Done"
    );
    assert_ne!(
        report.settlement,
        Settlement::CanonicalReplyReady,
        "a settlement verdict must not be converted into a published completion"
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
/// The harness's derived wait: one tenth of the operation budget every
/// scenario below grants (`Limits::operation_ms`, 25 s). A literal here is
/// what let a loaded run miss a notice the host had lawfully not yet sent —
/// every wait scales with the budget the fixture actually runs.
const fn harness_wait() -> Duration {
    Duration::from_millis(crate::approval::Gate::OPERATION_BUDGET_MS / 10)
}

/// Approval response frames the probe actually read, one JSON line each.
fn probe_read_frames(work: &std::path::Path) -> Vec<serde_json::Value> {
    std::fs::read_to_string(work.join("owned-dispatch.approval-bytes"))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}
/// Host-written approval response frames (id starts with `approval-`).
fn host_response_frames(work: &std::path::Path) -> Vec<serde_json::Value> {
    std::fs::read_to_string(work.join("owned-dispatch.requests"))
        .unwrap()
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|value| {
            value["id"]
                .as_str()
                .is_some_and(|id| id.starts_with("approval-"))
                && value.get("result").is_some()
        })
        .collect()
}


/// Every `owned-dispatch.*` marker file currently present under `work`, so an
/// expired wait reports what the probe DID emit: a timing miss (the marker
/// arrives late) is distinguishable from a logic miss (it never comes).
fn markers_present(work: &std::path::Path) -> String {
    std::fs::read_dir(work)
        .map(|entries| {
            let mut names: Vec<String> = entries
                .filter_map(std::result::Result::ok)
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .filter(|name| name.starts_with("owned-dispatch."))
                .collect();
            names.sort();
            if names.is_empty() {
                "<none>".to_owned()
            } else {
                names.join(", ")
            }
        })
        .unwrap_or_else(|_| "<unreadable>".to_owned())
}
/// The middle case (design §3a): the host is held at the recheck gate with
/// `in_flight` set and the frame armed but unwritten; the fixture then emits
/// the resolution for that in-flight id. The write must still complete — a
/// resolution must not cancel a frame committed to the transport. Fails on
/// the pre-change predicate (`write.is_none()` alone cancels).
#[tokio::test]
async fn native_owned_approval_in_flight_resolution_completes_write() {
    use std::sync::{Arc, atomic::Ordering};
    let root = tempfile::tempdir().unwrap();
    let work = root.path().join("work");
    hagency_store::private::directory(&work).unwrap();
    let work = work.canonicalize().unwrap();
    let (domain, cap) = fixture(root.path());
    let gate = Arc::new(crate::approval::Gate::default());
    let mut configured = host(&work, Fault::RecheckGate, "owned-approval-gate-resolve");
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
    let notice = tokio::time::timeout(harness_wait() * 3, notices.recv())
        .await
        .unwrap()
        .unwrap();
    choose(&domain, notice.request_id).await;
    let end = tokio::time::Instant::now() + harness_wait();
    while !gate.entered.load(Ordering::Acquire) {
        assert!(
            tokio::time::Instant::now() < end,
            "original recheck did not reach gate"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    // The host is in flight at the gate; release both halves. The probe now
    // emits the resolution BEFORE the write, which is the state under test.
    gate.release.store(true, Ordering::Release);
    std::fs::write(work.join("owned-dispatch.approval-release"), b"release").unwrap();
    let report = op.wait().await.unwrap();
    assert_eq!(
        report.protocol,
        Protocol::Completed,
        "{:?} {:?}; {}",
        report.failure,
        report.runtime_observation(),
        crate::approval::diagnostics::last_cancellation_trace(&cap.dispatch_id)
    );
    assert!(report.failure.is_none());
    assert_eq!(host_response_frames(&work).len(), 1);
    assert_eq!(probe_read_frames(&work).len(), 1);
    let sql = rusqlite::Connection::open(root.path().join("state/domain.sqlite3")).unwrap();
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

/// The named first case (design §3b): a resolution before admission still
/// cancels with the exact variant, and nothing reaches the wire.
#[tokio::test]
async fn native_owned_approval_resolution_before_admission_cancels() {
    let root = tempfile::tempdir().unwrap();
    let work = root.path().join("work");
    hagency_store::private::directory(&work).unwrap();
    let work = work.canonicalize().unwrap();
    let (domain, cap) = fixture(root.path());
    // RecheckGate without an attached gate is inert: the take() finds None.
    let mut op = Operation::start(
        domain.clone(),
        cap.clone(),
        host(&work, Fault::RecheckGate, "owned-approval-resolve"),
        Limits {
            operation_ms: 25_000,
            response_ms: 1500,
        },
    )
    .unwrap();
    let mut notices = op.take_approval_requests().unwrap();
    let _notice = tokio::time::timeout(harness_wait() * 3, notices.recv())
        .await
        .unwrap()
        .unwrap();
    // Deliberately NO owner verdict: the probe resolves while the entry is
    // retained but not admitted, which is the pre-admission case.
    std::fs::write(work.join("owned-dispatch.approval-release"), b"release").unwrap();
    let report = op.wait().await.unwrap();
    assert_eq!(
        report.failure,
        Some(Failure::ApprovalCancelled),
        "{:?}; {}",
        report.runtime_observation(),
        crate::approval::diagnostics::last_cancellation_trace(&cap.dispatch_id)
    );
    assert!(!work.join("owned-dispatch.approval-bytes").exists());
    assert!(host_response_frames(&work).is_empty());
}

/// The re-entry guard (design §3c): after one written frame and its legal
/// post-write resolution, no second frame for that id is ever written — the
/// probe itself fails if one arrives.
#[tokio::test]
async fn native_owned_approval_no_second_frame_after_resolution() {
    let root = tempfile::tempdir().unwrap();
    let work = root.path().join("work");
    hagency_store::private::directory(&work).unwrap();
    let work = work.canonicalize().unwrap();
    let (domain, cap) = fixture(root.path());
    let mut op = Operation::start(
        domain.clone(),
        cap.clone(),
        host(
            &work,
            Fault::RecheckGate,
            "owned-approval-admitted-resolve-count",
        ),
        Limits {
            operation_ms: 25_000,
            response_ms: 1500,
        },
    )
    .unwrap();
    let mut notices = op.take_approval_requests().unwrap();
    let notice = tokio::time::timeout(harness_wait() * 3, notices.recv())
        .await
        .unwrap()
        .unwrap();
    choose(&domain, notice.request_id).await;
    let report = op.wait().await.unwrap();
    assert_eq!(
        report.protocol,
        Protocol::Completed,
        "{:?} {:?}; {}",
        report.failure,
        report.runtime_observation(),
        crate::approval::diagnostics::last_cancellation_trace(&cap.dispatch_id)
    );
    assert_eq!(host_response_frames(&work).len(), 1);
    assert_eq!(probe_read_frames(&work).len(), 1);
    let sql = rusqlite::Connection::open(root.path().join("state/domain.sqlite3")).unwrap();
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

/// The receipt-before-resolution ordering (design §4 scenario 1): the host is
/// held between the transport's write receipt and the acceptance observation
/// (`Fault::ReceiptGate`); the probe reads the frame, then resolves, so the
/// resolution can only be parsed after the receipt. The acceptance row must
/// exist and no transport failure may occur.
#[tokio::test]
async fn native_owned_approval_receipt_before_resolution() {
    use std::sync::{Arc, atomic::Ordering};
    let root = tempfile::tempdir().unwrap();
    let work = root.path().join("work");
    hagency_store::private::directory(&work).unwrap();
    let work = work.canonicalize().unwrap();
    let (domain, cap) = fixture(root.path());
    let gate = Arc::new(crate::approval::Gate::default());
    let mut configured = host(&work, Fault::ReceiptGate, "owned-approval");
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
    let notice = tokio::time::timeout(harness_wait() * 3, notices.recv())
        .await
        .unwrap()
        .unwrap();
    choose(&domain, notice.request_id).await;
    let end = tokio::time::Instant::now() + harness_wait();
    while !gate.entered.load(Ordering::Acquire) {
        assert!(
            tokio::time::Instant::now() < end,
            "write receipt did not reach the receipt gate; probe markers present: {}",
            markers_present(&work)
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    // The probe must have read and recorded the frame before the host's
    // acceptance observation runs; the gate does not pump the session, so the
    // resolution it emits stays unparsed across the hold.
    let bytes = work.join("owned-dispatch.approval-bytes");
    let end = tokio::time::Instant::now() + harness_wait();
    while !bytes.exists() {
        assert!(
            tokio::time::Instant::now() < end,
            "probe never recorded the written frame; probe markers present: {}",
            markers_present(&work)
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    gate.release.store(true, Ordering::Release);
    let report = op.wait().await.unwrap();
    assert_eq!(
        report.protocol,
        Protocol::Completed,
        "{:?} {:?}; {}",
        report.failure,
        report.runtime_observation(),
        crate::approval::diagnostics::last_cancellation_trace(&cap.dispatch_id)
    );
    let expected = if cfg!(target_os = "macos") {
        Some(Failure::CleanupUnknown)
    } else {
        None
    };
    assert_eq!(
        report.failure,
        expected,
        "{:?} {:?}; settlement={:?}",
        report.failure,
        report.runtime_observation(),
        report.settlement_cause
    );
    assert_eq!(host_response_frames(&work).len(), 1);
    assert_eq!(probe_read_frames(&work).len(), 1);
    if let Some(observation) = report.runtime_observation() {
        assert!(observation.transport_cause.is_none(), "{observation:?}");
    }
    let sql = rusqlite::Connection::open(root.path().join("state/domain.sqlite3")).unwrap();
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

/// F2's case (design §4 scenario 2): the resolution is emitted while the host
/// is held at the recheck gate — before the first byte of the frame. The
/// send-site guard must refuse the resolved frame with the named verdict,
/// never `Closed`, and no frame may reach the wire.
#[tokio::test]
async fn native_owned_approval_resolved_before_first_byte() {
    use std::sync::{Arc, atomic::Ordering};
    let root = tempfile::tempdir().unwrap();
    let work = root.path().join("work");
    hagency_store::private::directory(&work).unwrap();
    let work = work.canonicalize().unwrap();
    let (domain, cap) = fixture(root.path());
    let gate = Arc::new(crate::approval::Gate::default());
    let mut configured = host(&work, Fault::RecheckGate, "owned-approval-resolve-first");
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
    let notice = tokio::time::timeout(harness_wait() * 3, notices.recv())
        .await
        .unwrap()
        .unwrap();
    choose(&domain, notice.request_id).await;
    let end = tokio::time::Instant::now() + harness_wait();
    while !gate.entered.load(Ordering::Acquire) {
        assert!(
            tokio::time::Instant::now() < end,
            "original recheck did not reach gate; probe markers present: {}",
            markers_present(&work)
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    // The host is armed at the recheck gate; emit the resolution before the
    // first byte, confirm it is on the wire, then release the host.
    std::fs::write(work.join("owned-dispatch.approval-release"), b"release").unwrap();
    let resolving = work.join("owned-dispatch.approval-resolving");
    let end = tokio::time::Instant::now() + harness_wait();
    while !resolving.exists() {
        assert!(
            tokio::time::Instant::now() < end,
            "probe never emitted the pre-first-byte resolution; probe markers present: {}",
            markers_present(&work)
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    gate.release.store(true, Ordering::Release);
    let report = op.wait().await.unwrap();
    // The quiet path (Q2): a pre-send resolution completes the operation —
    // the same quiet completion the pre-admission resolution produces, never
    // a named failure (`ResponseUnavailable` is withdrawn) and never a
    // transport refusal (`Closed`).
    assert_eq!(
        report.protocol,
        Protocol::Completed,
        "{:?} {:?}; {}",
        report.failure,
        report.runtime_observation(),
        crate::approval::diagnostics::last_cancellation_trace(&cap.dispatch_id)
    );
    let expected = if cfg!(target_os = "macos") {
        Some(Failure::CleanupUnknown)
    } else {
        None
    };
    assert_eq!(
        report.failure,
        expected,
        "{:?} {:?}; {}",
        report.failure,
        report.runtime_observation(),
        crate::approval::diagnostics::last_cancellation_trace(&cap.dispatch_id)
    );
    if let Some(observation) = report.runtime_observation() {
        assert!(
            !matches!(
                observation.transport_cause,
                Some(hagency_runtime::codex::transport::Error::Closed)
            ),
            "{observation:?}"
        );
    }
    // No frame reached the wire and no acceptance row exists.
    assert!(host_response_frames(&work).is_empty());
    assert!(!work.join("owned-dispatch.approval-bytes").exists());
    let sql = rusqlite::Connection::open(root.path().join("state/domain.sqlite3")).unwrap();
    assert_eq!(
        sql.query_row(
            "SELECT COUNT(*) FROM approval_responses WHERE write_accepted=1",
            [],
            |row| row.get::<_, u64>(0)
        )
        .unwrap(),
        0
    );
}

/// A frame whose peer vanished before its first byte is never uncertain
/// (design Q4): the `owned-approval-eof` probe exits right after its
/// callback, so the armed response frame's first write fails with zero
/// bytes accepted. The verdict names the refusal (`PeerUnavailable`), the
/// observation carries the arm and `accepted_bytes == 0`, and no accepted
/// row is manufactured — distinct from both `Protocol` (malformed bytes)
/// and the reconcile's uncertainty (a written frame's lost acknowledgement).
#[tokio::test]
async fn native_owned_approval_peer_gone_before_first_byte() {
    let root = tempfile::tempdir().unwrap();
    let work = root.path().join("work");
    hagency_store::private::directory(&work).unwrap();
    let work = work.canonicalize().unwrap();
    let (domain, cap) = fixture(root.path());
    // RecheckGate without an attached gate is inert: the take() finds None.
    let mut op = Operation::start(
        domain.clone(),
        cap.clone(),
        host(&work, Fault::RecheckGate, "owned-approval-eof"),
        Limits {
            operation_ms: 25_000,
            response_ms: 1500,
        },
    )
    .unwrap();
    let mut notices = op.take_approval_requests().unwrap();
    let notice = tokio::time::timeout(harness_wait() * 3, notices.recv())
        .await
        .unwrap()
        .unwrap();
    choose(&domain, notice.request_id).await;
    let report = op.wait().await.unwrap();
    assert_eq!(
        report.failure,
        Some(Failure::PeerUnavailable),
        "{:?} {:?}; trace: {}; cancelled: {}",
        report.failure,
        report.runtime_observation(),
        hagency_execution::diagnostics::dispatch_trace(&cap.dispatch_id),
        hagency_execution::diagnostics::last_cancellation_trace(&cap.dispatch_id)
    );
    // The observation names the failing arm and proves zero accepted bytes.
    let observation = report.runtime_observation().expect("observation");
    assert!(
        matches!(
            observation.transport_cause,
            Some(hagency_runtime::codex::transport::Error::Io(_))
                | Some(hagency_runtime::codex::transport::Error::PeerEof)
        ),
        "{observation:?}"
    );
    let write = observation.write.expect("unconfirmed write snapshot");
    assert_eq!(write.accepted_bytes, 0, "{observation:?}");
    assert_eq!(write.total_bytes, 51, "{observation:?}");
    // Never transmitted: no frame reached the wire and no row was accepted.
    assert!(host_response_frames(&work).is_empty());
    assert!(!work.join("owned-dispatch.approval-bytes").exists());
    let sql = rusqlite::Connection::open(root.path().join("state/domain.sqlite3")).unwrap();
    assert_eq!(
        sql.query_row(
            "SELECT COUNT(*) FROM approval_responses WHERE write_accepted=1",
            [],
            |row| row.get::<_, u64>(0)
        )
        .unwrap(),
        0
    );
    assert_eq!(
        sql.query_row(
            "SELECT COUNT(*) FROM owner_approvals WHERE state='applied'",
            [],
            |row| row.get::<_, u64>(0)
        )
        .unwrap(),
        0
    );
}

/// The final verdict's rule, untransmitted arm (design Q3/Q4): the probe ends
/// the turn while the host is held at the recheck gate — the entry is in
/// flight, the frame armed, not one byte on the wire. The turn end must never
/// end the operation silently: with zero accepted bytes the verdict is
/// `PeerUnavailable` (never transmitted), never `Completed`.
#[tokio::test]
async fn native_owned_approval_turn_end_untransmitted() {
    use std::sync::{Arc, atomic::Ordering};
    let root = tempfile::tempdir().unwrap();
    let work = root.path().join("work");
    hagency_store::private::directory(&work).unwrap();
    let work = work.canonicalize().unwrap();
    let (domain, cap) = fixture(root.path());
    let gate = Arc::new(crate::approval::Gate::default());
    let mut configured = host(
        &work,
        Fault::RecheckGate,
        "owned-approval-turn-untransmitted",
    );
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
    let notice = tokio::time::timeout(harness_wait() * 3, notices.recv())
        .await
        .unwrap()
        .unwrap();
    choose(&domain, notice.request_id).await;
    // The host reaches the recheck gate: in flight, armed, zero bytes.
    let end = tokio::time::Instant::now() + harness_wait();
    while !gate.entered.load(Ordering::Acquire) {
        assert!(
            tokio::time::Instant::now() < end,
            "original recheck did not reach gate; probe markers present: {}",
            markers_present(&work)
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    // Let the probe end its turn and exit while the host is still held.
    std::fs::write(work.join("owned-dispatch.approval-release"), b"release").unwrap();
    let resolving = work.join("owned-dispatch.approval-resolving");
    let end = tokio::time::Instant::now() + harness_wait();
    while !resolving.exists() {
        assert!(
            tokio::time::Instant::now() < end,
            "probe never emitted the turn end; probe markers present: {}",
            markers_present(&work)
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    gate.release.store(true, Ordering::Release);
    let report = op.wait().await.unwrap();
    assert_ne!(
        report.protocol,
        Protocol::Completed,
        "{:?} {:?}; trace: {}; cancelled: {}",
        report.failure,
        report.runtime_observation(),
        hagency_execution::diagnostics::dispatch_trace(&cap.dispatch_id),
        hagency_execution::diagnostics::last_cancellation_trace(&cap.dispatch_id)
    );
    assert_eq!(
        report.failure,
        Some(Failure::PeerUnavailable),
        "{:?} {:?}; trace: {}",
        report.failure,
        report.runtime_observation(),
        hagency_execution::diagnostics::dispatch_trace(&cap.dispatch_id)
    );
    let trace = hagency_execution::diagnostics::dispatch_trace(&cap.dispatch_id);
    assert!(
        trace.contains("turn-ended-in-flight-untransmitted"),
        "untransmitted arm never stamped; trace: {trace}"
    );
    // Never transmitted: no frame, no accepted row.
    assert!(host_response_frames(&work).is_empty());
    let sql = rusqlite::Connection::open(root.path().join("state/domain.sqlite3")).unwrap();
    assert_eq!(
        sql.query_row(
            "SELECT COUNT(*) FROM approval_responses WHERE write_accepted=1",
            [],
            |row| row.get::<_, u64>(0)
        )
        .unwrap(),
        0
    );
}

/// The final verdict's rule, uncertain arm: the probe stays silent while the
/// whole frame is accepted, then ends the turn and exits with the frame
/// unread. The fate of a transmitted, receipt-less frame is unknown — the
/// operation must never complete silently. On Windows the flush fails
/// mid-write (bytes accepted, uncertain arm); on POSIX the atomic 51-byte
/// write completes and the turn end reaches a written, unrecorded entry (the
/// reconcile's `SettlementUnknown`). Both arms assert the same verdict:
/// `SettlementUnknown`, never `Completed`.
#[tokio::test]
async fn native_owned_approval_turn_end_midwrite_uncertain() {
    let root = tempfile::tempdir().unwrap();
    let work = root.path().join("work");
    hagency_store::private::directory(&work).unwrap();
    let work = work.canonicalize().unwrap();
    let (domain, cap) = fixture(root.path());
    // RecheckGate without an attached gate is inert: the take() finds None.
    let mut op = Operation::start(
        domain.clone(),
        cap.clone(),
        host(&work, Fault::RecheckGate, "owned-approval-turn-midwrite"),
        Limits {
            operation_ms: 25_000,
            response_ms: 1500,
        },
    )
    .unwrap();
    let mut notices = op.take_approval_requests().unwrap();
    let notice = tokio::time::timeout(harness_wait() * 3, notices.recv())
        .await
        .unwrap()
        .unwrap();
    choose(&domain, notice.request_id).await;
    // The probe exits on its own schedule (silent 1.2 s, turn end, exit
    // unread); wait for its handshake marker before claiming the report.
    let midwrite = work.join("owned-dispatch.approval-midwrite-exit");
    let end = tokio::time::Instant::now() + harness_wait();
    while !midwrite.exists() {
        assert!(
            tokio::time::Instant::now() < end,
            "probe never reached its midwrite exit; probe markers present: {}",
            markers_present(&work)
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let report = op.wait().await.unwrap();
    assert_ne!(
        report.protocol,
        Protocol::Completed,
        "{:?} {:?}; trace: {}",
        report.failure,
        report.runtime_observation(),
        hagency_execution::diagnostics::dispatch_trace(&cap.dispatch_id)
    );
    assert_eq!(
        report.failure,
        Some(Failure::SettlementUnknown),
        "{:?} {:?}; trace: {}",
        report.failure,
        report.runtime_observation(),
        hagency_execution::diagnostics::dispatch_trace(&cap.dispatch_id)
    );
    let trace = hagency_execution::diagnostics::dispatch_trace(&cap.dispatch_id);
    // One of the two uncertainty arms must name the fate — the mid-write
    // send refusal or the reconcile's unrecorded acceptance; which one is
    // platform timing, being-uncertain is not.
    assert!(
        trace.contains("turn-ended-in-flight-uncertain")
            || trace.contains("write-flushed")
            || trace.contains("settlement"),
        "no uncertainty arm stamped; trace: {trace}"
    );
}
