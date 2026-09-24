use super::*;
use std::time::Instant;

fn setup(root: &std::path::Path) -> (DomainRepository, crate::OwnedApprovalScope, u64) {
    let (mut db, cap, context) = clock_tests::approval_fixture_mode(root, false);
    let original = db
        .owned_dispatch_scope(&cap, writer_time().unwrap())
        .unwrap();
    db.start_owned_dispatch(&cap, original.fingerprint(), writer_time().unwrap())
        .unwrap();
    let end = Instant::now() + Duration::from_secs(1);
    let wall = writer_time().unwrap() + 1000;
    let scope = db
        .bind_owned_approval_context(
            &cap,
            original.fingerprint(),
            &context,
            end,
            wall,
            writer_time().unwrap(),
        )
        .unwrap();
    db.request_owner_approval(
        &cap,
        &clock_tests::approval_input(wall),
        writer_time().unwrap(),
    )
    .unwrap();
    (db, scope, wall)
}

#[tokio::test]
async fn native_owned_approval_maintenance_clock() {
    for mode in ["transaction", "lock", "queue"] {
        let root = tempfile::tempdir().unwrap();
        let (mut db, scope, wall) = setup(root.path());
        let inspect = rusqlite::Connection::open(root.path().join("state/domain.sqlite3")).unwrap();
        if mode == "transaction" {
            inspect.busy_timeout(Duration::ZERO).unwrap();
            db.maintain_owned_approval_clock(&scope.proof(), || {
                assert!(matches!(inspect.execute_batch("BEGIN IMMEDIATE"), Err(rusqlite::Error::SqliteFailure(e, _)) if e.code == rusqlite::ErrorCode::DatabaseBusy));
                writer_time()
            }).unwrap();
            continue;
        }
        let before: u64 = inspect
            .query_row(
                "SELECT lease_until FROM runner_dispatches WHERE id='dispatch'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let store = DomainStore::start(db, 16).unwrap();
        if mode == "lock" {
            // Approach the original captured expiry; hold the actual SQLite
            // writer lock for less than its unchanged 100 ms busy allowance.
            tokio::time::sleep(Duration::from_millis(
                wall.saturating_sub(writer_time().unwrap())
                    .saturating_sub(50),
            ))
            .await;
            inspect.execute_batch("BEGIN IMMEDIATE").unwrap();
            let command = store.maintain_owned_approval(&scope);
            tokio::pin!(command);
            assert!(
                tokio::time::timeout(Duration::from_millis(10), &mut command)
                    .await
                    .is_err()
            );
            tokio::time::sleep(Duration::from_millis(
                wall.saturating_sub(writer_time().unwrap()) + 10,
            ))
            .await;
            inspect.execute_batch("COMMIT").unwrap();
            assert!(matches!(command.await, Err(Error::RunnerAuthority)));
        } else {
            let (entered, ready) = oneshot::channel();
            let (release, gate) = std::sync::mpsc::channel();
            let blocking = store.clone();
            let blocker = tokio::spawn(async move {
                blocking
                    .call(1, move |_| {
                        let _ = entered.send(());
                        gate.recv_timeout(Duration::from_secs(2))
                            .map_err(|_| Error::Unavailable)?;
                        Ok(())
                    })
                    .await
            });
            ready.await.unwrap();
            let command = store.maintain_owned_approval(&scope);
            tokio::pin!(command);
            assert!(
                tokio::time::timeout(Duration::from_millis(10), &mut command)
                    .await
                    .is_err()
            );
            assert_eq!(store.tx.capacity(), 15);
            tokio::time::sleep(Duration::from_millis(
                wall.saturating_sub(writer_time().unwrap()) + 10,
            ))
            .await;
            release.send(()).unwrap();
            blocker.await.unwrap().unwrap();
            assert!(matches!(command.await, Err(Error::RunnerAuthority)));
        }
        let after: u64 = inspect
            .query_row(
                "SELECT lease_until FROM runner_dispatches WHERE id='dispatch'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(before, after);
        store.shutdown().await.unwrap();
    }
}
