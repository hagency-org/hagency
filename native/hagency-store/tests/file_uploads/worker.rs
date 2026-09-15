use super::*;
use clock_fixtures::{proof, registration, request, resource};
use hagency_core::{attachments::AttachmentMetadata, uploads::*};
use serde_json::json;
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
fn fixture(root: &std::path::Path) -> (DomainRepository, RunnerCapability) {
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
            encrypted: true,
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
    db.enqueue_dispatch(&DispatchInput {
        id: "dispatch".into(),
        session_id: "session".into(),
        task_id: Some("task".into()),
        resources: vec![hagency_core::tasks::ResourceLease {
            id: "work".into(),
            exclusive: true,
        }],
        payload: json!({"instruction":"offline"}),
    })
    .unwrap();
    let cap = db
        .claim_dispatch("host", now(), 60_000, 60_000, 1)
        .unwrap()
        .unwrap();
    db.start_dispatch(&cap, now()).unwrap();
    (db, cap)
}

fn input(call: &str) -> UploadRequest {
    UploadRequest {
        call_id: call.into(),
        request_digest: "1".repeat(64),
        metadata: AttachmentMetadata {
            filename: "result.txt".into(),
            mime_type: None,
            declared_size: Some(12),
        },
    }
}
fn stage() -> StageCommitment {
    StageCommitment {
        namespace_digest: "2".repeat(64),
        operation_id: "stage".into(),
        receipt_digest: "3".repeat(64),
        len: 12,
    }
}
async fn ready(store: &DomainStore, cap: &RunnerCapability, call: &str) -> crate::UploadIdentity {
    let a = store
        .reserve_upload(cap.clone(), input(call))
        .await
        .unwrap();
    store
        .bind_upload_stage(cap.clone(), Arc::new(a.preparation.unwrap()), stage())
        .await
        .unwrap();
    store
        .observe_upload_staged(
            a.identity.clone(),
            stage(),
            UploadStageObservation::FileAndDirectorySynced,
        )
        .await
        .unwrap();
    a.identity
}

#[tokio::test]
async fn native_upload_worker_lost_and_concurrent() {
    let root = tempfile::tempdir().unwrap();
    let (db, cap) = fixture(root.path());
    let store = DomainStore::start(db, 8).unwrap();
    // Drop the actual successful committed return on the owner worker. This is
    // response-loss evidence, not a mocked success or inferred rollback.
    let (done, wait) = oneshot::channel();
    let original = cap.clone();
    store
        .tx
        .try_send(Job::Run {
            operation: Box::new(move |db| {
                let admitted = db
                    .upload_transaction(writer_time, |tx, n| {
                        crate::domain::uploads::reserve(tx, &original, &input("lost"), n)
                    })
                    .unwrap();
                assert!(admitted.preparation.is_some());
                drop(admitted);
                let _ = done.send(());
            }),
            _bytes: store.bytes.clone().try_acquire_many_owned(2048).unwrap(),
        })
        .unwrap_or_else(|_| panic!("fixture admission"));
    tokio::time::timeout(Duration::from_secs(2), wait)
        .await
        .unwrap()
        .unwrap();
    let restored = store
        .restore_upload(cap.clone(), input("lost"))
        .await
        .unwrap()
        .unwrap();
    let replay = store
        .reserve_upload(cap.clone(), input("lost"))
        .await
        .unwrap();
    assert!(replay.preparation.is_none());
    assert_eq!(replay.identity.id(), restored.id());
    let id = ready(&store, &cap, "race").await;
    let (a, b) = tokio::join!(
        store.claim_upload(cap.clone(), id.clone(), 60000),
        store.claim_upload(cap.clone(), id.clone(), 60000)
    );
    let mut claims = [a.unwrap(), b.unwrap()];
    assert_eq!(claims.iter().flatten().count(), 1);
    let c = claims.iter_mut().find_map(Option::take).unwrap();
    let (a, b) = tokio::join!(
        store.begin_upload(cap.clone(), c.clone()),
        store.begin_upload(cap.clone(), c.clone())
    );
    assert_eq!([a.is_ok(), b.is_ok()].into_iter().filter(|x| *x).count(), 1);
    drop(a);
    drop(b); // Lose the successful begin return; never recreate its grant.
    assert_eq!(
        store.inspect_upload(id.clone()).await.unwrap().upload,
        UploadState::WritePossible
    );
    store
        .mark_upload_uncertain(id.clone(), c.fence())
        .await
        .unwrap();
    assert!(
        store
            .claim_upload(cap.clone(), id.clone(), 60000)
            .await
            .unwrap()
            .is_none()
    );
    store.shutdown().await.unwrap();
    let db = DomainRepository::open(&root.path().join("state")).unwrap();
    assert_eq!(
        db.inspect_upload(&id).unwrap().upload,
        UploadState::WritePossible
    );
    assert!(db.inspect_upload(&id).unwrap().outcome_unknown);
}

#[tokio::test]
async fn native_upload_worker_queued_expiry() {
    let root = tempfile::tempdir().unwrap();
    let (db, cap) = fixture(root.path());
    let store = DomainStore::start(db, 8).unwrap();
    let id = ready(&store, &cap, "expired").await;
    let c = store
        .claim_upload(cap.clone(), id.clone(), 60000)
        .await
        .unwrap()
        .unwrap();
    let (entered, reached) = oneshot::channel();
    let (resume, paused) = std::sync::mpsc::channel();
    let path = root.path().join("state/domain.sqlite3");
    store
        .tx
        .try_send(Job::Run {
            operation: Box::new(move |_db| {
                // Keep ownership while a real caller queues, then let its persisted
                // lease expire before the queued transaction samples writer time.
                rusqlite::Connection::open(&path)
                    .unwrap()
                    .execute(
                        "UPDATE runner_dispatches SET lease_until=?1 WHERE id='dispatch'",
                        [now() + 30],
                    )
                    .unwrap();
                let _ = entered.send(());
                paused.recv_timeout(Duration::from_secs(2)).unwrap();
            }),
            _bytes: store.bytes.clone().try_acquire_owned().unwrap(),
        })
        .unwrap_or_else(|_| panic!("fixture admission"));
    reached.await.unwrap();
    let start = tokio::spawn({
        let store = store.clone();
        let cap = cap.clone();
        let c = c.clone();
        async move { store.begin_upload(cap, c).await }
    });
    tokio::time::timeout(Duration::from_secs(1), async {
        while store.tx.capacity() == 8 {
            tokio::task::yield_now().await
        }
    })
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    resume.send(()).unwrap();
    assert!(matches!(start.await.unwrap(), Err(Error::RunnerAuthority)));
    assert_eq!(
        store.inspect_upload(id.clone()).await.unwrap().upload,
        UploadState::Claimed
    );
    assert!(!store.inspect_upload(id).await.unwrap().outcome_unknown);
    store.shutdown().await.unwrap();
}
