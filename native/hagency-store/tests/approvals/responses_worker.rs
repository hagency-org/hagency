use super::*;
use crate::{ApprovalResponseGrant, ApprovalResponseState};
use hagency_core::approvals::*;
use std::{path::Path, time::Instant};

fn setup(root: &Path) -> (DomainRepository, RunnerCapability, String) {
    let (mut db, cap, context) = clock_tests::approval_fixture(root);
    db.bind_approval_context(&cap, &context, writer_time().unwrap())
        .unwrap();
    let input = clock_tests::approval_input(writer_time().unwrap() + 60_000);
    let request = db
        .request_owner_approval(&cap, &input, writer_time().unwrap())
        .unwrap();
    let private = db
        .private_approval(&request.id, writer_time().unwrap())
        .unwrap();
    db.observe_owner_verdict(
        &OwnerVerdictObservation {
            request_id: request.id.clone(),
            request_digest: private.digest,
            binding_generation: private.binding_generation,
            server_name: "example.test".into(),
            room_id: private.room_id,
            sender_mxid: private.owner_mxid,
            event_id: "$response".into(),
            encrypted: true,
            choice: ApprovalChoice::Once,
        },
        writer_time().unwrap(),
    )
    .unwrap();
    (db, cap, request.id)
}
fn state(sql: &rusqlite::Connection) -> String {
    sql.query_row(
        "SELECT state FROM runner_dispatches WHERE id='dispatch'",
        [],
        |r| r.get(0),
    )
    .unwrap()
}

#[tokio::test]
async fn native_approval_response_caller_loss() {
    for mode in [
        "consume_ack",
        "queued_begin",
        "begin_ack",
        "late_begin_ack",
        "write_receipt_loss",
    ] {
        let root = tempfile::tempdir().unwrap();
        let (mut db, cap, id) = setup(root.path());
        let sql = rusqlite::Connection::open(root.path().join("state/domain.sqlite3")).unwrap();
        let original = if mode == "consume_ack" {
            None
        } else {
            Some(
                db.authorize_approval_response(&cap, &id, writer_time().unwrap())
                    .unwrap(),
            )
        };
        let store = DomainStore::start(db, 16).unwrap();
        if mode == "consume_ack" {
            let (entered, ready) = oneshot::channel();
            let mut request = Box::pin(store.authorize_approval_response_ack(
                cap.clone(),
                id.clone(),
                async move {
                    let _ = entered.send(());
                    std::future::pending::<()>().await;
                },
            ));
            tokio::select! { _=ready=>{}, _=&mut request=>panic!("actual consumed grant must remain withheld") }
            drop(request);
            assert_eq!(
                store
                    .approval_response_summary(id.clone())
                    .await
                    .unwrap()
                    .response,
                ApprovalResponseState::Authorized
            );
            assert_eq!(state(&sql), "parked");
            assert!(store.authorize_approval_response(cap, id).await.is_err());
        } else {
            let mut batch = [original.unwrap()];
            if mode == "queued_begin" || mode == "write_receipt_loss" {
                if mode == "write_receipt_loss" {
                    store
                        .begin_approval_responses(
                            cap.clone(),
                            &mut batch,
                            Instant::now() + Duration::from_secs(1),
                        )
                        .await
                        .unwrap();
                }
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
                let mut request = Box::pin(async {
                    if mode == "write_receipt_loss" {
                        // Domain host-observation double, not physical wire proof.
                        store
                            .observe_approval_response(
                                &mut batch[0],
                                crate::ApprovalResponseObservation::WriteAccepted,
                            )
                            .await
                            .map(|_| ())
                    } else {
                        store
                            .begin_approval_responses(
                                cap.clone(),
                                &mut batch,
                                Instant::now() + Duration::from_secs(1),
                            )
                            .await
                    }
                });
                assert!(
                    tokio::time::timeout(Duration::from_millis(10), &mut request)
                        .await
                        .is_err()
                );
                assert_eq!(store.tx.capacity(), 15);
                drop(request);
                release.send(()).unwrap();
                blocker.await.unwrap().unwrap();
                assert_eq!(
                    store
                        .approval_response_summary(id.clone())
                        .await
                        .unwrap()
                        .response,
                    if mode == "write_receipt_loss" {
                        ApprovalResponseState::ResponseMaySend
                    } else {
                        ApprovalResponseState::Authorized
                    }
                );
                assert_eq!(
                    state(&sql),
                    if mode == "write_receipt_loss" {
                        "started"
                    } else {
                        "parked"
                    }
                );
            } else {
                let (entered, ready) = oneshot::channel();
                let until = Instant::now() + Duration::from_secs(1);
                let mut request = Box::pin(store.begin_approval_responses_ack(
                    cap.clone(),
                    &mut batch,
                    until,
                    async move {
                        let _ = entered.send(());
                        if mode == "late_begin_ack" {
                            tokio::time::sleep_until(until.into()).await;
                        } else {
                            std::future::pending::<()>().await;
                        }
                    },
                ));
                tokio::select! {_=ready=>{}, _=&mut request=>panic!("actual begin acknowledgement must remain withheld")}
                if mode == "late_begin_ack" {
                    assert!(matches!(
                        request.as_mut().await,
                        Err(Error::RunnerAuthority)
                    ));
                }
                drop(request);
                assert_eq!(
                    store
                        .approval_response_summary(id.clone())
                        .await
                        .unwrap()
                        .response,
                    ApprovalResponseState::ResponseMaySend
                );
                assert_eq!(state(&sql), "started");
            }
            assert!(!batch[0].is_admitted());
            assert!(
                store
                    .begin_approval_responses(
                        cap.clone(),
                        &mut batch,
                        Instant::now() + Duration::from_secs(2)
                    )
                    .await
                    .is_err()
            );
            assert!(store.check_approval_response(cap, &batch[0]).await.is_err());
        }
        store.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn native_approval_response_clock() {
    let root = tempfile::tempdir().unwrap();
    let (mut db, cap, id) = setup(root.path());
    let inspect = rusqlite::Connection::open(root.path().join("state/domain.sqlite3")).unwrap();
    inspect.busy_timeout(Duration::ZERO).unwrap();
    let clock = || {
        let failure = inspect.execute_batch("BEGIN IMMEDIATE").unwrap_err();
        assert_eq!(
            failure.sqlite_error_code(),
            Some(rusqlite::ErrorCode::DatabaseBusy)
        );
        writer_time()
    };
    let mut grant = [db
        .authorize_approval_response_clock(&cap, &id, clock)
        .unwrap()];
    let until = Instant::now() + Duration::from_secs(2);
    let proofs = ApprovalResponseGrant::attempt(&mut grant, until).unwrap();
    // Duplicate/missing proofs cannot bypass the public non-Clone grant boundary.
    assert!(
        db.begin_approval_responses_clock(
            &cap,
            &[proofs[0].clone(), proofs[0].clone()],
            until,
            clock
        )
        .is_err()
    );
    db.begin_approval_responses_clock(&cap, &proofs, until, clock)
        .unwrap();
    ApprovalResponseGrant::acknowledge(&mut grant).unwrap();
    db.check_approval_response_clock(&cap, &proofs[0], until, clock)
        .unwrap();
    let saved = grant[0].deadline();
    assert!(
        db.check_approval_response_clock(&cap, &proofs[0], Instant::now(), clock)
            .is_err()
    );
    assert_eq!(grant[0].deadline(), saved);

    for expiry in ["deadline", "lease"] {
        let root = tempfile::tempdir().unwrap();
        let (mut db, cap, id) = setup(root.path());
        let grant = db
            .authorize_approval_response(&cap, &id, writer_time().unwrap())
            .unwrap();
        let at = writer_time().unwrap();
        if expiry == "lease" {
            db.renew_dispatch(&cap, at, 1000).unwrap();
        }
        let mut sql = rusqlite::Connection::open(root.path().join("state/domain.sqlite3")).unwrap();
        let store = DomainStore::start(db, 16).unwrap();
        // Finish setup, then use only the final 60 ms of the unchanged 100 ms
        // SQLite busy window. No authority row is changed by this inspector.
        let expiry_at = at + 1000;
        tokio::time::sleep(Duration::from_millis(
            expiry_at
                .saturating_sub(writer_time().unwrap())
                .saturating_sub(60),
        ))
        .await;
        let lock = sql
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .unwrap();
        let mut batch = [grant];
        let until = if expiry == "deadline" {
            Instant::now() + Duration::from_millis(40)
        } else {
            Instant::now() + Duration::from_secs(1)
        };
        let mut request = Box::pin(store.begin_approval_responses(cap.clone(), &mut batch, until));
        assert!(
            tokio::time::timeout(Duration::from_millis(5), &mut request)
                .await
                .is_err()
        );
        assert!(writer_time().unwrap() < expiry_at);
        tokio::time::sleep(Duration::from_millis(
            expiry_at.saturating_sub(writer_time().unwrap()) + 5,
        ))
        .await;
        lock.commit().unwrap();
        assert!(matches!(request.await, Err(Error::RunnerAuthority)));
        assert!(!batch[0].is_admitted());
        assert_eq!(state(&sql), "parked");
        store.shutdown().await.unwrap();
    }
}

/// A caller whose reply wait expires leaves the row unrecorded, and the ordered
/// reconcile read proves it: because the acceptance command's caller dropped its
/// receiver, the queue skips that enqueued job, so it can never execute after the
/// negative read. This is the store-side half of the reconcile contract; the
/// execution crate's two branches are proven in its own suite.
#[tokio::test]
async fn native_domain_acceptance_reply_timeout_reconciles() {
    let root = tempfile::tempdir().unwrap();
    let (mut db, cap, id) = setup(root.path());
    let grant = db
        .authorize_approval_response(&cap, &id, writer_time().unwrap())
        .unwrap();
    let store = DomainStore::start(db, 16).unwrap();
    let mut batch = [grant];
    store
        .begin_approval_responses(
            cap.clone(),
            &mut batch,
            Instant::now() + Duration::from_secs(2),
        )
        .await
        .unwrap();
    assert!(batch[0].is_admitted());
    assert_eq!(
        store
            .approval_response_summary(id.clone())
            .await
            .unwrap()
            .response,
        ApprovalResponseState::ResponseMaySend
    );
    // Occupy the single writer so the acceptance command provably cannot run.
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
        .unwrap_or_else(|_| panic!("parking operation was not admitted"));
    tokio::time::timeout(Duration::from_secs(2), reached)
        .await
        .unwrap()
        .unwrap();
    // Enqueued behind the parked job, the acceptance observation's own
    // two-second reply wait expires while the writer is provably occupied.
    let failure = store
        .observe_approval_response(
            &mut batch[0],
            crate::ApprovalResponseObservation::WriteAccepted,
        )
        .await
        .unwrap_err();
    assert!(matches!(failure, Error::OutcomeUnknown), "{failure:?}");
    // Attributed as still queued, never dequeued: nothing ran for the command.
    assert_eq!(store.last_unknown_dequeued(), Some(false));
    // Release the writer. The abandoned acceptance job is skipped because its
    // caller dropped the receiver, so the ordered reconcile read must find that
    // no acceptance was ever recorded. If this row turns out accepted, the skip
    // is not effective for this path and that is a finding, not a weakening.
    resume.send(()).unwrap();
    let summary = store.approval_response_summary(id.clone()).await.unwrap();
    assert_eq!(
        summary.response,
        ApprovalResponseState::ResponseMaySend,
        "an abandoned acceptance job must not execute after the negative read"
    );
    assert!(!summary.write_accepted);
    store.shutdown().await.unwrap();
}
