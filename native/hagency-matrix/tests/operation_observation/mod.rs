use super::*;
use crate::CancellationToken;
use crate::collector::{
    Collector, fixtures as common,
    observation::{Phase, SdkCommand, Trace, observed},
};

#[tokio::test]
async fn native_matrix_operation_observation_queued_owner() {
    let root = tempfile::tempdir().unwrap();
    let config = super::tests::config(root.path());
    let owner = Owner::open(&config).await.unwrap();
    let sql = rusqlite::Connection::open(config.root.join(DATABASES[0])).unwrap();
    sql.execute_batch("BEGIN IMMEDIATE").unwrap();
    let first = Trace::new("held original sync", Some("caller dropped"), None);
    let mut held = first.hold(Phase::Started);
    let mut write = Box::pin(observed(
        first.clone(),
        owner.sync(common::sync("retained-original")),
    ));
    tokio::select! {
        _ = held.reached() => {},
        result = &mut write => panic!("original write escaped held owner: {result:?}"),
    }
    first.wait(Phase::Queued).await;
    drop(write);
    let second = Trace::new("queued original cursor", None, None);
    let mut read = Box::pin(observed(second.clone(), owner.cursor()));
    tokio::select! {
        _ = second.wait(Phase::Queued) => {},
        result = &mut read => panic!("queued read escaped held owner: {result:?}"),
    }
    assert!(!second.has(Phase::Started));
    drop(read);
    assert!(matches!(Owner::open(&config).await, Err(Error::Busy)));
    held.release();
    sql.execute_batch("COMMIT").unwrap();
    drop(sql);
    first.wait(Phase::Returned).await;
    second.wait(Phase::Returned).await;
    for (trace, command, label) in [
        (&first, SdkCommand::Sync, "held original sync"),
        (&second, SdkCommand::Cursor, "queued original cursor"),
    ] {
        let snapshot = trace.snapshot();
        assert_eq!(snapshot.callsite, label);
        assert_eq!(snapshot.batch, None);
        assert!(
            snapshot.primary.is_none()
                && snapshot.fence.is_none()
                && snapshot.sdk_failure.is_none()
        );
        assert!(!trace.has(Phase::CallerReturned));
        assert!(!trace.has(Phase::OperationReturned));
        let events = snapshot.events.iter().flatten().collect::<Vec<_>>();
        assert_eq!(events.len(), 3);
        assert!(
            events
                .windows(2)
                .all(|pair| pair[0].elapsed_us <= pair[1].elapsed_us)
        );
        assert!(events.iter().all(|e| e.command == Some(command)
            && e.sequence == 1
            && e.index.is_none()
            && e.error.is_none()));
        assert!(trace.has(Phase::Started));
    }
    assert_eq!(first.snapshot().variant, Some("caller dropped"));
    assert_eq!(second.snapshot().variant, None);
    assert_eq!(
        owner.cursor().await.unwrap().as_deref(),
        Some("retained-original")
    );
    owner.close().await.unwrap();
    let owner = Owner::open(&config).await.unwrap();
    assert_eq!(
        owner.cursor().await.unwrap().as_deref(),
        Some("retained-original")
    );
    owner.close().await.unwrap();
}

#[tokio::test]
async fn native_matrix_operation_observation_owner_lifecycle() {
    let root = tempfile::tempdir().unwrap();
    let config = super::tests::config(root.path());
    let opening = Trace::new("held original open", None, None);
    let mut held = opening.hold(Phase::Prepared);
    let mut open = Box::pin(observed(opening.clone(), Owner::open(&config)));
    tokio::select! {
        _ = held.reached() => {},
        _ = &mut open => panic!("original open escaped held owner"),
    }
    assert!(opening.has(Phase::PrepareStarted));
    assert!(!opening.has(Phase::SdkOpenStarted));
    let rejected = Trace::new("held owner's competing open", None, None);
    assert!(matches!(
        observed(rejected.clone(), Owner::open(&config)).await,
        Err(Error::Busy)
    ));
    assert!(rejected.has(Phase::PrepareFailed));
    assert!(!rejected.has(Phase::SdkOpenStarted));
    assert!(!rejected.has(Phase::SdkOpenReturned));
    held.release();
    let owner = open.await.unwrap();
    for phase in [
        Phase::StateStoreOpen,
        Phase::CryptoStoreOpen,
        Phase::Activate,
        Phase::JournalLoad,
        Phase::SdkOpenReturned,
        Phase::OpenCallerReturned,
    ] {
        assert!(opening.has(phase), "missing original open phase {phase:?}");
    }
    let (send, reply) = oneshot::channel();
    owner
        .tx
        .try_send(Command::CloseFault(send))
        .unwrap_or_else(|_| panic!("fixture close fault queue"));
    reply.await.unwrap();
    let closing = Trace::new("held original close", None, None);
    let mut held = closing.hold(Phase::RuntimeDropStarted);
    let mut close = Box::pin(observed(closing.clone(), owner.close()));
    tokio::select! {
        _ = held.reached() => {},
        result = &mut close => panic!("original close escaped held owner: {result:?}"),
    }
    assert!(closing.has(Phase::CloseStoresReturned));
    assert!(!closing.has(Phase::LockDropped));
    assert!(!closing.has(Phase::CloseAcknowledgement));
    assert!(matches!(Owner::open(&config).await, Err(Error::Busy)));
    held.release();
    assert_eq!(close.await, Err(Error::OutcomeUnknown));
    let snapshot = closing.snapshot();
    assert_eq!(
        snapshot.sdk_failure.unwrap().error,
        Some(Error::OutcomeUnknown)
    );
    let events = snapshot.events.iter().flatten().collect::<Vec<_>>();
    let position = |phase| events.iter().position(|e| e.phase == phase).unwrap();
    assert!(position(Phase::CloseStoresReturned) < position(Phase::RuntimeDropped));
    assert!(position(Phase::RuntimeDropped) < position(Phase::LockDropped));
    assert!(position(Phase::LockDropped) < position(Phase::CloseAcknowledgement));
    assert!(position(Phase::CloseAcknowledgement) < position(Phase::CallerReturned));
    assert_eq!(
        events[position(Phase::CallerReturned)].error,
        Some(Error::OutcomeUnknown)
    );
    let owner = Owner::open(&config).await.unwrap();
    owner.close().await.unwrap();
}

#[tokio::test]
async fn native_matrix_operation_observation_primary_fence() {
    let f = common::Fixture::new();
    let mut fake = common::Fake::start(false).await;
    f.store
        .observe_matrix_transport(f.identity.transport.clone())
        .await
        .unwrap();
    let c = Collector::new(f.config(&fake.endpoint), f.store.clone()).unwrap();
    let trace = Trace::new("original refusal with failed fence", None, None);
    let cancel = CancellationToken::new();
    let result = observed(trace.clone(), async {
        let (result, ()) = common::scripted(c.collect(&cancel), async {
            let request = fake.next().await;
            // The actual collector already read its expected transport before whoami.
            common::shutdown_domain(&f.store, "primary-fence writer shutdown").await;
            let mut who = common::who();
            who["device_id"] = serde_json::json!("WRONG_DEVICE");
            request.json(200, who);
        })
        .await;
        result
    })
    .await;
    assert_eq!(result, Err(Error::OutcomeUnknown));
    let snapshot = trace.snapshot();
    assert_eq!(snapshot.primary.unwrap().error, Some(Error::Identity));
    assert_eq!(snapshot.fence.unwrap().error, Some(Error::Domain));
    assert!(trace.has(Phase::Whoami));
    assert!(!trace.has(Phase::OpenOwner));
    assert_eq!(c.close().await, Err(Error::Domain));
    fake.close().await;
}

#[tokio::test]
async fn native_matrix_operation_observation_intake_subphases() {
    use hagency_core::tasks::SessionBinding;

    for (variant, hold) in [
        ("prepared persisted", Some(Phase::IntakePreparedPersisted)),
        ("SDK apply", Some(Phase::IntakeSyncApply)),
        ("derived persist", Some(Phase::IntakeDerivedPersist)),
        ("write refused", None),
    ] {
        let f = common::Fixture::new();
        let mut fake = common::Fake::start(true).await;
        let config = f
            .config(&fake.endpoint)
            .with_root_pem(include_bytes!("../fixtures/ca.pem"))
            .unwrap();
        let c = Collector::new(config, f.store.clone()).unwrap();
        let config = &c.inner.config;
        let cancel = CancellationToken::new();
        let (result, ()) =
            common::scripted(c.collect(&cancel), common::success(&mut fake, "bootstrap")).await;
        result.unwrap();
        f.store
            .resolve_verified_matrix_session(SessionBinding {
                id: "root".into(),
                engagement_id: f.identity.transport.engagement_id.clone(),
                room_id: "!direct:example.test".into(),
                thread_root: None,
            })
            .await
            .unwrap();
        let targets = vec![f.store.matrix_intake_route("root".into()).await.unwrap()];
        let guard = c.inner.owner.lock().await;
        let owner = guard.as_ref().unwrap();
        let trace = Trace::new("original intake subphases", Some(variant), None);
        let raw = common::sync("retained-intake");
        if let Some(phase) = hold {
            let mut held = trace.hold(phase);
            let mut start = Box::pin(observed(
                trace.clone(),
                owner.intake_start(raw.clone(), targets),
            ));
            tokio::select! {
                _ = held.reached() => {},
                result = &mut start => panic!("original intake escaped held phase: {result:?}"),
            }
            trace.wait(Phase::Queued).await;
            assert!(!trace.has(Phase::Returned));
            drop(start);
            let queued = Trace::new("separate original batch", None, None);
            let mut read = Box::pin(observed(queued.clone(), owner.batch()));
            tokio::select! {
                _ = queued.wait(Phase::Queued) => {},
                _ = &mut read => panic!("queued batch escaped held original intake"),
            }
            assert!(!queued.has(Phase::Started));
            drop(read);
            assert!(matches!(Owner::open(config).await, Err(Error::Busy)));
            held.release();
            trace.wait(Phase::Returned).await;
            queued.wait(Phase::Returned).await;
            assert!(!trace.has(Phase::CallerReturned));
            assert!(!trace.has(Phase::OperationReturned));
            let snapshot = trace.snapshot();
            assert_eq!(snapshot.variant, Some(variant));
            assert!(snapshot.sdk_failure.is_none());
            let events = snapshot.events.iter().flatten().collect::<Vec<_>>();
            assert!(
                events
                    .iter()
                    .all(|event| event.command == Some(SdkCommand::Start)
                        && event.sequence == 1
                        && event.error.is_none())
            );
            let position = |phase| events.iter().position(|e| e.phase == phase).unwrap();
            for (before, after) in [
                (Phase::IntakePreparedPersist, Phase::IntakePreparedPersisted),
                (Phase::IntakeApplyingPersist, Phase::IntakeApplyingPersisted),
                (Phase::IntakeSyncApply, Phase::IntakeSyncApplied),
                (Phase::IntakeDerivedPersist, Phase::IntakeDerivedPersisted),
                (Phase::IntakeFinalFilesCheck, Phase::IntakeFinalFilesChecked),
                (Phase::IntakeFinalFilesChecked, Phase::Returned),
            ] {
                assert!(position(before) < position(after));
                assert!(events[position(before)].elapsed_us <= events[position(after)].elapsed_us);
            }
            let other = queued.snapshot();
            let events = other.events.iter().flatten().collect::<Vec<_>>();
            assert_eq!(events.len(), 3);
            assert!(
                events
                    .iter()
                    .all(|event| event.command == Some(SdkCommand::Batch)
                        && event.sequence == 1
                        && event.error.is_none())
            );
            assert!(!queued.has(Phase::IntakePreparedPersist));
            let batch = owner.batch().await.unwrap().unwrap();
            assert!(batch.phase == crate::event_batch::Phase::Derived);
            assert_eq!(batch.raw, raw);
            assert_eq!(batch.targets.len(), 1);
        } else {
            let sql = rusqlite::Connection::open(config.root.join(DATABASES[0])).unwrap();
            sql.execute_batch("CREATE TRIGGER original_intake_abort BEFORE INSERT ON kv_blob BEGIN SELECT RAISE(ABORT,'original fixture write refusal'); END;").unwrap();
            assert_eq!(
                observed(trace.clone(), owner.intake_start(raw.clone(), targets)).await,
                Err(Error::OutcomeUnknown)
            );
            assert!(trace.has(Phase::IntakePreparedPersist));
            assert!(!trace.has(Phase::IntakePreparedPersisted));
            assert!(!trace.has(Phase::IntakeApplyingPersist));
            assert!(!trace.has(Phase::IntakeSyncApply));
            let snapshot = trace.snapshot();
            let failure = snapshot.sdk_failure.unwrap();
            assert_eq!(failure.phase, Phase::Returned);
            assert_eq!(failure.command, Some(SdkCommand::Start));
            assert_eq!(failure.sequence, 1);
            assert_eq!(failure.error, Some(Error::OutcomeUnknown));
            sql.execute_batch("DROP TRIGGER original_intake_abort")
                .unwrap();
            drop(sql);
        }
        drop(guard);
        c.close().await.unwrap();
        let owner = Owner::open(config).await.unwrap();
        let restored = owner.batch().await.unwrap();
        if hold.is_some() {
            let batch = restored.unwrap();
            assert!(batch.phase == crate::event_batch::Phase::Derived);
            assert_eq!(batch.raw, raw);
            assert_eq!(batch.targets.len(), 1);
        } else {
            assert!(restored.is_none());
            assert_eq!(owner.cursor().await.unwrap().as_deref(), Some("bootstrap"));
        }
        owner.close().await.unwrap();
        common::shutdown_domain(&f.store, "original intake subphase fixture").await;
        fake.no_request().await;
        fake.close().await;
    }
}
