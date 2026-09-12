use crate::{
    DomainRepository, Effect, EffectOutcome, Error, ShutdownOutcome, ShutdownSnapshot,
    shutdown::{Phase, Probe, mark},
};
use hagency_core::approvals::{
    ApprovalIntakeTarget, ApprovalRoomAuthority, ApprovalRoomCapture, ApprovalSummary,
    ApprovalVerdictInput,
};
use hagency_core::{
    allocation::Budget,
    authority::{Registration, VerifiedRequest},
    messages::{InboundMessage, InboxItem, MessageReceipt, MessageTarget},
    project::{CatalogResource, ConfiguredResource, Engagement, Resource, Seat},
    replies::*,
    task_intents::{IntentResult, NoticeClaim, NoticeDelivery, TaskIntent},
    tasks::{
        DispatchInput, MutationResult, RunnerCapability, RunnerCommand, SessionBinding, Task,
        TaskComment, TaskEvent, TaskMutation,
    },
};
use serde::Serialize;
use std::{
    sync::{
        Arc,
        atomic::{AtomicU8, AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc, oneshot};

#[cfg(test)]
#[path = "../tests/common/mod.rs"]
pub(crate) mod clock_fixtures;

type Operation = Box<dyn FnOnce(&mut DomainRepository) + Send>;
enum ReceiverPolicy {
    CancelIfDropped,
    RetainEnqueuedInvalidation,
}

#[cfg(test)]
#[path = "../tests/matrix_invalidation/worker.rs"]
mod matrix_invalidation_tests;

enum Job {
    Run {
        operation: Operation,
        _bytes: OwnedSemaphorePermit,
    },
    Shutdown {
        reply: oneshot::Sender<()>,
        probe: Option<Arc<Probe>>,
    },
}

#[cfg(test)]
mod shutdown_tests {
    use super::*;

    async fn held_field_drop(phase: Phase) {
        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("state");
        let store = DomainStore::start(DomainRepository::open(&state).unwrap(), 1).unwrap();
        let (probe, reached, resume) = Probe::paused(phase);
        let attempt = {
            let store = store.clone();
            let probe = probe.clone();
            tokio::spawn(async move { store.shutdown_tracked(Some(probe)).await })
        };
        tokio::time::timeout(Duration::from_secs(2), reached)
            .await
            .unwrap()
            .unwrap();
        let (result, outcome) = attempt.await.unwrap();
        assert!(matches!(result, Err(Error::OutcomeUnknown)));
        assert_eq!(outcome, ShutdownOutcome::ReplyTimedOut);
        let snapshot = probe.snapshot(outcome);
        assert!(snapshot.connection_drop_started_us.is_some());
        #[cfg(windows)]
        {
            let caller = Probe::new();
            caller.observe_domain_writer();
            let crate::NativeWriterObservation::Measured {
                process_id: caller_pid,
                thread_id: caller_tid,
                ..
            } = caller.snapshot(outcome).native_writer
            else {
                panic!("actual Windows caller query unavailable");
            };
            assert!(matches!(snapshot.native_writer,
                crate::NativeWriterObservation::Measured { process_id, thread_id, .. }
                    if process_id == caller_pid && thread_id != caller_tid));
        }
        #[cfg(not(windows))]
        assert_eq!(
            snapshot.native_writer,
            crate::NativeWriterObservation::Unsupported
        );
        assert_eq!(snapshot.ownership_drop_finished_us, None);
        assert_eq!(snapshot.drop_finished_us, None);
        assert_eq!(snapshot.acknowledgement_started_us, None);
        if phase == Phase::ConnectionDropStarted {
            assert_eq!(snapshot.sqlite_close_entered_us, None);
            assert_eq!(snapshot.connection_drop_finished_us, None);
            assert_eq!(snapshot.ownership_drop_started_us, None);
        } else if phase == Phase::SqliteCloseEntered {
            assert!(snapshot.sqlite_close_entered_us.is_some());
            assert_eq!(snapshot.connection_drop_finished_us, None);
            assert_eq!(snapshot.ownership_drop_started_us, None);
        } else {
            assert!(snapshot.sqlite_close_entered_us.is_some());
            assert!(snapshot.connection_drop_finished_us.is_some());
            assert!(snapshot.ownership_drop_started_us.is_some());
        }
        // The actual original ownership file remains locked even after the
        // connection has dropped. This is not inferred from a phase alone.
        assert!(matches!(DomainRepository::open(&state), Err(Error::Locked)));
        resume.send(()).unwrap();
        closed(&store).await;
        let reopened = DomainRepository::open(&state).unwrap();
        drop(reopened);
        let later = probe.snapshot(outcome);
        assert_eq!(later.native_writer, snapshot.native_writer);
        let ordered = [
            later.worker_picked_up_us,
            later.drop_started_us,
            later.connection_drop_started_us,
            later.sqlite_close_entered_us,
            later.connection_drop_finished_us,
            later.ownership_drop_started_us,
            later.ownership_drop_finished_us,
            later.drop_finished_us,
            later.acknowledgement_started_us,
        ]
        .map(Option::unwrap);
        assert!(ordered.windows(2).all(|pair| pair[0] <= pair[1]));
        assert_eq!(later.acknowledgement_sent_us, None);
        assert_eq!(snapshot.ownership_drop_finished_us, None);
        assert_eq!(snapshot.outcome, ShutdownOutcome::ReplyTimedOut);
    }

    #[tokio::test]
    async fn native_domain_shutdown_connection_drop() {
        held_field_drop(Phase::ConnectionDropStarted).await;
    }

    #[tokio::test]
    async fn native_domain_shutdown_ownership_drop() {
        held_field_drop(Phase::OwnershipDropStarted).await;
    }

    #[tokio::test]
    async fn native_domain_shutdown_sqlite_close_entry() {
        held_field_drop(Phase::SqliteCloseEntered).await;
    }

    async fn closed(store: &DomainStore) {
        tokio::time::timeout(Duration::from_secs(2), store.tx.closed())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn native_domain_shutdown_queue() {
        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("state");
        let store = DomainStore::start(DomainRepository::open(&state).unwrap(), 1).unwrap();
        let (entered, reached) = oneshot::channel();
        let (resume, paused) = std::sync::mpsc::channel();
        store
            .tx
            .try_send(Job::Run {
                operation: Box::new(move |_| {
                    let _ = entered.send(());
                    let _ = paused.recv_timeout(Duration::from_secs(6));
                }),
                _bytes: store.bytes.clone().try_acquire_owned().unwrap(),
            })
            .unwrap_or_else(|_| panic!("test operation was not admitted"));
        tokio::time::timeout(Duration::from_secs(2), reached)
            .await
            .unwrap()
            .unwrap();
        let (result, snapshot) = store.shutdown_observed().await;
        assert!(matches!(result, Err(Error::OutcomeUnknown)));
        assert_eq!(snapshot.outcome, ShutdownOutcome::ReplyTimedOut);
        assert!(snapshot.enqueue_observed_us.is_some());
        assert!(snapshot.caller_finished_us.is_some());
        assert_eq!(snapshot.worker_picked_up_us, None);
        assert_eq!(snapshot.drop_started_us, None);
        assert_eq!(snapshot.sqlite_close_entered_us, None);
        assert_eq!(
            snapshot.native_writer,
            crate::NativeWriterObservation::Unobserved
        );
        assert!(matches!(DomainRepository::open(&state), Err(Error::Locked)));
        resume.send(()).unwrap();
        // Observe cleanup of the ORIGINAL queued job, never retry shutdown to
        // turn its unknown verdict into success.
        closed(&store).await;
        DomainRepository::open(&state).unwrap();

        // A queue with no receiving worker exercises the unchanged enqueue
        // deadline separately, without attributing any repository outcome.
        let (tx, rx) = mpsc::channel(1);
        let full = DomainStore {
            tx,
            bytes: Arc::new(Semaphore::new(1)),
            progress: Arc::new(Progress::default()),
        };
        let (reply, _) = oneshot::channel();
        full.tx
            .try_send(Job::Shutdown { reply, probe: None })
            .unwrap_or_else(|_| panic!("test queue was not filled"));
        let (result, snapshot) = full.shutdown_observed().await;
        assert!(matches!(result, Err(Error::OutcomeUnknown)));
        assert_eq!(snapshot.outcome, ShutdownOutcome::EnqueueTimedOut);
        assert_eq!(snapshot.enqueue_observed_us, None);
        assert_eq!(snapshot.worker_picked_up_us, None);
        drop(rx);
        assert_eq!(
            snapshot.native_writer,
            crate::NativeWriterObservation::Unobserved
        );
    }

    /// The two `OutcomeUnknown` cases must be distinguishable: a command the
    /// writer had begun versus one still queued when the reply bound expired
    /// (accepted design §1.4). Neither is a retryable success; the bit is a
    /// diagnosis only. Note the tail: `shutdown()` drains the queue and
    /// closes the writer (no `Shutdown` was enqueued otherwise, so a bare
    /// channel-close wait could not resolve).
    #[tokio::test]
    async fn native_domain_unknown_reports_dequeue() {
        // Case 1: the writer is parked inside an earlier job, so this command
        // is still queued when its two-second reply bound expires.
        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("state");
        let store = DomainStore::start(DomainRepository::open(&state).unwrap(), 2).unwrap();
        let (entered, reached) = oneshot::channel();
        let (resume, paused) = std::sync::mpsc::channel::<()>();
        store
            .tx
            .try_send(Job::Run {
                operation: Box::new(move |_| {
                    let _ = entered.send(());
                    let _ = paused.recv_timeout(Duration::from_secs(6));
                }),
                _bytes: store.bytes.clone().try_acquire_owned().unwrap(),
            })
            .unwrap_or_else(|_| panic!("parking job was not admitted"));
        tokio::time::timeout(Duration::from_secs(2), reached)
            .await
            .unwrap()
            .unwrap();
        let queued = store
            .call_with_policy(1, ReceiverPolicy::CancelIfDropped, |_| Ok(()))
            .await;
        assert!(matches!(queued, Err(Error::OutcomeUnknown)));
        assert_eq!(
            store.last_unknown_dequeued(),
            Some(false),
            "a command behind a parked writer must not be reported as dequeued"
        );
        // The command is NOT withdrawn: the writer runs it once released, so
        // the outcome genuinely remains unknown rather than "not executed".
        resume.send(()).unwrap();
        store.shutdown().await.unwrap();

        // Case 2: the writer begins this command and it outlives the bound.
        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("state");
        let store = DomainStore::start(DomainRepository::open(&state).unwrap(), 1).unwrap();
        let running = store
            .call_with_policy(1, ReceiverPolicy::CancelIfDropped, |_| {
                std::thread::sleep(Duration::from_secs(3));
                Ok(())
            })
            .await;
        assert!(matches!(running, Err(Error::OutcomeUnknown)));
        assert_eq!(
            store.last_unknown_dequeued(),
            Some(true),
            "a command the writer had begun must be reported as dequeued"
        );
        store.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn native_domain_shutdown_phases() {
        for phase in [Phase::DropStarted, Phase::AcknowledgementStarted] {
            let root = tempfile::tempdir().unwrap();
            let state = root.path().join("state");
            let store = DomainStore::start(DomainRepository::open(&state).unwrap(), 1).unwrap();
            let (probe, reached, resume) = Probe::paused(phase);
            let attempt = {
                let store = store.clone();
                let probe = probe.clone();
                tokio::spawn(async move { store.shutdown_tracked(Some(probe)).await })
            };
            tokio::time::timeout(Duration::from_secs(2), reached)
                .await
                .unwrap()
                .unwrap();
            let (result, outcome) = attempt.await.unwrap();
            let snapshot = probe.snapshot(outcome);
            assert!(matches!(result, Err(Error::OutcomeUnknown)));
            assert_eq!(outcome, ShutdownOutcome::ReplyTimedOut);
            assert!(snapshot.worker_picked_up_us.is_some());
            assert!(snapshot.drop_started_us.is_some());
            assert_eq!(snapshot.acknowledgement_sent_us, None);
            if phase == Phase::DropStarted {
                assert_eq!(snapshot.drop_finished_us, None);
                assert_eq!(snapshot.acknowledgement_started_us, None);
                assert!(matches!(DomainRepository::open(&state), Err(Error::Locked)));
            } else {
                assert!(snapshot.drop_finished_us.is_some());
                assert!(snapshot.acknowledgement_started_us.is_some());
            }
            resume.send(()).unwrap();
            closed(&store).await;
            DomainRepository::open(&state).unwrap();
            // The original caller timed out: sending to its dropped receiver
            // cannot become an observed successful acknowledgement afterward.
            assert_eq!(probe.snapshot(outcome).acknowledgement_sent_us, None);
            assert_eq!(snapshot.outcome, ShutdownOutcome::ReplyTimedOut);
        }
    }
}

#[cfg(test)]
mod clock_tests {
    use super::*;
    use clock_fixtures::{proof, registration, request, resource};
    use serde_json::json;
    fn now() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    }

    fn owned_fixture(root: &std::path::Path) -> (DomainRepository, RunnerCapability) {
        owned_fixture_with_attachment(root, false)
    }
    fn owned_fixture_with_attachment(
        root: &std::path::Path,
        attachment: bool,
    ) -> (DomainRepository, RunnerCapability) {
        let mut db = DomainRepository::open(&root.join("state")).unwrap();
        db.register(&registration()).unwrap();
        let pool = resource("pool", "seat", 100);
        db.put_resource(&pool).unwrap();
        let proof = proof(&request("request", "Worker", &pool, 10));
        let e = db.admit(&proof, 1000).unwrap();
        db.approve("approve", &proof, 1000).unwrap();
        let effect = db.claim_effect().unwrap().unwrap();
        db.observe_effect(
            &effect.id,
            effect.fence,
            &EffectOutcome::Applied {
                receipt: "offline".into(),
            },
        )
        .unwrap();
        db.observe_matrix_transport(
            &hagency_core::replies::MatrixTransportObservation {
                engagement_id: e.id.clone(),
                registration_generation: 1,
                generation: 1,
                sender_mxid: "@worker:example.test".into(),
                device_id: "DEVICE".into(),
            },
            now(),
        )
        .unwrap();
        db.observe_matrix_room(
            &hagency_core::replies::MatrixRoomObservation {
                engagement_id: e.id.clone(),
                registration_generation: 1,
                transport_generation: 1,
                room_id: "!project:example.test".into(),
                generation: 1,
                privacy: hagency_core::replies::RoomPrivacy::Group {},
                joined: std::collections::BTreeSet::from([
                    "@worker:example.test".into(),
                    "@owner:example.test".into(),
                ]),
                invite_only: true,
                encrypted: attachment,
            },
            now(),
        )
        .unwrap();
        db.resolve_verified_matrix_session(
            &SessionBinding {
                id: "session".into(),
                engagement_id: e.id,
                room_id: "!project:example.test".into(),
                thread_root: None,
            },
            now(),
        )
        .unwrap();
        db.register_workspace("work").unwrap();
        db.create_canonical_task("task", "session", "Receipt loss", now())
            .unwrap();
        let input = DispatchInput {
            id: "dispatch".into(),
            session_id: "session".into(),
            task_id: Some("task".into()),
            resources: vec![hagency_core::tasks::ResourceLease {
                id: "work".into(),
                exclusive: true,
            }],
            payload: json!({"instruction":"offline"}),
        };
        if attachment {
            use hagency_core::{attachments::*, ingress::MatrixEventObservation};
            let event = MatrixAttachmentObservation {
                event: MatrixEventObservation {
                    scope: db.matrix_ingress_scope("session").unwrap(),
                    event: InboundMessage {
                        server_name: "example.test".into(),
                        room_id: "!project:example.test".into(),
                        event_id: "$receive".into(),
                        sender_mxid: "@owner:example.test".into(),
                        thread_root: None,
                        body: "untrusted file".into(),
                        kind: "m.file".into(),
                        origin_ts: now(),
                    },
                    mentions: std::collections::BTreeSet::from(["@worker:example.test".into()]),
                    encrypted: true,
                },
                metadata: AttachmentMetadata {
                    filename: "input.bin".into(),
                    mime_type: None,
                    declared_size: Some(3),
                },
                sdk_identity: "1".repeat(64),
                manifest_id: "2".repeat(64),
                content_digest: "3".repeat(64),
            };
            let receipt = db.admit_matrix_attachment(&event, now()).unwrap();
            db.enqueue_inbox_dispatch(&input, &[receipt.sequence])
                .unwrap();
        } else {
            db.enqueue_dispatch(&input).unwrap();
        }
        let cap = db
            .claim_dispatch("host", now(), 60_000, 60_000, 1)
            .unwrap()
            .unwrap();
        (db, cap)
    }

    pub(super) fn approval_fixture(
        root: &std::path::Path,
    ) -> (
        DomainRepository,
        RunnerCapability,
        hagency_core::approvals::HostApprovalContext,
    ) {
        approval_fixture_mode(root, true)
    }
    pub(super) fn approval_fixture_mode(
        root: &std::path::Path,
        started: bool,
    ) -> (
        DomainRepository,
        RunnerCapability,
        hagency_core::approvals::HostApprovalContext,
    ) {
        use hagency_core::approvals::*;
        let (mut db, cap) = owned_fixture(root);
        if started {
            db.start_dispatch(&cap, now()).unwrap();
        }
        let inspect = rusqlite::Connection::open(root.join("state/domain.sqlite3")).unwrap();
        let engagement_id: String = inspect
            .query_row(
                "SELECT engagement_id FROM runner_sessions WHERE id='session'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        db.observe_approval_room(
            &ApprovalRoomObservation {
                engagement_id,
                registration_generation: 1,
                generation: 1,
                room_id: "!private:example.test".into(),
                device_id: "APPROVAL_DEVICE".into(),
                joined: std::collections::BTreeSet::from([
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
        let context = HostApprovalContext {
            id: "clock_context".into(),
            connection_id: "clock_connection".into(),
            thread_id: "thread".into(),
            turn_id: "turn".into(),
            workspace_resource: "work".into(),
            workspace: "/work/clock".into(),
            windows_paths: false,
            environment_id: None,
            may_write: true,
            yolo: false,
        };
        (db, cap, context)
    }

    pub(super) fn approval_input(expires_at: u64) -> hagency_core::approvals::HostApprovalRequest {
        use hagency_core::approvals::*;
        HostApprovalRequest {
            context_id: "clock_context".into(),
            upstream_id: ApprovalRpcId::Number(1),
            item_id: "item".into(),
            method: "item/commandExecution/requestApproval".into(),
            params: json!({"threadId":"thread","turnId":"turn","itemId":"item","command":"touch result","cwd":"/work/clock"}),
            expires_at,
        }
    }

    #[test]
    fn native_approval_transaction_clock_owned() {
        use hagency_core::approvals::*;
        let root = tempfile::tempdir().unwrap();
        let (mut db, cap, context) = approval_fixture(root.path());
        let inspect = rusqlite::Connection::open(root.path().join("state/domain.sqlite3")).unwrap();
        inspect.busy_timeout(Duration::ZERO).unwrap();
        // Every production closure is invoked with the original transaction
        // physically excluding another Immediate writer on this exact database.
        let sampled = std::cell::Cell::new(0);
        let clock = || {
            let error = inspect.execute_batch("BEGIN IMMEDIATE").unwrap_err();
            assert_eq!(
                error.sqlite_error_code(),
                Some(rusqlite::ErrorCode::DatabaseBusy)
            );
            sampled.set(sampled.get() + 1);
            Ok(now())
        };
        db.bind_approval_context_clock(&cap, &context, clock)
            .unwrap();
        let pending = db
            .request_owner_approval_clock(&cap, &approval_input(now() + 60_000), clock)
            .unwrap();
        let card = db.private_approval(&pending.id, now()).unwrap();
        let verdict = OwnerVerdictObservation {
            request_id: pending.id.clone(),
            request_digest: card.digest,
            binding_generation: card.binding_generation,
            server_name: "example.test".into(),
            room_id: card.room_id,
            sender_mxid: card.owner_mxid,
            event_id: "$clock".into(),
            encrypted: true,
            choice: ApprovalChoice::Once,
        };
        let sdk = ApprovalVerdictInput {
            target: db.approval_intake_target(&pending.id, now()).unwrap(),
            verdict: verdict.clone(),
            source_digest: "a".repeat(64),
        };
        db.observe_owner_verdict_clock(&verdict, clock).unwrap();
        // SDK admission still acquires the physical transaction before its
        // current-target check rejects this already-decided request.
        assert!(matches!(
            db.admit_approval_verdict_clock(&sdk, clock),
            Err(Error::RunnerAuthority)
        ));
        let application = db
            .consume_owner_approval_clock(&cap, &pending.id, clock)
            .unwrap();
        db.observe_approval_application_clock(
            &ApprovalApplicationObservation {
                application,
                outcome: ApplicationOutcome::Unknown,
                evidence: "original owner cannot establish application".into(),
            },
            clock,
        )
        .unwrap();
        assert_eq!(sampled.get(), 6);
        // No transaction remains held after any success or refusal.
        inspect.execute_batch("BEGIN IMMEDIATE; ROLLBACK").unwrap();
    }

    #[tokio::test]
    async fn native_approval_clock_after_queue() {
        let root = tempfile::tempdir().unwrap();
        let (mut db, cap, context) = approval_fixture(root.path());
        db.bind_approval_context(&cap, &context, now()).unwrap();
        let store = DomainStore::start(db, 16).unwrap();
        let (entered, ready) = oneshot::channel();
        let (release, gate) = std::sync::mpsc::channel();
        let blocking = store.clone();
        let blocker = tokio::spawn(async move {
            blocking
                .call(1, move |_| {
                    let _ = entered.send(());
                    gate.recv_timeout(Duration::from_secs(1))
                        .map_err(|_| Error::Unavailable)?;
                    Ok(())
                })
                .await
        });
        ready.await.unwrap();
        let deadline = now() + 150;
        let input = approval_input(deadline);
        let queued = store.request_owner_approval(cap, input);
        tokio::pin!(queued);
        assert!(
            tokio::time::timeout(Duration::from_millis(20), &mut queued)
                .await
                .is_err()
        );
        assert_eq!(
            store.tx.capacity(),
            15,
            "actual bounded queue retains this command"
        );
        assert!(now() < deadline);
        tokio::time::sleep(Duration::from_millis(deadline.saturating_sub(now()) + 20)).await;
        release.send(()).unwrap();
        blocker.await.unwrap().unwrap();
        assert!(matches!(queued.await, Err(Error::RunnerAuthority)));
        let inspect = rusqlite::Connection::open(root.path().join("state/domain.sqlite3")).unwrap();
        let requests: u64 = inspect
            .query_row("SELECT COUNT(*) FROM owner_approvals", [], |r| r.get(0))
            .unwrap();
        assert_eq!(requests, 0);
        store.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn native_owned_claim_profile_after_queue() {
        use hagency_core::replies::*;
        let root = tempfile::tempdir().unwrap();
        let (mut db, _original) = owned_fixture(root.path());
        let inspection =
            rusqlite::Connection::open(root.path().join("state/domain.sqlite3")).unwrap();
        let engagement: String = inspection
            .query_row(
                "SELECT engagement_id FROM runner_sessions WHERE id='session'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        drop(inspection);
        db.observe_matrix_room(
            &MatrixRoomObservation {
                engagement_id: engagement.clone(),
                registration_generation: 1,
                transport_generation: 1,
                room_id: "!project:example.test".into(),
                generation: 2,
                privacy: RoomPrivacy::Group {},
                joined: std::collections::BTreeSet::from([
                    "@worker:example.test".into(),
                    "@owner:example.test".into(),
                ]),
                invite_only: true,
                encrypted: true,
            },
            now(),
        )
        .unwrap();
        db.resolve_verified_matrix_session(
            &SessionBinding {
                id: "fresh".into(),
                engagement_id: engagement.clone(),
                room_id: "!project:example.test".into(),
                thread_root: Some("$fresh".into()),
            },
            now(),
        )
        .unwrap();
        db.create_canonical_task("fresh_task", "fresh", "Queued host claim", now())
            .unwrap();
        db.register_workspace("fresh_work").unwrap();
        db.enqueue_dispatch(&DispatchInput {
            id: "fresh_dispatch".into(),
            session_id: "fresh".into(),
            task_id: Some("fresh_task".into()),
            resources: vec![hagency_core::tasks::ResourceLease {
                id: "fresh_work".into(),
                exclusive: true,
            }],
            payload: json!({"instruction":"offline"}),
        })
        .unwrap();
        let profile = crate::OwnedClaimProfile::new(
            MatrixTransportObservation {
                engagement_id: engagement,
                registration_generation: 1,
                generation: 1,
                sender_mxid: "@worker:example.test".into(),
                device_id: "DEVICE".into(),
            },
            vec![
                crate::OwnedClaimRoom::new(
                    "!project:example.test".into(),
                    2,
                    RoomPrivacy::Group {},
                )
                .unwrap(),
            ],
            vec!["fresh_work".into()],
        )
        .unwrap();
        let store = DomainStore::start(db, 16).unwrap();
        let (entered, ready) = oneshot::channel();
        let (release, gate) = std::sync::mpsc::channel();
        let worker = store.clone();
        let hold = tokio::spawn(async move {
            worker
                .call(1, move |_| {
                    let _ = entered.send(());
                    gate.recv_timeout(Duration::from_secs(4))
                        .map_err(|_| Error::Unavailable)?;
                    Ok(())
                })
                .await
        });
        ready.await.unwrap();
        let mut claim = Box::pin(store.claim_owned_dispatch_for_host(
            profile,
            "queued_host".into(),
            60_000,
            60_000,
            2,
        ));
        std::future::poll_fn(|cx| {
            assert!(std::future::Future::poll(claim.as_mut(), cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        let before = now();
        // Advance one real UTC tick while the actual writer stays held. This
        // separates pre-enqueue timestamps without a deadline-sensitive sleep.
        let bound = tokio::time::Instant::now() + Duration::from_secs(1);
        while now() == before {
            assert!(tokio::time::Instant::now() < bound);
            tokio::task::yield_now().await;
        }
        let released_at = now();
        release.send(()).unwrap();
        hold.await.unwrap().unwrap();
        let cap = claim.await.unwrap().unwrap();
        assert_eq!(cap.dispatch_id, "fresh_dispatch");
        let inspect = rusqlite::Connection::open(root.path().join("state/domain.sqlite3")).unwrap();
        let observed: u64 = inspect
            .query_row(
                "SELECT created_at FROM runner_attempts WHERE dispatch_id='fresh_dispatch'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            observed >= released_at,
            "claim used a timestamp from before its queue wait"
        );
        drop(inspect);
        store.shutdown().await.unwrap();
    }

    fn usage_snapshot() -> hagency_metering::observation::UsageObservation {
        hagency_metering::observation::UsageObservation::parse(
            hagency_metering::Framework::Codex,
            r#"{"payload":{"info":{"total_token_usage":{"input_tokens":10,"output_tokens":2,"cached_input_tokens":3,"reasoning_output_tokens":0,"total_tokens":12}}}}"#,
        )
        .unwrap()
    }

    #[tokio::test]
    async fn native_usage_worker_after_lock() {
        let root = tempfile::tempdir().unwrap();
        let (mut db, cap) = owned_fixture(root.path());
        let admission = db.owned_dispatch_scope(&cap, now()).unwrap();
        let started = db
            .start_owned_dispatch(&cap, admission.fingerprint(), now())
            .unwrap();
        let source = db.bind_usage_source(&cap, &started, now()).unwrap();
        let store = DomainStore::start(db, 16).unwrap();
        let lock = rusqlite::Connection::open(root.path().join("state/domain.sqlite3")).unwrap();
        lock.execute_batch("BEGIN IMMEDIATE").unwrap();
        let mut record = Box::pin(store.record_usage_observation(
            source.clone(),
            "after_lock".into(),
            usage_snapshot(),
        ));
        std::future::poll_fn(|cx| {
            assert!(std::future::Future::poll(record.as_mut(), cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await; // The real command is now queued; writer must still acquire SQLite.
        tokio::time::sleep(Duration::from_millis(30)).await;
        let released_at = now();
        lock.execute_batch("COMMIT").unwrap();
        let receipt = record.await.unwrap();
        assert!(receipt.observed_at >= released_at);
        assert!(!receipt.replayed);
        assert_eq!(store.usage_source(source).await.unwrap().observations, 1);
        drop(lock);
        store.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn native_usage_worker_receipt_loss() {
        // A real writer commit may outlive its caller. The private test gate
        // withholds only acknowledgement and never manufactures durable success.
        for committed in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let (mut db, cap) = owned_fixture(root.path());
            let admission = db.owned_dispatch_scope(&cap, now()).unwrap();
            let started = db
                .start_owned_dispatch(&cap, admission.fingerprint(), now())
                .unwrap();
            let source = db.bind_usage_source(&cap, &started, now()).unwrap();
            let store = DomainStore::start(db, 16).unwrap();
            let (entered, ready) = oneshot::channel();
            let (release, gate) = std::sync::mpsc::channel();
            let worker = store.clone();
            let bound = source.clone();
            let withheld = tokio::spawn(async move {
                worker
                    .call(1, move |db| {
                        if committed {
                            db.record_usage_observation(
                                &bound,
                                "snapshot",
                                &usage_snapshot(),
                                now(),
                            )?;
                        }
                        let _ = entered.send(());
                        gate.recv_timeout(Duration::from_secs(4))
                            .map_err(|_| Error::Unavailable)?;
                        Ok(())
                    })
                    .await
            });
            ready.await.unwrap();
            if !committed {
                assert!(matches!(
                    store
                        .record_usage_observation(
                            source.clone(),
                            "snapshot".into(),
                            usage_snapshot()
                        )
                        .await,
                    Err(Error::OutcomeUnknown)
                ));
            }
            assert!(matches!(
                withheld.await.unwrap(),
                Err(Error::OutcomeUnknown)
            ));
            release.send(()).unwrap();
            let before = store.usage_source(source.clone()).await.unwrap();
            assert_eq!(before.observations, u64::from(committed));
            let replay = store
                .record_usage_observation(source.clone(), "snapshot".into(), usage_snapshot())
                .await
                .unwrap();
            assert_eq!(replay.replayed, committed);
            let view = store.usage_source(source).await.unwrap();
            assert_eq!(view.observations, 1);
            assert_eq!(view.high_water.input, Some(7));
            store.shutdown().await.unwrap();
            let reopened = DomainRepository::open(&root.path().join("state")).unwrap();
            let source = reopened.restore_usage_source(&replay.source_id).unwrap();
            assert_eq!(reopened.usage_source(&source).unwrap().observations, 1);
        }
    }

    #[tokio::test]
    async fn native_owned_completion_queue_reply_loss() {
        for committed in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let (mut db, cap) = owned_fixture(root.path());
            let admission = db.owned_dispatch_scope(&cap, now()).unwrap();
            let scope = db
                .start_owned_dispatch(&cap, admission.fingerprint(), now())
                .unwrap();
            let input = hagency_core::completions::CompleteTaskWithReply {
                id: "task".into(),
                call_id: "finish".into(),
                body: "Exact held result".into(),
            };
            let store = DomainStore::start(db, 16).unwrap();
            let (entered, ready) = oneshot::channel();
            let (release, gate) = std::sync::mpsc::channel();
            let worker = store.clone();
            let c = cap.clone();
            let i = input.clone();
            let withheld = tokio::spawn(async move {
                worker
                    .call(1, move |db| {
                        if committed {
                            db.complete_task_with_reply(&c, &i, now())?;
                        }
                        let _ = entered.send(());
                        gate.recv_timeout(Duration::from_secs(4))
                            .map_err(|_| Error::Unavailable)?;
                        Ok(())
                    })
                    .await
            });
            ready.await.unwrap();
            if !committed {
                assert!(matches!(
                    store
                        .runner_command(
                            cap.clone(),
                            RunnerCommand::CompleteTaskWithReply(input.clone())
                        )
                        .await,
                    Err(Error::OutcomeUnknown)
                ));
            }
            assert!(matches!(
                withheld.await.unwrap(),
                Err(Error::OutcomeUnknown)
            ));
            release.send(()).unwrap();
            let observed = store
                .observe_owned_completion(cap.clone(), scope)
                .await
                .unwrap();
            assert_eq!(observed.is_some(), committed);
            let inspect =
                rusqlite::Connection::open(root.path().join("state/domain.sqlite3")).unwrap();
            let task: Task = serde_json::from_str(
                &inspect
                    .query_row(
                        "SELECT config FROM canonical_tasks WHERE id='task'",
                        [],
                        |r| r.get::<_, String>(0),
                    )
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(
                task.status,
                if committed {
                    hagency_core::tasks::TaskState::Done
                } else {
                    hagency_core::tasks::TaskState::InProgress
                }
            );
            assert_eq!(
                inspect
                    .query_row("SELECT COUNT(*) FROM final_replies", [], |r| r
                        .get::<_, u64>(0))
                    .unwrap(),
                0
            );
            if committed {
                let replay = store
                    .runner_command(cap, RunnerCommand::CompleteTaskWithReply(input))
                    .await
                    .unwrap();
                assert_eq!(replay["replayed"], true);
            }
            drop(inspect);
            store.shutdown().await.unwrap();
        }
    }
    #[tokio::test]
    async fn native_owned_completion_queued_cancellation() {
        queued_publication(false).await;
    }
    #[tokio::test]
    async fn native_owned_completion_queued_deadline() {
        queued_publication(true).await;
    }
    async fn queued_publication(expire: bool) {
        let root = tempfile::tempdir().unwrap();
        let (mut db, cap) = owned_fixture(root.path());
        let admission = db.owned_dispatch_scope(&cap, now()).unwrap();
        let scope = db
            .start_owned_dispatch(&cap, admission.fingerprint(), now())
            .unwrap();
        db.complete_task_with_reply(
            &cap,
            &hagency_core::completions::CompleteTaskWithReply {
                id: "task".into(),
                call_id: "finish".into(),
                body: "Result".into(),
            },
            now(),
        )
        .unwrap();
        let reference = db.observe_owned_completion(&cap, &scope).unwrap().unwrap();
        let store = DomainStore::start(db, 16).unwrap();
        let (entered, ready) = oneshot::channel();
        let (release, gate) = std::sync::mpsc::channel();
        let worker = store.clone();
        let hold = tokio::spawn(async move {
            worker
                .call(1, move |_| {
                    let _ = entered.send(());
                    gate.recv_timeout(Duration::from_secs(1))
                        .map_err(|_| Error::Unavailable)?;
                    Ok(())
                })
                .await
        });
        ready.await.unwrap();
        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let signal = cancel.clone();
        let worker = store.clone();
        let deadline =
            std::time::Instant::now() + Duration::from_millis(if expire { 30 } else { 1000 });
        let mut queued =
            Box::pin(worker.publish_owned_completion(cap, scope, reference, signal, deadline));
        std::future::poll_fn(|cx| {
            assert!(std::future::Future::poll(queued.as_mut(), cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await; // call() has enqueued its command before its first Pending.
        if expire {
            tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)).await;
        } else {
            cancel.store(true, std::sync::atomic::Ordering::Release);
        }
        release.send(()).unwrap();
        hold.await.unwrap().unwrap();
        assert!(matches!(queued.await, Err(Error::State)));
        let inspect = rusqlite::Connection::open(root.path().join("state/domain.sqlite3")).unwrap();
        assert_eq!(
            inspect
                .query_row("SELECT COUNT(*) FROM final_replies", [], |r| r
                    .get::<_, u64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            inspect
                .query_row("SELECT COUNT(*) FROM resource_leases", [], |r| r
                    .get::<_, u64>(0))
                .unwrap(),
            1
        );
        drop(inspect);
        let (result, snapshot) = store.shutdown_observed().await;
        if let Err(error) = result {
            panic!("queued completion shutdown failed: {error:?}; {snapshot:?}");
        }
    }

    #[tokio::test]
    async fn native_owned_dispatch_queue_reply_loss() {
        // The writer remains real. Only its receipt is withheld; every gate has
        // a finite bound, and no cancelled or timed-out future is polled again.
        for started in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let (db, cap) = owned_fixture(root.path());
            let fingerprint = db
                .owned_dispatch_scope(&cap, now())
                .unwrap()
                .fingerprint()
                .to_owned();
            let store = DomainStore::start(db, 16).unwrap();
            let (entered, entered_rx) = oneshot::channel();
            let (release, release_rx) = std::sync::mpsc::channel::<()>();
            let worker = store.clone();
            let held_cap = cap.clone();
            let held_fingerprint = fingerprint.clone();
            let withheld = tokio::spawn(async move {
                worker
                    .call(1, move |db| {
                        if started {
                            db.start_owned_dispatch(&held_cap, &held_fingerprint, now())?;
                        }
                        let _ = entered.send(());
                        release_rx
                            .recv_timeout(Duration::from_secs(4))
                            .map_err(|_| Error::Unavailable)?;
                        Ok(())
                    })
                    .await
            });
            entered_rx.await.unwrap();
            if !started {
                // This exact start command enters the queue but its receiver
                // expires before the writer can execute it.
                assert!(matches!(
                    store.start_owned_dispatch(cap.clone(), fingerprint).await,
                    Err(Error::OutcomeUnknown)
                ));
            }
            assert!(matches!(
                withheld.await.unwrap(),
                Err(Error::OutcomeUnknown)
            ));
            release.send(()).unwrap();
            // The next command is also a barrier behind the withheld operation.
            let observed = store
                .observe_owned_failure(cap, crate::OwnedFailure::StartUnknown)
                .await
                .unwrap();
            assert_eq!(
                observed,
                if started {
                    crate::OwnedObservation::Fenced
                } else {
                    crate::OwnedObservation::Unstarted
                }
            );
            let inspect =
                rusqlite::Connection::open(root.path().join("state/domain.sqlite3")).unwrap();
            let state: String = inspect
                .query_row("SELECT state FROM runner_dispatches", [], |r| r.get(0))
                .unwrap();
            assert_eq!(
                state,
                if started {
                    "outcome_unknown"
                } else {
                    "superseded"
                }
            );
            let leases: u64 = inspect
                .query_row("SELECT COUNT(*) FROM resource_leases", [], |r| r.get(0))
                .unwrap();
            assert_eq!(leases, u64::from(started));
            let task: String = inspect
                .query_row(
                    "SELECT json_extract(config,'$.status') FROM canonical_tasks",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(task, if started { "in_progress" } else { "created" });
            store.shutdown().await.unwrap();
        }
    }

    #[tokio::test]
    async fn native_receive_original_clock() {
        use hagency_core::received_files::*;
        let root = tempfile::tempdir().unwrap();
        let (mut db, cap) = owned_fixture_with_attachment(root.path(), true);
        let scope = db.owned_dispatch_scope(&cap, now()).unwrap();
        db.start_owned_dispatch(&cap, scope.fingerprint(), now())
            .unwrap();
        let admitted = db
            .reserve_received_file(&cap, "$receive", 1024, now())
            .unwrap();
        let reservation = admitted.reservation.unwrap();
        let inspect = rusqlite::Connection::open(root.path().join("state/domain.sqlite3")).unwrap();
        inspect.busy_timeout(Duration::ZERO).unwrap();
        // The exact transaction wrapper used by receive commands samples its
        // clock only while it holds the original SQLite IMMEDIATE write lock.
        db.upload_transaction(
            || {
                let error = inspect.execute_batch("BEGIN IMMEDIATE").unwrap_err();
                assert_eq!(
                    error.sqlite_error_code(),
                    Some(rusqlite::ErrorCode::DatabaseBusy)
                );
                Ok(now())
            },
            |tx, n| crate::domain::received_files::reserve(tx, &cap, "$receive", 1024, n),
        )
        .unwrap();
        let store = DomainStore::start(db, 16).unwrap();
        let (entered, ready) = oneshot::channel();
        let (release, gate) = std::sync::mpsc::channel();
        let blocking = store.clone();
        let held = tokio::spawn(async move {
            blocking
                .call(1, move |_| {
                    let _ = entered.send(());
                    gate.recv_timeout(Duration::from_secs(4))
                        .map_err(|_| Error::Unavailable)?;
                    Ok(())
                })
                .await
        });
        ready.await.unwrap();
        let mut request = Box::pin(store.start_received_file_write(
            cap.clone(),
            reservation,
            ReceivedFileFacts {
                size: 3,
                sha256: hagency_core::project::hash(b"abc"),
            },
        ));
        std::future::poll_fn(|cx| {
            assert!(std::future::Future::poll(request.as_mut(), cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        assert_eq!(store.tx.capacity(), 15);
        let queued_at = now();
        let watchdog = tokio::time::Instant::now() + Duration::from_secs(1);
        while now() == queued_at {
            assert!(tokio::time::Instant::now() < watchdog);
            tokio::task::yield_now().await;
        }
        // Only shorten current authority while the original command is queued;
        // do not create a capability or otherwise mutate current proof.
        inspect
            .execute(
                "UPDATE runner_dispatches SET lease_until=?1 WHERE id='dispatch'",
                [now()],
            )
            .unwrap();
        release.send(()).unwrap();
        held.await.unwrap().unwrap();
        assert!(matches!(request.await, Err(Error::RunnerAuthority)));
        assert_eq!(
            store
                .inspect_received_file(cap, admitted.identity.id().into())
                .await
                .unwrap()
                .state,
            ReceivedFileState::Reserved
        );
        store.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn native_receive_workspace_check_clock() {
        let root = tempfile::tempdir().unwrap();
        let (mut db, cap) = owned_fixture_with_attachment(root.path(), true);
        let scope = db.owned_dispatch_scope(&cap, now()).unwrap();
        let fingerprint = scope.fingerprint().to_owned();
        db.start_owned_dispatch(&cap, &fingerprint, now()).unwrap();
        let inspect = rusqlite::Connection::open(root.path().join("state/domain.sqlite3")).unwrap();
        inspect.busy_timeout(Duration::ZERO).unwrap();
        db.check_owned_clock(&cap, &fingerprint, || {
            let error = inspect.execute_batch("BEGIN IMMEDIATE").unwrap_err();
            assert_eq!(
                error.sqlite_error_code(),
                Some(rusqlite::ErrorCode::DatabaseBusy)
            );
            Ok(now())
        })
        .unwrap();
        inspect.execute_batch("BEGIN IMMEDIATE; ROLLBACK").unwrap();
        let store = DomainStore::start(db, 16).unwrap();
        let (entered, ready) = oneshot::channel();
        let (release, gate) = std::sync::mpsc::channel();
        let blocking = store.clone();
        let held = tokio::spawn(async move {
            blocking
                .call(1, move |_| {
                    let _ = entered.send(());
                    gate.recv_timeout(Duration::from_secs(4))
                        .map_err(|_| Error::Unavailable)?;
                    Ok(())
                })
                .await
        });
        ready.await.unwrap();
        let mut request = Box::pin(store.check_owned_dispatch(cap, fingerprint));
        std::future::poll_fn(|cx| {
            assert!(std::future::Future::poll(request.as_mut(), cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        assert_eq!(store.tx.capacity(), 15);
        inspect
            .execute(
                "UPDATE runner_dispatches SET lease_until=?1 WHERE id='dispatch'",
                [now()],
            )
            .unwrap();
        release.send(()).unwrap();
        held.await.unwrap().unwrap();
        assert!(matches!(request.await, Err(Error::RunnerAuthority)));
        store.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn native_runner_clock_after_queue() {
        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("state");
        let mut db = DomainRepository::open(&state).unwrap();
        db.register(&registration()).unwrap();
        let pool = resource("pool", "seat", 100);
        db.put_resource(&pool).unwrap();
        let proof = proof(&request("request", "Worker", &pool, 10));
        let e = db.admit(&proof, 1000).unwrap();
        db.approve("approve", &proof, 1000).unwrap();
        let effect = db.claim_effect().unwrap().unwrap();
        db.observe_effect(
            &effect.id,
            effect.fence,
            &EffectOutcome::Applied {
                receipt: "fixture_identity".into(),
            },
        )
        .unwrap();
        db.register_session(&SessionBinding {
            id: "session".into(),
            engagement_id: e.id,
            room_id: "!project:example.test".into(),
            thread_root: None,
        })
        .unwrap();
        db.enqueue_dispatch(&DispatchInput {
            id: "dispatch".into(),
            session_id: "session".into(),
            task_id: None,
            resources: vec![],
            payload: json!({}),
        })
        .unwrap();
        let cap = db
            .claim_dispatch("runner", now(), 60_000, 60_000, 1)
            .unwrap()
            .unwrap();
        db.start_dispatch(&cap, now()).unwrap();
        let store = DomainStore::start(db, 16).unwrap();
        store
            .runner_command(cap.clone(), RunnerCommand::Check)
            .await
            .unwrap();
        let (entered, entered_rx) = oneshot::channel();
        let (release, release_rx) = std::sync::mpsc::channel::<()>();
        let blocking = store.clone();
        let blocker = tokio::spawn(async move {
            blocking
                .call(1, move |_| {
                    let _ = entered.send(());
                    let _ = release_rx.recv();
                    Ok(())
                })
                .await
        });
        entered_rx.await.unwrap();
        let queued = store.runner_command(cap, RunnerCommand::Check);
        tokio::pin!(queued);
        tokio::select! {
            result=&mut queued=>panic!("writer is blocked; command unexpectedly completed: {result:?}"),
            _=tokio::time::sleep(Duration::from_millis(20))=>{}
        }
        assert_eq!(store.tx.capacity(), 15); // The request has entered the bounded queue.
        let inspect = rusqlite::Connection::open(state.join("domain.sqlite3")).unwrap();
        inspect
            .execute(
                "UPDATE runner_dispatches SET lease_until=?1 WHERE id='dispatch'",
                [now()],
            )
            .unwrap();
        release.send(()).unwrap();
        assert!(matches!(queued.await, Err(Error::RunnerAuthority)));
        blocker.await.unwrap().unwrap();
        store.shutdown().await.unwrap();
    }
}
#[derive(Clone)]
pub struct DomainStore {
    tx: mpsc::Sender<Job>,
    bytes: Arc<Semaphore>,
    progress: Arc<Progress>,
}
/// Queue-position diagnostic for a bounded reply wait (accepted design §1.1:
/// one monotonic ticket counter plus one "which ticket is running now" cell).
/// These values are observations, never execution authority: nothing here may
/// release a lease, mark a task Done or authorize a retry. `started` holds the
/// running job's ticket + 1 so a default-initialised cell means "no job begun
/// yet".
#[derive(Default)]
struct Progress {
    next: AtomicU64,
    started: AtomicU64,
    /// 0 = no `Error::OutcomeUnknown` recorded, 1 = the job was dequeued,
    /// 2 = it had not been dequeued when the reply bound expired.
    last_unknown: AtomicU8,
}
impl Progress {
    fn unknown_dequeued(&self) -> Option<bool> {
        match self.last_unknown.load(Ordering::Acquire) {
            1 => Some(true),
            2 => Some(false),
            _ => None,
        }
    }
}
fn weight(value: &impl Serialize) -> Result<u32, Error> {
    let len = serde_json::to_vec(value)?.len();
    if len > 64 * 1024 {
        return Err(hagency_core::InvalidInput("domain command exceeds 64 KiB").into());
    }
    Ok(len.max(1) as u32)
}
fn writer_time() -> Result<u64, Error> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| u64::try_from(d.as_millis()).ok())
        .ok_or(Error::Unavailable)
}
impl DomainStore {
    /// Host-only logical scope. No runtime command or HTTP projection exposes it.
    pub async fn owned_dispatch_scope(
        &self,
        cap: RunnerCapability,
    ) -> Result<crate::OwnedDispatchScope, Error> {
        self.call(weight(&cap)?, move |db| {
            db.owned_dispatch_scope(&cap, writer_time()?)
        })
        .await
    }
    pub async fn start_owned_dispatch(
        &self,
        cap: RunnerCapability,
        expected: String,
    ) -> Result<crate::OwnedDispatchScope, Error> {
        self.call(weight(&(&cap, &expected))?, move |db| {
            db.start_owned_clock(&cap, &expected, writer_time)
        })
        .await
    }
    pub async fn check_owned_dispatch(
        &self,
        cap: RunnerCapability,
        expected: String,
    ) -> Result<Task, Error> {
        self.call(weight(&(&cap, &expected))?, move |db| {
            db.check_owned_clock(&cap, &expected, writer_time)
        })
        .await
    }
    pub async fn renew_owned_dispatch(
        &self,
        cap: RunnerCapability,
        expected: String,
        lease_ms: u64,
    ) -> Result<Task, Error> {
        self.call(weight(&(&cap, &expected))?, move |db| {
            db.renew_owned_clock(&cap, &expected, lease_ms, writer_time)
        })
        .await
    }
    /// The caller must hold an actually stopped owner. This host API does not
    /// manufacture that process observation and has no runner HTTP equivalent.
    pub async fn complete_owned_dispatch(
        &self,
        cap: RunnerCapability,
        expected: String,
        output: serde_json::Value,
    ) -> Result<Task, Error> {
        self.call(weight(&(&cap, &expected, &output))?, move |db| {
            db.complete_owned_clock(&cap, &expected, &output, writer_time)
        })
        .await
    }
    pub async fn observe_owned_completion(
        &self,
        cap: RunnerCapability,
        scope: crate::OwnedDispatchScope,
    ) -> Result<Option<crate::OwnedCompletion>, Error> {
        let bytes = weight(&(&cap, scope.queue_value()))?;
        self.call(bytes, move |db| db.observe_owned_completion(&cap, &scope))
            .await
    }
    /// Caller retains this scope alongside the exact owner and has observed its
    /// complete stop. No runtime/API accepts this publication command.
    pub async fn publish_owned_completion(
        &self,
        cap: RunnerCapability,
        scope: crate::OwnedDispatchScope,
        reference: crate::OwnedCompletion,
        cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
        deadline: std::time::Instant,
    ) -> Result<hagency_core::completions::CompletionReceipt, Error> {
        let bytes = weight(&(&cap, scope.queue_value(), reference.id()))?;
        self.call(bytes, move |db| {
            db.publish_completion_clock(&cap, &scope, &reference, || {
                // Linearizes publication eligibility after writer queue and DB
                // lock, against the original cancellation signal and absolute
                // monotonic operation deadline, not a renewed queue-time budget.
                if cancel.load(std::sync::atomic::Ordering::Acquire)
                    || std::time::Instant::now() >= deadline
                {
                    return Err(Error::State);
                }
                writer_time()
            })
        })
        .await
    }
    pub async fn observe_owned_failure(
        &self,
        cap: RunnerCapability,
        failure: crate::OwnedFailure,
    ) -> Result<crate::OwnedObservation, Error> {
        self.call(weight(&(&cap, &failure))?, move |db| {
            db.observe_owned_failure(&cap, failure, writer_time()?)
        })
        .await
    }
    pub async fn observe_approval_room(
        &self,
        input: hagency_core::approvals::ApprovalRoomObservation,
    ) -> Result<(), Error> {
        self.call(weight(&input)?, move |db| {
            db.observe_approval_room(&input, writer_time()?)
        })
        .await
    }
    pub async fn bind_approval_context(
        &self,
        cap: RunnerCapability,
        input: hagency_core::approvals::HostApprovalContext,
    ) -> Result<(), Error> {
        self.call(weight(&input)?, move |db| {
            db.bind_approval_context_clock(&cap, &input, writer_time)
        })
        .await
    }
    pub async fn request_owner_approval(
        &self,
        cap: RunnerCapability,
        input: hagency_core::approvals::HostApprovalRequest,
    ) -> Result<hagency_core::approvals::ApprovalSummary, Error> {
        self.call(weight(&input)?, move |db| {
            db.request_owner_approval_clock(&cap, &input, writer_time)
        })
        .await
    }
    pub async fn observe_owner_verdict(
        &self,
        input: hagency_core::approvals::OwnerVerdictObservation,
    ) -> Result<hagency_core::approvals::ApprovalSummary, Error> {
        self.call(weight(&input)?, move |db| {
            db.observe_owner_verdict_clock(&input, writer_time)
        })
        .await
    }
    pub async fn consume_owner_approval(
        &self,
        cap: RunnerCapability,
        id: String,
    ) -> Result<hagency_core::approvals::ApprovalApplication, Error> {
        self.call(weight(&id)?, move |db| {
            db.consume_owner_approval_clock(&cap, &id, writer_time)
        })
        .await
    }
    pub async fn observe_approval_application(
        &self,
        input: hagency_core::approvals::ApprovalApplicationObservation,
    ) -> Result<hagency_core::approvals::ApprovalSummary, Error> {
        self.call(weight(&input)?, move |db| {
            db.observe_approval_application_clock(&input, writer_time)
        })
        .await
    }
    pub async fn approval_summary(
        &self,
        id: String,
    ) -> Result<hagency_core::approvals::ApprovalSummary, Error> {
        self.call(weight(&id)?, move |db| db.approval_summary(&id))
            .await
    }
    pub async fn private_approval(
        &self,
        id: String,
    ) -> Result<hagency_core::approvals::PrivateApproval, Error> {
        self.call(weight(&id)?, move |db| {
            db.private_approval(&id, writer_time()?)
        })
        .await
    }
    pub async fn private_approval_card(
        &self,
        id: String,
        owner_expires_at: u64,
    ) -> Result<crate::PrivateApprovalCard, Error> {
        self.call(weight(&id)? + 8, move |db| {
            db.private_approval_card_clock(&id, owner_expires_at, writer_time)
        })
        .await
    }
    /// Retains the original opaque packet across a queued fresh comparison.
    pub async fn check_private_approval_card(
        &self,
        card: Arc<crate::PrivateApprovalCard>,
    ) -> Result<(), Error> {
        self.call(weight(card.content())?, move |db| {
            db.check_private_approval_card_clock(&card, writer_time)
        })
        .await
    }
    pub async fn revoke_approval_grant(&self, id: String) -> Result<(), Error> {
        self.call(weight(&id)?, move |db| db.revoke_approval_grant(&id))
            .await
    }
    pub async fn approval_grants(
        &self,
        engagement: String,
        after: String,
        limit: u64,
    ) -> Result<Vec<hagency_core::approvals::GrantSummary>, Error> {
        self.call(weight(&(&engagement, &after))?, move |db| {
            db.approval_grants(&engagement, &after, limit)
        })
        .await
    }
    pub async fn matrix_ingress_scope(
        &self,
        session: String,
    ) -> Result<hagency_core::ingress::MatrixIngressScope, Error> {
        self.call(weight(&session)?, move |db| {
            db.matrix_ingress_scope(&session)
        })
        .await
    }
    pub async fn matrix_intake_route(
        &self,
        session: String,
    ) -> Result<hagency_core::replies::ReplyRoute, Error> {
        self.call(weight(&session)?, move |db| {
            db.matrix_intake_route(&session)
        })
        .await
    }
    pub async fn matrix_ingress_receipt(
        &self,
        input: hagency_core::ingress::MatrixEventObservation,
    ) -> Result<Option<hagency_core::ingress::MatrixIngressReceipt>, Error> {
        self.call(weight(&input)?, move |db| db.matrix_ingress_receipt(&input))
            .await
    }
    pub async fn admit_matrix_event(
        &self,
        input: hagency_core::ingress::MatrixEventObservation,
    ) -> Result<hagency_core::ingress::MatrixIngressReceipt, Error> {
        self.call(weight(&input)?, move |db| {
            db.admit_matrix_event(&input, writer_time()?)
        })
        .await
    }
    pub async fn admit_matrix_attachment(
        &self,
        input: hagency_core::attachments::MatrixAttachmentObservation,
    ) -> Result<hagency_core::ingress::MatrixIngressReceipt, Error> {
        self.call(weight(&input)?, move |db| {
            db.admit_matrix_attachment(&input, writer_time()?)
        })
        .await
    }
    pub async fn matrix_attachment_receipt(
        &self,
        input: hagency_core::attachments::MatrixAttachmentObservation,
    ) -> Result<Option<hagency_core::ingress::MatrixIngressReceipt>, Error> {
        self.call(weight(&input)?, move |db| {
            db.matrix_attachment_receipt(&input)
        })
        .await
    }
    pub async fn authorize_attachment(
        &self,
        cap: RunnerCapability,
        event_id: String,
    ) -> Result<crate::AttachmentTicket, Error> {
        self.call(weight(&(&cap, &event_id))?, move |db| {
            db.authorize_attachment(&cap, &event_id, writer_time()?)
        })
        .await
    }
    pub async fn revalidate_attachment(
        &self,
        cap: RunnerCapability,
        ticket: crate::AttachmentTicket,
    ) -> Result<(), Error> {
        // The sealed ticket is fixed and bounded by the admitted metadata limits.
        self.call(weight(&cap)? + 4096, move |db| {
            db.revalidate_attachment(&cap, &ticket, writer_time()?)
        })
        .await
    }
    pub async fn create_verified_task_intent(
        &self,
        input: hagency_core::ingress::VerifiedTaskRequest,
    ) -> Result<IntentResult, Error> {
        self.call(weight(&input)?, move |db| {
            db.create_verified_task_intent(&input, writer_time()?)
        })
        .await
    }
    pub async fn claim_verified_task_notice(
        &self,
        lease_ms: u64,
    ) -> Result<Option<hagency_core::ingress::VerifiedNoticeClaim>, Error> {
        self.call(1, move |db| {
            db.claim_verified_task_notice(writer_time()?, lease_ms)
        })
        .await
    }
    pub async fn begin_verified_task_notice_send(
        &self,
        id: String,
        token: String,
    ) -> Result<hagency_core::ingress::VerifiedNoticeSend, Error> {
        self.call(weight(&(&id, &token))?, move |db| {
            db.begin_verified_task_notice_send(&id, &token, writer_time()?)
        })
        .await
    }
    pub async fn verified_notice_receipt(
        &self,
        id: String,
    ) -> Result<hagency_core::ingress::VerifiedNoticeReceipt, Error> {
        self.call(weight(&id)?, move |db| db.verified_notice_receipt(&id))
            .await
    }
    pub async fn cancel_verified_task_notice(
        &self,
        id: String,
    ) -> Result<hagency_core::ingress::VerifiedNoticeReceipt, Error> {
        self.call(weight(&id)?, move |db| {
            db.cancel_verified_task_notice(&id, writer_time()?)
        })
        .await
    }
    pub async fn reconcile_verified_task_notice(
        &self,
        id: String,
        fence: u64,
        input: ReplyReconciliation,
    ) -> Result<hagency_core::ingress::VerifiedNoticeReceipt, Error> {
        self.call(weight(&(&id, &input))?, move |db| {
            db.reconcile_verified_task_notice(&id, fence, &input, writer_time()?)
        })
        .await
    }
    pub async fn deliver_verified_task_notice(
        &self,
        id: String,
        token: String,
        input: ReplyDeliveryObservation,
    ) -> Result<IntentResult, Error> {
        self.call(weight(&(&id, &token, &input))?, move |db| {
            db.deliver_verified_task_notice(&id, &token, &input, writer_time()?)
        })
        .await
    }

    pub async fn matrix_transport_state(
        &self,
        engagement: String,
    ) -> Result<Option<MatrixTransportState>, Error> {
        self.call(weight(&engagement)?, move |db| {
            db.matrix_transport_state(&engagement)
        })
        .await
    }
    pub async fn matrix_room_state(
        &self,
        engagement: String,
        room: String,
    ) -> Result<Option<MatrixRoomState>, Error> {
        self.call(weight(&(&engagement, &room))?, move |db| {
            db.matrix_room_state(&engagement, &room)
        })
        .await
    }
    pub async fn invalidate_matrix_transport(
        &self,
        input: MatrixTransportInvalidation,
    ) -> Result<(), Error> {
        self.call_with_policy(
            weight(&input)?,
            ReceiverPolicy::RetainEnqueuedInvalidation,
            move |db| db.invalidate_matrix_transport(&input, writer_time()?),
        )
        .await
    }
    pub async fn observe_matrix_transport(
        &self,
        input: MatrixTransportObservation,
    ) -> Result<(), Error> {
        self.call(weight(&input)?, move |db| {
            db.observe_matrix_transport(&input, writer_time()?)
        })
        .await
    }
    pub async fn observe_matrix_room(&self, input: MatrixRoomObservation) -> Result<(), Error> {
        self.call(weight(&input)?, move |db| {
            db.observe_matrix_room(&input, writer_time()?)
        })
        .await
    }
    pub async fn invalidate_matrix_room(&self, input: MatrixRoomInvalidation) -> Result<(), Error> {
        let bytes = weight(&input)?;
        self.call_with_policy(
            bytes,
            ReceiverPolicy::RetainEnqueuedInvalidation,
            move |db| db.invalidate_matrix_room(&input, writer_time()?),
        )
        .await
    }

    pub async fn resolve_verified_matrix_session(
        &self,
        input: SessionBinding,
    ) -> Result<SessionBinding, Error> {
        self.call(weight(&input)?, move |db| {
            db.resolve_verified_matrix_session(&input, writer_time()?)
        })
        .await
    }
    pub async fn claim_final_reply(&self, lease_ms: u64) -> Result<Option<ReplyClaim>, Error> {
        self.call(1, move |db| db.claim_final_reply(writer_time()?, lease_ms))
            .await
    }
    pub async fn preview_final_reply(&self, claim: ReplyClaim) -> Result<ReplySend, Error> {
        self.call(
            weight(&(&claim.id, claim.fence, &claim.secret))?,
            move |db| db.preview_final_reply(&claim, writer_time()?),
        )
        .await
    }
    pub async fn validate_final_reply_send(&self, claim: ReplyClaim) -> Result<(), Error> {
        self.call(
            weight(&(&claim.id, claim.fence, &claim.secret))?,
            move |db| db.validate_final_reply_send(&claim, writer_time()?),
        )
        .await
    }
    pub async fn begin_final_reply_send(&self, claim: ReplyClaim) -> Result<ReplySend, Error> {
        self.call(weight(&(&claim.id, &claim.secret))?, move |db| {
            db.begin_final_reply_send(&claim, writer_time()?)
        })
        .await
    }
    pub async fn observe_final_reply(
        &self,
        claim: ReplyClaim,
        input: ReplyDeliveryObservation,
    ) -> Result<ReplyReceipt, Error> {
        self.call(weight(&(&claim.id, &claim.secret, &input))?, move |db| {
            db.observe_final_reply(&claim, &input, writer_time()?)
        })
        .await
    }
    pub async fn cancel_final_reply(&self, id: String) -> Result<ReplyReceipt, Error> {
        self.call(weight(&id)?, move |db| {
            db.cancel_final_reply(&id, writer_time()?)
        })
        .await
    }
    pub async fn reconcile_final_reply(
        &self,
        id: String,
        fence: u64,
        input: ReplyReconciliation,
    ) -> Result<ReplyReceipt, Error> {
        self.call(weight(&(&id, &input))?, move |db| {
            db.reconcile_final_reply(&id, fence, &input, writer_time()?)
        })
        .await
    }
    /// Host cancellation adapter only; deliberately absent from RunnerCommand.
    pub async fn pending_conversation_stops(
        &self,
        after: String,
        limit: usize,
    ) -> Result<Vec<(String, u64)>, Error> {
        self.call(weight(&after)?, move |db| {
            db.pending_conversation_stops(&after, limit)
        })
        .await
    }
    /// The host supplies an already inspected result, never a runner assertion.
    pub async fn settle_conversation_stop(
        &self,
        id: String,
        fence: u64,
        evidence: String,
    ) -> Result<(), Error> {
        self.call(weight(&(&id, &evidence))?, move |db| {
            db.settle_conversation_stop(&id, fence, &evidence, writer_time()?)
        })
        .await
    }
    pub async fn create_task_intent(&self, input: TaskIntent) -> Result<IntentResult, Error> {
        input.definition.validate()?;
        self.call(weight(&input)?, move |db| {
            db.create_task_intent(&input, writer_time()?)
        })
        .await
    }
    pub async fn attach_task_inputs(
        &self,
        task: String,
        scope: String,
        key: String,
        sequences: Vec<u64>,
    ) -> Result<(), Error> {
        self.call(weight(&(&task, &scope, &key, &sequences))?, move |db| {
            db.attach_task_inputs(&task, &scope, &key, &sequences)
        })
        .await
    }
    pub async fn claim_task_notice(&self, lease_ms: u64) -> Result<Option<NoticeClaim>, Error> {
        self.call(1, move |db| db.claim_task_notice(writer_time()?, lease_ms))
            .await
    }
    pub async fn deliver_task_notice(
        &self,
        id: String,
        token: String,
        receipt: NoticeDelivery,
    ) -> Result<IntentResult, Error> {
        receipt.validate()?;
        self.call(weight(&(&id, &token, &receipt))?, move |db| {
            db.deliver_task_notice(&id, &token, &receipt, writer_time()?)
        })
        .await
    }
    pub async fn fail_task_notice(
        &self,
        id: String,
        token: String,
        code: String,
        permanent: bool,
    ) -> Result<(), Error> {
        self.call(weight(&(&id, &token, &code))?, move |db| {
            db.fail_task_notice(&id, &token, &code, permanent, writer_time()?)
        })
        .await
    }
    pub async fn retry_task_notice(&self, id: String) -> Result<(), Error> {
        self.call(weight(&id)?, move |db| {
            db.retry_task_notice(&id, writer_time()?)
        })
        .await
    }
    /// Obtain wall time inside the writer, after queueing; callers cannot freeze
    /// authorization at request arrival or supply a historical clock.
    pub async fn runner_command(
        &self,
        cap: RunnerCapability,
        command: RunnerCommand,
    ) -> Result<serde_json::Value, Error> {
        self.call(weight(&(&cap, &command))?, move |db| {
            let now = writer_time()?;
            Ok(match command {
                RunnerCommand::CompleteTaskWithReply(input) => {
                    serde_json::to_value(db.finish_task_clock(&cap, &input, writer_time)?)?
                }
                RunnerCommand::SubmitFinalReply(input) => {
                    serde_json::to_value(db.submit_final_reply(&cap, &input, now)?)?
                }
                RunnerCommand::FinalReply { id } => {
                    serde_json::to_value(db.runner_final_reply(&cap, &id, now)?)?
                }
                RunnerCommand::CreateWorkflow(input) => {
                    serde_json::to_value(db.create_workflow(&cap, &input, now)?)?
                }
                RunnerCommand::Workflow { id } => {
                    serde_json::to_value(db.runner_workflow(&cap, &id, now)?)?
                }
                RunnerCommand::Workflows { after, limit } => {
                    serde_json::to_value(db.runner_workflows(&cap, &after, limit, now)?)?
                }
                RunnerCommand::CancelWorkflow { id, input } => {
                    serde_json::to_value(db.cancel_workflow(&cap, &id, &input, now)?)?
                }
                RunnerCommand::WorkflowResult { id, input } => {
                    serde_json::to_value(db.report_workflow_result(&cap, &id, &input, now)?)?
                }
                RunnerCommand::WorkflowDependencies { id, after, limit } => {
                    serde_json::to_value(db.workflow_dependencies(&cap, &id, after, limit, now)?)?
                }
                RunnerCommand::WorkflowDependency { id, node_id } => {
                    serde_json::to_value(db.workflow_dependency(&cap, &id, &node_id, now)?)?
                }
                RunnerCommand::SendPeer(input) => {
                    serde_json::to_value(db.send_peer(&cap, &input, now)?)?
                }
                RunnerCommand::PeerInbox { after, limit } => {
                    serde_json::to_value(db.runner_peer_inbox(&cap, after, limit, now)?)?
                }
                RunnerCommand::OpenConversation(input) => {
                    serde_json::to_value(db.create_internal_conversation(&cap, &input, now)?)?
                }
                RunnerCommand::ChangeConversation { id, change } => {
                    serde_json::to_value(db.change_internal_conversation(&cap, &id, &change, now)?)?
                }
                RunnerCommand::Conversation { id } => {
                    serde_json::to_value(db.runner_conversation(&cap, &id, now)?)?
                }
                RunnerCommand::Delegate(input) => {
                    serde_json::to_value(db.delegate_task(&cap, &input, now)?)?
                }
                RunnerCommand::Check => {
                    db.check_runner(&cap, now)?;
                    serde_json::Value::Null
                }
                RunnerCommand::Task { id } => {
                    serde_json::to_value(db.runner_task(&cap, &id, now)?)?
                }
                RunnerCommand::Tasks { after, limit } => {
                    serde_json::to_value(db.runner_tasks(&cap, &after, limit, now)?)?
                }
                RunnerCommand::Comments { id, after, limit } => {
                    serde_json::to_value(db.runner_comments(&cap, &id, after, limit, now)?)?
                }
                RunnerCommand::Inbox { after, limit } => {
                    serde_json::to_value(db.runner_inbox(&cap, after, limit, now)?)?
                }
                RunnerCommand::Mutate {
                    id,
                    call_id,
                    operation,
                } => serde_json::to_value(db.mutate_task(&cap, &id, &call_id, &operation, now)?)?,
            })
        })
        .await
    }
    pub async fn check_runner(&self, cap: RunnerCapability, now: u64) -> Result<(), Error> {
        self.call(weight(&cap)?, move |db| db.check_runner(&cap, now))
            .await
    }
    pub async fn resolve_session(&self, binding: SessionBinding) -> Result<SessionBinding, Error> {
        self.call(weight(&binding)?, move |db| db.resolve_session(&binding))
            .await
    }
    pub async fn ingest_message(
        &self,
        input: InboundMessage,
        targets: Vec<MessageTarget>,
        now: u64,
    ) -> Result<MessageReceipt, Error> {
        input.validate()?;
        self.call(weight(&(&input, &targets))?, move |db| {
            db.ingest_message(&input, &targets, now)
        })
        .await
    }
    pub async fn inbox(
        &self,
        session: String,
        after: u64,
        limit: usize,
        kind: Option<String>,
    ) -> Result<Vec<InboxItem>, Error> {
        self.call(weight(&(&session, &kind))?, move |db| {
            db.inbox(&session, after, limit, kind.as_deref())
        })
        .await
    }
    pub async fn peer_inbox(
        &self,
        session: String,
        after: u64,
        limit: usize,
    ) -> Result<Vec<hagency_core::peers::PeerInboxItem>, Error> {
        self.call(weight(&session)?, move |db| {
            db.peer_inbox(&session, after, limit)
        })
        .await
    }
    pub async fn enqueue_peer_dispatch(
        &self,
        input: DispatchInput,
        sequences: Vec<u64>,
    ) -> Result<(), Error> {
        input.validate()?;
        self.call(weight(&(&input, &sequences))?, move |db| {
            db.enqueue_peer_dispatch(&input, &sequences)
        })
        .await
    }
    pub async fn enqueue_inbox_dispatch(
        &self,
        input: DispatchInput,
        sequences: Vec<u64>,
    ) -> Result<(), Error> {
        input.validate()?;
        self.call(weight(&(&input, &sequences))?, move |db| {
            db.enqueue_inbox_dispatch(&input, &sequences)
        })
        .await
    }
    pub async fn runner_inbox(
        &self,
        cap: RunnerCapability,
        after: u64,
        limit: usize,
        now: u64,
    ) -> Result<Vec<InboxItem>, Error> {
        self.call(weight(&cap)?, move |db| {
            db.runner_inbox(&cap, after, limit, now)
        })
        .await
    }
    pub async fn register_session(&self, binding: SessionBinding) -> Result<(), Error> {
        self.call(weight(&binding)?, move |db| db.register_session(&binding))
            .await
    }
    pub async fn register_workspace(&self, id: String) -> Result<(), Error> {
        self.call(weight(&id)?, move |db| db.register_workspace(&id))
            .await
    }
    pub async fn create_canonical_task(
        &self,
        id: String,
        session: String,
        title: String,
        now: u64,
    ) -> Result<Task, Error> {
        self.call(weight(&(&id, &session, &title))?, move |db| {
            db.create_canonical_task(&id, &session, &title, now)
        })
        .await
    }
    pub async fn create_coordinator_task(
        &self,
        cap: RunnerCapability,
        id: String,
        session: String,
        title: String,
        now: u64,
    ) -> Result<Task, Error> {
        self.call(weight(&(&cap, &id, &session, &title))?, move |db| {
            db.create_coordinator_task(&cap, &id, &session, &title, now)
        })
        .await
    }
    pub async fn enqueue_dispatch(&self, input: DispatchInput) -> Result<(), Error> {
        input.validate()?;
        self.call(weight(&input)?, move |db| db.enqueue_dispatch(&input))
            .await
    }
    pub async fn claim_dispatch(
        &self,
        runner: String,
        now: u64,
        lease_ms: u64,
        capability_ms: u64,
        max_live: u32,
    ) -> Result<Option<RunnerCapability>, Error> {
        self.call(weight(&runner)?, move |db| {
            db.claim_dispatch(&runner, now, lease_ms, capability_ms, max_live)
        })
        .await
    }
    /// One host-selected compatible claim; time is observed inside the transaction.
    pub async fn claim_owned_dispatch_for_host(
        &self,
        profile: crate::OwnedClaimProfile,
        runner: String,
        lease_ms: u64,
        capability_ms: u64,
        max_live: u32,
    ) -> Result<Option<RunnerCapability>, Error> {
        self.call(weight(&(profile.encoded(), &runner))?, move |db| {
            db.claim_owned_clock(
                &profile,
                &runner,
                lease_ms,
                capability_ms,
                max_live,
                writer_time,
            )
        })
        .await
    }
    pub async fn start_dispatch(
        &self,
        cap: RunnerCapability,
        now: u64,
    ) -> Result<serde_json::Value, Error> {
        self.call(weight(&cap)?, move |db| db.start_dispatch(&cap, now))
            .await
    }
    pub async fn park_dispatch(
        &self,
        cap: RunnerCapability,
        parked: bool,
        now: u64,
    ) -> Result<(), Error> {
        self.call(weight(&cap)?, move |db| db.park_dispatch(&cap, parked, now))
            .await
    }
    pub async fn renew_dispatch(
        &self,
        cap: RunnerCapability,
        now: u64,
        lease_ms: u64,
    ) -> Result<(), Error> {
        self.call(weight(&cap)?, move |db| {
            db.renew_dispatch(&cap, now, lease_ms)
        })
        .await
    }
    pub async fn fail_before_start(
        &self,
        cap: RunnerCapability,
        now: u64,
        retry_ms: u64,
    ) -> Result<(), Error> {
        self.call(weight(&cap)?, move |db| {
            db.fail_before_start(&cap, now, retry_ms)
        })
        .await
    }
    pub async fn complete_dispatch(
        &self,
        cap: RunnerCapability,
        output: serde_json::Value,
        now: u64,
    ) -> Result<(), Error> {
        self.call(weight(&(&cap, &output))?, move |db| {
            db.complete_dispatch(&cap, &output, now)
        })
        .await
    }
    pub async fn record_late_output(
        &self,
        cap: RunnerCapability,
        output: serde_json::Value,
    ) -> Result<(), Error> {
        self.call(weight(&(&cap, &output))?, move |db| {
            db.record_late_output(&cap, &output)
        })
        .await
    }
    pub async fn reconcile_dispatches(&self, now: u64) -> Result<(), Error> {
        self.call(1, move |db| db.reconcile_dispatches(now)).await
    }
    pub async fn recover_dispatch(
        &self,
        original: String,
        replacement: DispatchInput,
        evidence: String,
        now: u64,
    ) -> Result<(), Error> {
        self.call(weight(&(&original, &replacement, &evidence))?, move |db| {
            db.recover_dispatch(&original, &replacement, &evidence, now)
        })
        .await
    }
    pub async fn runner_task(
        &self,
        cap: RunnerCapability,
        id: String,
        now: u64,
    ) -> Result<Task, Error> {
        self.call(weight(&(&cap, &id))?, move |db| {
            db.runner_task(&cap, &id, now)
        })
        .await
    }
    pub async fn runner_tasks(
        &self,
        cap: RunnerCapability,
        after: String,
        limit: usize,
        now: u64,
    ) -> Result<Vec<Task>, Error> {
        self.call(weight(&(&cap, &after))?, move |db| {
            db.runner_tasks(&cap, &after, limit, now)
        })
        .await
    }
    pub async fn mutate_task(
        &self,
        cap: RunnerCapability,
        id: String,
        call_id: String,
        mutation: TaskMutation,
        now: u64,
    ) -> Result<MutationResult, Error> {
        self.call(weight(&(&cap, &id, &call_id, &mutation))?, move |db| {
            db.mutate_task(&cap, &id, &call_id, &mutation, now)
        })
        .await
    }
    pub async fn runner_comments(
        &self,
        cap: RunnerCapability,
        id: String,
        after: u64,
        limit: usize,
        now: u64,
    ) -> Result<Vec<TaskComment>, Error> {
        self.call(weight(&(&cap, &id))?, move |db| {
            db.runner_comments(&cap, &id, after, limit, now)
        })
        .await
    }
    pub async fn task_events(&self, after: u64, limit: usize) -> Result<Vec<TaskEvent>, Error> {
        self.call(1, move |db| db.task_events(after, limit)).await
    }
    pub fn start(mut repository: DomainRepository, capacity: usize) -> Result<Self, Error> {
        if !(1..=128).contains(&capacity) {
            return Err(hagency_core::InvalidInput("queue capacity must be 1..128").into());
        }
        let (tx, mut rx) = mpsc::channel(capacity);
        let progress = Arc::new(Progress::default());
        std::thread::Builder::new()
            .name("hagency-domain".into())
            .spawn(move || {
                while let Some(job) = rx.blocking_recv() {
                    match job {
                        Job::Run { operation, _bytes } => operation(&mut repository),
                        Job::Shutdown { reply, probe } => {
                            mark(&probe, Phase::WorkerPickedUp);
                            mark(&probe, Phase::DropStarted);
                            if let Some(probe) = &probe {
                                repository.drop_observed(probe);
                            } else {
                                drop(repository);
                            }
                            mark(&probe, Phase::DropFinished);
                            mark(&probe, Phase::AcknowledgementStarted);
                            if reply.send(()).is_ok() {
                                mark(&probe, Phase::AcknowledgementSent);
                            }
                            return;
                        }
                    }
                }
            })?;
        Ok(Self {
            tx,
            bytes: Arc::new(Semaphore::new(8 * 1024 * 1024)),
            progress,
        })
    }
    async fn call<T: Send + 'static>(
        &self,
        bytes: u32,
        operation: impl FnOnce(&mut DomainRepository) -> Result<T, Error> + Send + 'static,
    ) -> Result<T, Error> {
        self.call_with_policy(bytes, ReceiverPolicy::CancelIfDropped, operation)
            .await
    }
    async fn call_with_policy<T: Send + 'static>(
        &self,
        bytes: u32,
        policy: ReceiverPolicy,
        operation: impl FnOnce(&mut DomainRepository) -> Result<T, Error> + Send + 'static,
    ) -> Result<T, Error> {
        let ticket = self.progress.next.fetch_add(1, Ordering::AcqRel);
        let permit = self
            .bytes
            .clone()
            .try_acquire_many_owned(bytes)
            .map_err(|_| Error::Busy)?;
        let (reply, rx) = oneshot::channel();
        let progress = self.progress.clone();
        let operation = Box::new(move |db: &mut DomainRepository| {
            // Publish the running ticket BEFORE this job's operation can block,
            // so a caller whose reply bound expires while this job runs
            // observes "dequeued" and never "still queued" (accepted design
            // §1.1). Directly enqueued test jobs take no ticket and never
            // publish, which leaves `started` at its default.
            progress
                .started
                .store(ticket.wrapping_add(1), Ordering::Release);
            // An admitted negative observation must retire its exact old scope
            // even after receiver loss. Ordinary abandoned work still stops here.
            if matches!(policy, ReceiverPolicy::RetainEnqueuedInvalidation) || !reply.is_closed() {
                let _ = reply.send(operation(db));
            }
        });
        self.tx
            .try_send(Job::Run {
                operation,
                _bytes: permit,
            })
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => Error::Busy,
                mpsc::error::TrySendError::Closed(_) => Error::Unavailable,
            })?;
        tokio::time::timeout(Duration::from_secs(2), rx)
            .await
            .map_err(|_| {
                // Attribution by equality, not inequality: this job was the
                // one running exactly when the writer published this ticket
                // + 1. Any other value means the reply bound expired while
                // this command was still queued behind another job.
                let dequeued =
                    self.progress.started.load(Ordering::Acquire) == ticket.wrapping_add(1);
                // 1 = dequeued, 2 = still queued, matching `unknown_dequeued`.
                self.progress
                    .last_unknown
                    .store(if dequeued { 1 } else { 2 }, Ordering::Release);
                Error::OutcomeUnknown
            })?
            .map_err(|_| Error::Unavailable)?
    }
    /// Diagnostic attribution for the most recent `Error::OutcomeUnknown` this
    /// store produced: whether the writer had dequeued (begun) that command
    /// when its reply bound expired. `None` when no such error occurred. The
    /// cell is shared across callers, so it is exact for a single expiring
    /// caller and diagnostic-only otherwise. It is never execution authority:
    /// nothing may read it to release a lease, mark a task Done or authorize a
    /// retry (accepted design §1.3).
    pub fn last_unknown_dequeued(&self) -> Option<bool> {
        self.progress.unknown_dequeued()
    }
    pub async fn shutdown(&self) -> Result<(), Error> {
        self.shutdown_tracked(None).await.0
    }

    /// Diagnose one host shutdown attempt without changing its queue or waits.
    /// The snapshot is not proof of rollback, OS-thread exit or retry safety.
    pub async fn shutdown_observed(&self) -> (Result<(), Error>, ShutdownSnapshot) {
        let probe = Arc::new(Probe::new());
        let (result, outcome) = self.shutdown_tracked(Some(probe.clone())).await;
        (result, probe.snapshot(outcome))
    }

    async fn shutdown_tracked(
        &self,
        probe: Option<Arc<Probe>>,
    ) -> (Result<(), Error>, ShutdownOutcome) {
        let (reply, rx) = oneshot::channel();
        mark(&probe, Phase::EnqueueStarted);
        let queued = tokio::time::timeout(
            Duration::from_secs(2),
            self.tx.send(Job::Shutdown {
                reply,
                probe: probe.clone(),
            }),
        )
        .await;
        let verdict = match queued {
            Err(_) => (Err(Error::OutcomeUnknown), ShutdownOutcome::EnqueueTimedOut),
            Ok(Err(_)) => (Err(Error::Unavailable), ShutdownOutcome::EnqueueClosed),
            Ok(Ok(())) => {
                mark(&probe, Phase::EnqueueObserved);
                match tokio::time::timeout(Duration::from_secs(2), rx).await {
                    Err(_) => (Err(Error::OutcomeUnknown), ShutdownOutcome::ReplyTimedOut),
                    Ok(Err(_)) => (Err(Error::Unavailable), ShutdownOutcome::ReplyClosed),
                    Ok(Ok(())) => (Ok(()), ShutdownOutcome::Complete),
                }
            }
        };
        mark(&probe, Phase::CallerFinished);
        verdict
    }
    pub async fn put_resource(&self, resource: Resource) -> Result<CatalogResource, Error> {
        resource.validate()?;
        self.call(weight(&resource)?, move |db| db.put_resource(&resource))
            .await
    }
    pub async fn put_seat(&self, seat: Seat) -> Result<(), Error> {
        seat.validate()?;
        self.call(weight(&seat)?, move |db| db.put_seat(&seat))
            .await
    }
    pub async fn set_role_publication(&self, role: String, published: bool) -> Result<(), Error> {
        self.call(weight(&role)?, move |db| {
            db.set_role_publication(&role, published)
        })
        .await
    }
    pub async fn role_publications(&self) -> Result<Vec<serde_json::Value>, Error> {
        self.call(1, |db| db.role_publications()).await
    }
    pub async fn catalog_for(
        &self,
        fleet: String,
        after: String,
        limit: usize,
    ) -> Result<Vec<CatalogResource>, Error> {
        self.call(weight(&(&fleet, &after))?, move |db| {
            db.catalog_for(Some(&fleet), &after, limit)
        })
        .await
    }
    pub async fn check_publication_registration(
        &self,
        identity: crate::outbound::RegistrationIdentity,
    ) -> Result<(), Error> {
        self.call(weight(&identity)?, move |db| {
            db.check_publication_registration(&identity)
        })
        .await
    }
    pub async fn published_catalog(
        &self,
        identity: crate::outbound::RegistrationIdentity,
    ) -> Result<crate::PublishedCatalog, Error> {
        self.call(weight(&identity)?, move |db| {
            db.published_catalog(&identity)
        })
        .await
    }
    pub async fn edit_resource(
        &self,
        resource: Resource,
        publication: Option<bool>,
    ) -> Result<CatalogResource, Error> {
        self.call(weight(&resource)?, move |db| {
            db.edit_resource(&resource, publication)
        })
        .await
    }
    pub async fn resource_configurations(
        &self,
        after: String,
        limit: usize,
    ) -> Result<Vec<ConfiguredResource>, Error> {
        self.call(weight(&after)?, move |db| {
            db.resource_configurations(&after, limit)
        })
        .await
    }
    pub async fn seats(&self, after: String, limit: usize) -> Result<Vec<Seat>, Error> {
        self.call(weight(&after)?, move |db| db.seats(&after, limit))
            .await
    }
    pub async fn catalog(
        &self,
        after: String,
        limit: usize,
    ) -> Result<Vec<CatalogResource>, Error> {
        self.call(weight(&after)?, move |db| db.catalog(&after, limit))
            .await
    }
    pub async fn engagements(&self, after: String, limit: usize) -> Result<Vec<Engagement>, Error> {
        self.call(weight(&after)?, move |db| db.engagements(&after, limit))
            .await
    }
    pub async fn resource_budget(&self, id: String) -> Result<Budget, Error> {
        self.call(weight(&id)?, move |db| db.resource_budget(&id))
            .await
    }
    /// Host-only concrete publication command; never part of RunnerCommand.
    pub async fn resource_configuration(
        &self,
        id: String,
    ) -> Result<hagency_core::project::Resource, Error> {
        self.call(256, move |db| db.resource_configuration(&id))
            .await
    }
    pub async fn account_choices(&self) -> Result<Vec<crate::AccountChoice>, Error> {
        self.call(256, |db| db.account_choices()).await
    }
    pub async fn managed_account(&self, id: String) -> Result<crate::ManagedAccount, Error> {
        if id.len() > 128 {
            return Err(Error::Capacity);
        }
        self.call(256, move |db| db.managed_account(&id)).await
    }
    pub async fn enroll_account_resource(
        &self,
        command: crate::AccountEnrollmentCommand,
    ) -> Result<crate::ResourceConfigurationResult, Error> {
        self.call(command.weight(), move |db| {
            db.enroll_account_resource(command)
        })
        .await
    }
    pub async fn configure_resource(
        &self,
        command: crate::ResourceConfigurationCommand,
    ) -> Result<crate::ResourceConfigurationResult, Error> {
        self.call(command.weight(), move |db| db.configure_resource(command))
            .await
    }
    pub async fn publish_resource(
        &self,
        command: crate::ResourcePublicationCommand,
    ) -> Result<crate::ResourcePublicationResult, Error> {
        self.call(command.weight(), move |db| db.publish_resource(command))
            .await
    }
    pub async fn register(&self, registration: Registration) -> Result<(), Error> {
        self.call(weight(&registration)?, move |db| db.register(&registration))
            .await
    }
    pub async fn admit(&self, proof: VerifiedRequest, now: u64) -> Result<Engagement, Error> {
        self.call(
            weight(&(
                proof.request(),
                proof.registration(),
                proof.project_name(),
                proof.audit(),
            ))?,
            move |db| db.admit(&proof, now),
        )
        .await
    }
    pub async fn approve(
        &self,
        command: String,
        proof: VerifiedRequest,
        now: u64,
    ) -> Result<Engagement, Error> {
        self.call(
            weight(&(
                &command,
                proof.request(),
                proof.registration(),
                proof.project_name(),
                proof.audit(),
            ))?,
            move |db| db.approve(&command, &proof, now),
        )
        .await
    }
    pub async fn reject(&self, command: String, id: String) -> Result<Engagement, Error> {
        self.call(weight(&(&command, &id))?, move |db| {
            db.reject(&command, &id)
        })
        .await
    }
    pub async fn revoke(&self, command: String, id: String) -> Result<Engagement, Error> {
        self.call(weight(&(&command, &id))?, move |db| {
            db.revoke(&command, &id)
        })
        .await
    }
    pub async fn claim_effect(&self) -> Result<Option<Effect>, Error> {
        self.call(1, DomainRepository::claim_effect).await
    }
    pub async fn retry_cleanup(&self, command: String, id: String) -> Result<Engagement, Error> {
        self.call(weight(&(&command, &id))?, move |db| {
            db.retry_cleanup(&command, &id)
        })
        .await
    }
    pub async fn observe_effect(
        &self,
        id: String,
        fence: u64,
        outcome: EffectOutcome,
    ) -> Result<Engagement, Error> {
        self.call(weight(&(&id, &outcome))?, move |db| {
            db.observe_effect(&id, fence, &outcome)
        })
        .await
    }
}

impl DomainStore {
    /// Host-only current notice attempt check; absent from RunnerCommand.
    pub async fn validate_verified_task_notice_send(
        &self,
        id: String,
        token: String,
        fence: u64,
    ) -> Result<(), Error> {
        self.call(weight(&(&id, &token))?, move |db| {
            db.validate_verified_task_notice_send(&id, &token, fence, writer_time()?)
        })
        .await
    }
}

impl DomainStore {
    /// All usage methods are host-only; no RunnerCommand or HTTP source setter.
    pub async fn bind_usage_source(
        &self,
        cap: RunnerCapability,
        scope: crate::OwnedDispatchScope,
    ) -> Result<crate::UsageSource, Error> {
        let bytes = weight(&(&cap, scope.queue_value()))?;
        self.call(bytes, move |db| {
            db.bind_usage_clock(&cap, &scope, writer_time)
        })
        .await
    }
    pub async fn restore_usage_source(&self, id: String) -> Result<crate::UsageSource, Error> {
        self.call(weight(&id)?, move |db| db.restore_usage_source(&id))
            .await
    }
    pub async fn record_usage_observation(
        &self,
        source: crate::UsageSource,
        call_id: String,
        observation: hagency_metering::observation::UsageObservation,
    ) -> Result<crate::UsageReceipt, Error> {
        self.call(
            weight(&(source.queue_value(), &call_id, &observation))?,
            move |db| db.record_usage_clock(&source, &call_id, &observation, writer_time),
        )
        .await
    }
    pub async fn usage_source(
        &self,
        source: crate::UsageSource,
    ) -> Result<crate::SourceUsage, Error> {
        self.call(weight(&source.queue_value())?, move |db| {
            db.usage_source(&source)
        })
        .await
    }
    pub async fn usage_summary(&self, engagement: String) -> Result<crate::UsageSummary, Error> {
        self.call(weight(&engagement)?, move |db| {
            db.usage_summary(&engagement)
        })
        .await
    }
    /// One projection under the original writer queue. Query time only selects
    /// periods and never becomes observation attribution.
    pub async fn usage_report(
        &self,
        engagement: String,
        at: Option<u64>,
    ) -> Result<crate::UsageReport, Error> {
        self.call(weight(&(&engagement, at))?, move |db| {
            let at = match at {
                Some(at) => at,
                None => writer_time()?,
            };
            db.usage_report(&engagement, at)
        })
        .await
    }
    pub async fn usage_period(
        &self,
        engagement: String,
        kind: crate::UsagePeriodKind,
        at: u64,
    ) -> Result<Option<crate::UsagePeriod>, Error> {
        self.call(weight(&(&engagement, kind, at))?, move |db| {
            db.usage_period(&engagement, kind, at)
        })
        .await
    }
    pub async fn approval_room_authority(
        &self,
        engagement: String,
    ) -> Result<ApprovalRoomAuthority, Error> {
        self.call(weight(&engagement)?, move |db| {
            db.approval_room_authority(&engagement)
        })
        .await
    }
    pub async fn approval_room_capture(
        &self,
        authority: ApprovalRoomAuthority,
    ) -> Result<Option<ApprovalRoomCapture>, Error> {
        self.call(weight(&authority)?, move |db| {
            db.approval_room_capture(&authority)
        })
        .await
    }
    pub async fn fence_approval_room(
        &self,
        authority: ApprovalRoomAuthority,
        device: String,
        generation: u64,
        prior: Option<ApprovalRoomCapture>,
    ) -> Result<(), Error> {
        self.call(weight(&(&authority, &device, &prior))?, move |db| {
            db.fence_approval_room(&authority, &device, generation, prior.as_ref())
        })
        .await
    }
    pub async fn approval_intake_target(&self, id: String) -> Result<ApprovalIntakeTarget, Error> {
        self.call(weight(&id)?, move |db| {
            db.approval_intake_target(&id, writer_time()?)
        })
        .await
    }
    pub async fn approval_verdict_receipt(
        &self,
        input: ApprovalVerdictInput,
    ) -> Result<Option<ApprovalSummary>, Error> {
        self.call(weight(&input)?, move |db| {
            db.approval_verdict_receipt(&input)
        })
        .await
    }
    pub async fn admit_approval_verdict(
        &self,
        input: ApprovalVerdictInput,
    ) -> Result<ApprovalSummary, Error> {
        self.call(weight(&input)?, move |db| {
            db.admit_approval_verdict_clock(&input, writer_time)
        })
        .await
    }
}

// Host-only upload registry. These methods are deliberately absent from
// RunnerCommand and all HTTP schemas. Sample time inside BEGIN IMMEDIATE.
impl DomainStore {
    pub async fn reserve_upload(
        &self,
        cap: RunnerCapability,
        input: hagency_core::uploads::UploadRequest,
    ) -> Result<crate::UploadAdmission, Error> {
        self.call(weight(&(&cap, &input))?, move |db| {
            db.upload_transaction(writer_time, |tx, now| {
                crate::domain::uploads::reserve(tx, &cap, &input, now)
            })
        })
        .await
    }
    pub async fn restore_upload(
        &self,
        cap: RunnerCapability,
        input: hagency_core::uploads::UploadRequest,
    ) -> Result<Option<crate::UploadIdentity>, Error> {
        self.call(weight(&(&cap, &input))?, move |db| {
            db.restore_upload(&cap, &input)
        })
        .await
    }
    pub async fn restore_upload_settlement(
        &self,
        id: String,
        fence: u64,
        stage: hagency_core::uploads::StageCommitment,
        route: ReplyRoute,
    ) -> Result<Option<crate::UploadSettlement>, Error> {
        crate::domain::uploads::settlement_lookup(&id, fence, &stage, &route)?;
        self.call(weight(&(&id, fence, &stage, &route))?, move |db| {
            db.restore_upload_settlement(&id, fence, &stage, &route)
        })
        .await
    }
    pub async fn inspect_upload_settlement(
        &self,
        restored: Arc<crate::UploadSettlement>,
    ) -> Result<hagency_core::uploads::UploadReceipt, Error> {
        self.call(weight(&restored.queue_value())?, move |db| {
            db.inspect_upload_settlement(&restored)
        })
        .await
    }
    /// Arc retains the original historical handle across a lost queued result.
    /// No current execution method accepts this type.
    pub async fn record_upload_settlement(
        &self,
        restored: Arc<crate::UploadSettlement>,
        observed: hagency_core::uploads::UploadAcceptance,
    ) -> Result<hagency_core::uploads::UploadReceipt, Error> {
        observed.validate()?;
        self.call(weight(&(restored.queue_value(), &observed))?, move |db| {
            db.upload_transaction(writer_time, |tx, now| {
                crate::domain::uploads::settle(tx, &restored, &observed, now)
            })
        })
        .await
    }
    pub async fn inspect_upload(
        &self,
        id: crate::UploadIdentity,
    ) -> Result<hagency_core::uploads::UploadReceipt, Error> {
        self.call(weight(&id.queue_value())?, move |db| db.inspect_upload(&id))
            .await
    }
    pub async fn upload_stage_commitment(
        &self,
        id: crate::UploadIdentity,
    ) -> Result<Option<hagency_core::uploads::StageCommitment>, Error> {
        self.call(weight(&id.queue_value())?, move |db| {
            db.upload_stage_commitment(&id)
        })
        .await
    }
    pub async fn upload_fence(&self, id: crate::UploadIdentity) -> Result<u64, Error> {
        self.call(weight(&id.queue_value())?, move |db| db.upload_fence(&id))
            .await
    }
    pub async fn bind_upload_stage(
        &self,
        cap: RunnerCapability,
        preparation: Arc<crate::UploadPreparation>,
        stage: hagency_core::uploads::StageCommitment,
    ) -> Result<hagency_core::uploads::UploadReceipt, Error> {
        self.call(
            weight(&(&cap, preparation.queue_value(), &stage))?,
            move |db| {
                db.upload_transaction(writer_time, |tx, n| {
                    crate::domain::uploads::bind(tx, &cap, &preparation, &stage, n)
                })
            },
        )
        .await
    }
    pub async fn observe_upload_staged(
        &self,
        id: crate::UploadIdentity,
        stage: hagency_core::uploads::StageCommitment,
        observation: hagency_core::uploads::UploadStageObservation,
    ) -> Result<hagency_core::uploads::UploadReceipt, Error> {
        self.call(weight(&(id.queue_value(), &stage))? + 1, move |db| {
            db.upload_transaction(writer_time, |tx, n| {
                crate::domain::uploads::staged(tx, &id, &stage, observation, n)
            })
        })
        .await
    }
    pub async fn claim_upload(
        &self,
        cap: RunnerCapability,
        id: crate::UploadIdentity,
        lease: u64,
    ) -> Result<Option<crate::UploadClaim>, Error> {
        self.call(weight(&(&cap, id.queue_value(), lease))?, move |db| {
            db.upload_transaction(writer_time, |tx, n| {
                crate::domain::uploads::claim(tx, &cap, &id, n, lease)
            })
        })
        .await
    }
    pub async fn begin_upload(
        &self,
        cap: RunnerCapability,
        claim: crate::UploadClaim,
    ) -> Result<crate::UploadSend, Error> {
        self.call(weight(&(&cap, claim.queue_value()))?, move |db| {
            db.upload_transaction(writer_time, |tx, n| {
                crate::domain::uploads::begin(tx, &cap, &claim, n)
            })
        })
        .await
    }
    pub async fn validate_upload_send(
        &self,
        cap: RunnerCapability,
        claim: crate::UploadClaim,
    ) -> Result<(), Error> {
        self.call(weight(&(&cap, claim.queue_value()))?, move |db| {
            db.upload_transaction(writer_time, |tx, n| {
                crate::domain::uploads::validate(tx, &cap, &claim, n)
            })
        })
        .await
    }
    pub async fn record_upload_acceptance(
        &self,
        id: crate::UploadIdentity,
        fence: u64,
        stage: hagency_core::uploads::StageCommitment,
        observed: hagency_core::uploads::UploadAcceptance,
    ) -> Result<hagency_core::uploads::UploadReceipt, Error> {
        self.call(
            weight(&(id.queue_value(), fence, &stage, &observed))?,
            move |db| {
                db.upload_transaction(writer_time, |tx, n| {
                    crate::domain::uploads::accept(tx, &id, fence, &stage, &observed, n)
                })
            },
        )
        .await
    }
    pub async fn cancel_upload(
        &self,
        id: crate::UploadIdentity,
    ) -> Result<hagency_core::uploads::UploadReceipt, Error> {
        self.call(weight(&id.queue_value())?, move |db| {
            db.upload_transaction(writer_time, |tx, n| {
                crate::domain::uploads::cancel(tx, &id, n)
            })
        })
        .await
    }
    pub async fn mark_upload_uncertain(
        &self,
        id: crate::UploadIdentity,
        fence: u64,
    ) -> Result<hagency_core::uploads::UploadReceipt, Error> {
        self.call(weight(&(id.queue_value(), fence))?, move |db| {
            db.upload_transaction(writer_time, |tx, n| {
                crate::domain::uploads::uncertain(tx, &id, fence, n)
            })
        })
        .await
    }
}

#[cfg(test)]
#[path = "../tests/file_uploads/worker.rs"]
mod upload_tests;

// Separate host-only file-delivery methods. ADR096's compatible claim seam is
// independent of this block. Current time is sampled within BEGIN IMMEDIATE.
mod file_delivery_methods {
    use super::*;
    use crate::domain::file_delivery as files;
    use hagency_core::file_delivery::*;
    impl DomainStore {
        pub async fn reserve_file_delivery(
            &self,
            cap: RunnerCapability,
            input: FileDeliveryRequest,
        ) -> Result<crate::FileDeliveryAdmission, Error> {
            files::request_input(&cap, &input)?;
            self.call(weight(&(&cap, &input))?, move |db| {
                db.upload_transaction(writer_time, |tx, n| files::reserve(tx, &cap, &input, n))
            })
            .await
        }
        pub async fn restore_file_delivery(
            &self,
            cap: RunnerCapability,
            input: FileDeliveryRequest,
        ) -> Result<Option<crate::FileDeliveryIdentity>, Error> {
            files::request_input(&cap, &input)?;
            self.call(weight(&(&cap, &input))?, move |db| {
                db.restore_file_delivery(&cap, &input)
            })
            .await
        }
        pub async fn inspect_file_delivery(
            &self,
            cap: RunnerCapability,
            id: String,
        ) -> Result<FileDeliveryReceipt, Error> {
            files::cap_input(&cap)?;
            files::id_input(&id)?;
            self.call(weight(&(&cap, &id))?, move |db| {
                db.inspect_file_delivery(&cap, &id)
            })
            .await
        }
        pub async fn bind_file_delivery_stage(
            &self,
            cap: RunnerCapability,
            id: crate::FileDeliveryIdentity,
            preparation: Arc<crate::UploadPreparation>,
            captured: CapturedFile,
            stage: hagency_core::uploads::StageCommitment,
        ) -> Result<FileDeliveryReceipt, Error> {
            files::cap_input(&cap)?;
            captured.validate()?;
            stage.validate()?;
            self.call(
                weight(&(
                    &cap,
                    id.queue_value(),
                    preparation.queue_value(),
                    &captured,
                    &stage,
                ))?,
                move |db| {
                    db.upload_transaction(writer_time, |tx, n| {
                        files::bind(tx, &cap, &id, &preparation, &captured, &stage, n)
                    })
                },
            )
            .await
        }
        pub async fn claim_file_publication(
            &self,
            cap: RunnerCapability,
            id: crate::FileDeliveryIdentity,
            lease: u64,
        ) -> Result<Option<crate::FilePublicationClaim>, Error> {
            files::cap_input(&cap)?;
            self.call(weight(&(&cap, id.queue_value(), lease))?, move |db| {
                db.upload_transaction(writer_time, |tx, n| files::claim(tx, &cap, &id, n, lease))
            })
            .await
        }
        pub async fn begin_file_publication(
            &self,
            cap: RunnerCapability,
            claim: crate::FilePublicationClaim,
        ) -> Result<crate::FilePublicationSend, Error> {
            files::cap_input(&cap)?;
            self.call(weight(&(&cap, claim.queue_value()))?, move |db| {
                db.upload_transaction(writer_time, |tx, n| files::begin(tx, &cap, &claim, n))
            })
            .await
        }
        pub async fn validate_file_publication(
            &self,
            cap: RunnerCapability,
            claim: crate::FilePublicationClaim,
        ) -> Result<(), Error> {
            files::cap_input(&cap)?;
            self.call(weight(&(&cap, claim.queue_value()))?, move |db| {
                db.upload_transaction(writer_time, |tx, n| files::validate(tx, &cap, &claim, n))
            })
            .await
        }
        pub async fn cancel_file_delivery(
            &self,
            id: crate::FileDeliveryIdentity,
            reason: FileDeliveryFailure,
        ) -> Result<FileDeliveryReceipt, Error> {
            self.call(weight(&(id.queue_value(), reason))?, move |db| {
                db.upload_transaction(writer_time, |tx, n| files::cancel(tx, &id, reason, n))
            })
            .await
        }
        pub async fn mark_file_publication_uncertain(
            &self,
            id: crate::FileDeliveryIdentity,
            fence: u64,
        ) -> Result<FileDeliveryReceipt, Error> {
            self.call(weight(&(id.queue_value(), fence))?, move |db| {
                db.upload_transaction(writer_time, |tx, n| files::uncertain(tx, &id, fence, n))
            })
            .await
        }
        pub async fn restore_file_delivery_settlement(
            &self,
            input: FilePublicationLocator,
        ) -> Result<Option<crate::FileDeliverySettlement>, Error> {
            files::lookup_input(&input)?;
            self.call(weight(&input)?, move |db| {
                db.restore_file_delivery_settlement(&input)
            })
            .await
        }
        pub async fn inspect_file_delivery_settlement(
            &self,
            settlement: Arc<crate::FileDeliverySettlement>,
        ) -> Result<FileDeliveryReceipt, Error> {
            self.call(weight(&settlement.queue_value())?, move |db| {
                db.inspect_file_delivery_settlement(&settlement)
            })
            .await
        }
        /// Checks original historical content; this lookup never grants a send.
        pub async fn restore_file_delivery_settlement_for_content(
            &self,
            input: FilePublicationLocator,
            request: FileDeliveryRequest,
            captured: CapturedFile,
        ) -> Result<Option<crate::FileDeliverySettlement>, Error> {
            files::lookup_input(&input)?;
            request.validate()?;
            captured.validate()?;
            self.call(weight(&(&input, &request, &captured))?, move |db| {
                db.restore_file_delivery_settlement_for_content(&input, &request, &captured)
            })
            .await
        }
        pub async fn record_file_delivery_settlement(
            &self,
            settlement: Arc<crate::FileDeliverySettlement>,
            observed: FileDeliveryAcceptance,
        ) -> Result<FileDeliveryReceipt, Error> {
            observed.validate()?;
            self.call(weight(&(settlement.queue_value(), &observed))?, move |db| {
                db.upload_transaction(writer_time, |tx, n| {
                    files::settle(tx, &settlement, &observed, n)
                })
            })
            .await
        }
    }
}
#[cfg(test)]
#[path = "../tests/file_delivery/worker.rs"]
mod file_delivery_tests;

// Incoming local cache facts stay separate from upload and publication custody.
mod received_file_commands {
    use super::*;
    use crate::domain::received_files as received;
    use hagency_core::received_files::*;
    impl DomainStore {
        pub async fn select_receive_inbox(
            &self,
            plan: ReceiveInboxPlan,
        ) -> Result<ReceiveInboxSelection, Error> {
            plan.validate()?;
            self.call(weight(&plan)?, move |db| db.select_receive_inbox(&plan))
                .await
        }
        pub async fn visible_attachments(
            &self,
            cap: RunnerCapability,
            after: u64,
            limit: usize,
        ) -> Result<hagency_core::attachments::AttachmentPage, Error> {
            crate::domain::file_delivery::cap_input(&cap)?;
            self.call(weight(&(&cap, after, limit))?, move |db| {
                db.visible_attachments_clock(&cap, after, limit, writer_time)
            })
            .await
        }
        pub async fn reserve_received_file(
            &self,
            cap: RunnerCapability,
            event_id: String,
            limit: usize,
        ) -> Result<crate::ReceiveAdmission, Error> {
            crate::domain::file_delivery::cap_input(&cap)?;
            hagency_core::replies::matrix_event(&event_id)?;
            receive_limit(limit)?;
            self.call(weight(&(&cap, &event_id, limit))?, move |db| {
                db.upload_transaction(writer_time, |tx, n| {
                    received::reserve(tx, &cap, &event_id, limit, n)
                })
            })
            .await
        }
        pub async fn start_received_file_write(
            &self,
            cap: RunnerCapability,
            reservation: crate::ReceiveReservation,
            facts: ReceivedFileFacts,
        ) -> Result<crate::ReceiveWrite, Error> {
            crate::domain::file_delivery::cap_input(&cap)?;
            facts.validate(reservation.limit())?;
            self.call(
                weight(&(&cap, reservation.queue_value(), &facts))?,
                move |db| {
                    db.upload_transaction(writer_time, |tx, n| {
                        received::begin(tx, &cap, &reservation, &facts, n)
                    })
                },
            )
            .await
        }
        pub async fn record_received_file_ready(
            &self,
            cap: RunnerCapability,
            original: crate::ReceiveIdentity,
            facts: ReceivedFileFacts,
        ) -> Result<ReceivedFileObservation, Error> {
            crate::domain::file_delivery::cap_input(&cap)?;
            facts.validate(MAX_RECEIVED_FILE_BYTES)?;
            self.call(
                weight(&(&cap, original.queue_value(), &facts))?,
                move |db| {
                    db.upload_transaction(writer_time, |tx, n| {
                        received::ready(tx, &cap, &original, &facts, n)
                    })
                },
            )
            .await
        }
        pub async fn record_received_file_negative(
            &self,
            cap: RunnerCapability,
            original: crate::ReceiveIdentity,
            failure: ReceiveFailure,
        ) -> Result<ReceivedFileObservation, Error> {
            crate::domain::file_delivery::cap_input(&cap)?;
            self.call(
                weight(&(&cap, original.queue_value(), failure))?,
                move |db| {
                    db.upload_transaction(writer_time, |tx, _| {
                        received::negative(tx, &cap, &original, failure)
                    })
                },
            )
            .await
        }
        pub async fn inspect_received_file(
            &self,
            cap: RunnerCapability,
            id: String,
        ) -> Result<ReceivedFileObservation, Error> {
            crate::domain::file_delivery::cap_input(&cap)?;
            hagency_core::project::identifier(&id, 128)?;
            self.call(weight(&(&cap, &id))?, move |db| {
                db.inspect_received_file(&cap, &id)
            })
            .await
        }
    }
}

// The original response grants stay in their caller-owned batch across awaits.
mod approval_response_commands {
    use super::*;
    use crate::{ApprovalResponseGrant, ApprovalResponseObservation, ApprovalResponseSummary};
    impl DomainStore {
        pub async fn authorize_approval_response(
            &self,
            cap: RunnerCapability,
            id: String,
        ) -> Result<ApprovalResponseGrant, Error> {
            self.authorize_approval_response_ack(cap, id, std::future::ready(()))
                .await
        }
        // Delay only delivery of an actual committed result in negative tests.
        pub(super) async fn authorize_approval_response_ack(
            &self,
            cap: RunnerCapability,
            id: String,
            ack: impl std::future::Future<Output = ()>,
        ) -> Result<ApprovalResponseGrant, Error> {
            let grant = self
                .call(weight(&(&cap, &id))?, move |db| {
                    db.authorize_approval_response_clock(&cap, &id, writer_time)
                })
                .await?;
            ack.await;
            Ok(grant)
        }

        pub async fn begin_approval_responses(
            &self,
            cap: RunnerCapability,
            grants: &mut [ApprovalResponseGrant],
            deadline: std::time::Instant,
        ) -> Result<(), Error> {
            self.begin_approval_responses_ack(cap, grants, deadline, std::future::ready(()))
                .await
        }
        pub(super) async fn begin_approval_responses_ack(
            &self,
            cap: RunnerCapability,
            grants: &mut [ApprovalResponseGrant],
            deadline: std::time::Instant,
            ack: impl std::future::Future<Output = ()>,
        ) -> Result<(), Error> {
            let proofs = ApprovalResponseGrant::attempt(grants, deadline)?;
            let bytes = weight(&(&cap, proofs.iter().map(|p| p.weight()).collect::<Vec<_>>()))?;
            self.call(bytes, move |db| {
                db.begin_approval_responses_clock(&cap, &proofs, deadline, writer_time)
            })
            .await?;
            ack.await;
            ApprovalResponseGrant::acknowledge(grants)?;
            Ok(())
        }
        pub async fn check_approval_response(
            &self,
            cap: RunnerCapability,
            grant: &ApprovalResponseGrant,
        ) -> Result<(), Error> {
            let (proof, deadline) = grant.admitted()?;
            self.call(weight(&(&cap, proof.weight()))?, move |db| {
                db.check_approval_response_clock(&cap, &proof, deadline, writer_time)
            })
            .await?;
            grant.check_deadline()
        }
        pub async fn observe_approval_response(
            &self,
            grant: &mut ApprovalResponseGrant,
            observation: ApprovalResponseObservation,
        ) -> Result<ApprovalResponseSummary, Error> {
            let proof = grant.observation(observation)?;
            self.call(weight(&proof.weight())?, move |db| {
                db.observe_approval_response_proof(&proof, observation)
            })
            .await
        }
        pub async fn approval_response_summary(
            &self,
            id: String,
        ) -> Result<ApprovalResponseSummary, Error> {
            self.call(weight(&id)?, move |db| db.approval_response_summary(&id))
                .await
        }
    }
}

#[cfg(test)]
#[path = "../tests/approvals/responses_worker.rs"]
mod approval_response_tests;

// Separate instance-bound custody maintenance; never generic parked execution.
impl DomainStore {
    pub async fn bind_owned_approval_context(
        &self,
        cap: RunnerCapability,
        expected: String,
        context: hagency_core::approvals::HostApprovalContext,
        until: std::time::Instant,
        expires_at: u64,
    ) -> Result<crate::OwnedApprovalScope, Error> {
        self.call(
            weight(&(&cap, &expected, &context, expires_at))?,
            move |db| {
                db.bind_owned_approval_clock(
                    &cap,
                    &expected,
                    &context,
                    until,
                    expires_at,
                    writer_time,
                )
            },
        )
        .await
    }
    pub async fn maintain_owned_approval(
        &self,
        scope: &crate::OwnedApprovalScope,
    ) -> Result<crate::OwnedApprovalStatus, Error> {
        let proof = scope.proof();
        self.call(weight(&proof.weight())?, move |db| {
            db.maintain_owned_approval_clock(&proof, writer_time)
        })
        .await
    }
}

#[cfg(test)]
#[path = "../tests/approvals/owned_worker.rs"]
mod owned_approval_tests;
