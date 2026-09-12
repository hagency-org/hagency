use hagency_store::{DomainRepository, DomainStore, Error, ShutdownOutcome};

#[tokio::test]
async fn native_domain_shutdown_success() {
    for observed in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("state");
        let store = DomainStore::start(DomainRepository::open(&state).unwrap(), 1).unwrap();
        assert!(matches!(DomainRepository::open(&state), Err(Error::Locked)));
        if observed {
            let (result, snapshot) = store.shutdown_observed().await;
            result.unwrap();
            assert_eq!(snapshot.outcome, ShutdownOutcome::Complete);
            let started = snapshot.drop_started_us.unwrap();
            let finished = snapshot.drop_finished_us.unwrap();
            assert!(finished >= started);
            assert!(snapshot.caller_finished_us.unwrap() >= finished);
            assert!(snapshot.acknowledgement_started_us.is_some());
            // No total ordering is required between independent observers:
            // the worker may already run before enqueue is observed, and the
            // caller may receive before the sender publishes its sent marker.
            assert!(snapshot.enqueue_observed_us.is_some());
        } else {
            store.shutdown().await.unwrap();
        }
        DomainRepository::open(&state).unwrap();
        let (result, snapshot) = store.shutdown_observed().await;
        assert!(matches!(result, Err(Error::Unavailable)));
        // ACK precedes receiver destruction: a second send may observe the
        // closed queue, or enter that queue just before its receiver is dropped.
        assert!(matches!(
            snapshot.outcome,
            ShutdownOutcome::EnqueueClosed | ShutdownOutcome::ReplyClosed
        ));
        assert_eq!(snapshot.worker_picked_up_us, None);
    }
}
