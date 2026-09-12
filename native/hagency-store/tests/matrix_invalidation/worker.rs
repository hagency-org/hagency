use super::clock_fixtures::{proof, registration, request, resource};
use super::*;
use std::{collections::BTreeSet, future::Future, pin::Pin, task::Poll};

struct Fixture {
    root: tempfile::TempDir,
    store: DomainStore,
    transports: Vec<MatrixTransportObservation>,
    rooms: Vec<MatrixRoomObservation>,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut db = DomainRepository::open(&root.path().join("state")).unwrap();
        db.register(&registration()).unwrap();
        let pool = resource("pool", "seat", 1000);
        db.put_resource(&pool).unwrap();
        let joined = BTreeSet::from([
            "@owner:example.test".into(),
            "@first:example.test".into(),
            "@second:example.test".into(),
        ]);
        let mut transports = Vec::new();
        let mut rooms = Vec::new();
        for (n, name) in ["first", "second"].into_iter().enumerate() {
            let verified = proof(&request(name, name, &pool, 100));
            let engagement = db.admit(&verified, 1000).unwrap();
            db.approve(&format!("approve_{name}"), &verified, 1000)
                .unwrap();
            let effect = db.claim_effect().unwrap().unwrap();
            db.observe_effect(
                &effect.id,
                effect.fence,
                &EffectOutcome::Applied {
                    receipt: "fixture provision".into(),
                },
            )
            .unwrap();
            let transport = MatrixTransportObservation {
                engagement_id: engagement.id.clone(),
                registration_generation: 1,
                generation: 1,
                sender_mxid: format!("@{name}:example.test"),
                device_id: format!("DEVICE_{n}"),
            };
            db.observe_matrix_transport(&transport, 1001).unwrap();
            let room = MatrixRoomObservation {
                engagement_id: engagement.id,
                registration_generation: 1,
                transport_generation: 1,
                room_id: "!project:example.test".into(),
                generation: 1,
                privacy: RoomPrivacy::Group {},
                joined: joined.clone(),
                invite_only: true,
                encrypted: true,
            };
            db.observe_matrix_room(&room, 1002).unwrap();
            db.resolve_verified_matrix_session(
                &SessionBinding {
                    id: format!("session_{n}"),
                    engagement_id: room.engagement_id.clone(),
                    room_id: room.room_id.clone(),
                    thread_root: None,
                },
                1003,
            )
            .unwrap();
            transports.push(transport);
            rooms.push(room);
        }
        let store = DomainStore::start(db, 8).unwrap();
        Self {
            root,
            store,
            transports,
            rooms,
        }
    }
    fn transport_negative(&self) -> MatrixTransportInvalidation {
        MatrixTransportInvalidation {
            expected: self.transports[0].clone(),
            reason: "original authenticated identity failure".into(),
        }
    }
    fn room_negative(&self) -> MatrixRoomInvalidation {
        MatrixRoomInvalidation {
            engagement_id: self.rooms[0].engagement_id.clone(),
            registration_generation: 1,
            transport_generation: 1,
            room_id: self.rooms[0].room_id.clone(),
            generation: 2,
            reason: "original authenticated room failure".into(),
        }
    }
    fn count(&self, sql: &str) -> u64 {
        let db = rusqlite::Connection::open(self.root.path().join("state/domain.sqlite3")).unwrap();
        db.query_row(sql, [], |row| row.get(0)).unwrap()
    }
    async fn transport(&self, n: usize) -> MatrixTransportState {
        self.store
            .matrix_transport_state(self.transports[n].engagement_id.clone())
            .await
            .unwrap()
            .unwrap()
    }
    async fn room(&self, n: usize) -> MatrixRoomState {
        self.store
            .matrix_room_state(
                self.rooms[n].engagement_id.clone(),
                self.rooms[n].room_id.clone(),
            )
            .await
            .unwrap()
            .unwrap()
    }
}

struct HeldWriter(Option<std::sync::mpsc::Sender<()>>);
impl HeldWriter {
    fn release(mut self) {
        self.0.take().unwrap().send(()).unwrap();
    }
}
impl Drop for HeldWriter {
    fn drop(&mut self) {
        if let Some(release) = self.0.take() {
            let _ = release.send(());
        }
    }
}
async fn hold(
    store: &DomainStore,
    before_next: impl FnOnce(&mut DomainRepository) + Send + 'static,
) -> HeldWriter {
    let (picked_up, ready) = oneshot::channel();
    let (release, held) = std::sync::mpsc::channel();
    store
        .tx
        .try_send(Job::Run {
            operation: Box::new(move |db| {
                picked_up.send(()).unwrap();
                held.recv_timeout(Duration::from_secs(6)).unwrap();
                before_next(db);
            }),
            _bytes: store.bytes.clone().try_acquire_owned().unwrap(),
        })
        .unwrap_or_else(|_| panic!("fixture writer hold was not admitted"));
    tokio::time::timeout(Duration::from_secs(2), ready)
        .await
        .unwrap()
        .unwrap();
    HeldWriter(Some(release))
}
async fn enqueued<F: Future>(store: &DomainStore, mut future: Pin<&mut F>) {
    let available = store.tx.capacity();
    std::future::poll_fn(|cx| {
        assert!(
            future.as_mut().poll(cx).is_pending(),
            "held writer cannot reply"
        );
        Poll::Ready(())
    })
    .await;
    assert_eq!(
        store.tx.capacity(),
        available - 1,
        "original call must be enqueued"
    );
}

#[tokio::test]
async fn native_matrix_invalidation_transport_drop() {
    let f = Fixture::new();
    assert_eq!(f.count("SELECT COUNT(*) FROM current_matrix_routes"), 2);
    let held = hold(&f.store, |_| {}).await;
    let bytes = f.store.bytes.available_permits();
    let mut pending = Box::pin(f.store.invalidate_matrix_transport(f.transport_negative()));
    enqueued(&f.store, pending.as_mut()).await;
    let retained_bytes = f.store.bytes.available_permits();
    assert!(retained_bytes < bytes);
    drop(pending);
    assert_eq!(f.store.bytes.available_permits(), retained_bytes);
    held.release();
    assert!(!f.transport(0).await.available);
    assert!(f.transport(1).await.available);
    assert_eq!(f.count("SELECT COUNT(*) FROM current_matrix_routes"), 1);
    f.store.shutdown().await.unwrap();
}

#[tokio::test]
async fn native_matrix_invalidation_room_drop() {
    let f = Fixture::new();
    let held = hold(&f.store, |_| {}).await;
    let mut pending = Box::pin(f.store.invalidate_matrix_room(f.room_negative()));
    enqueued(&f.store, pending.as_mut()).await;
    drop(pending);
    held.release();
    for n in 0..2 {
        let room = f.room(n).await;
        assert!(!room.available);
        assert_eq!(room.generation, 2);
        assert!(f.transport(n).await.available);
    }
    assert_eq!(f.count("SELECT COUNT(*) FROM current_matrix_routes"), 0);
    f.store.shutdown().await.unwrap();
}

#[tokio::test]
async fn native_matrix_invalidation_replacement() {
    for transport_change in [true, false] {
        let f = Fixture::new();
        let mut replacement_transport = f.transports[0].clone();
        replacement_transport.generation = 2;
        replacement_transport.device_id = "REPLACEMENT".into();
        let mut replacement_room = f.rooms[0].clone();
        if transport_change {
            replacement_room.transport_generation = 2;
        } else {
            replacement_room.generation = 2;
        }
        let held = hold(&f.store, move |db| {
            let now = writer_time().unwrap();
            if transport_change {
                db.observe_matrix_transport(&replacement_transport, now)
                    .unwrap();
            }
            db.observe_matrix_room(&replacement_room, now).unwrap();
            db.resolve_verified_matrix_session(
                &SessionBinding {
                    id: "replacement_session".into(),
                    engagement_id: replacement_room.engagement_id.clone(),
                    room_id: replacement_room.room_id.clone(),
                    thread_root: None,
                },
                now,
            )
            .unwrap();
        })
        .await;
        if transport_change {
            let mut pending = Box::pin(f.store.invalidate_matrix_transport(f.transport_negative()));
            enqueued(&f.store, pending.as_mut()).await;
            drop(pending);
        } else {
            let mut pending = Box::pin(f.store.invalidate_matrix_room(f.room_negative()));
            enqueued(&f.store, pending.as_mut()).await;
            drop(pending);
        }
        held.release();
        let transport = f.transport(0).await;
        assert!(transport.available);
        assert_eq!(
            transport.observation.generation,
            if transport_change { 2 } else { 1 }
        );
        assert!(f.room(0).await.available);
        assert_eq!(
            f.count(
                "SELECT COUNT(*) FROM current_matrix_routes WHERE session_id='replacement_session'"
            ),
            1
        );
        f.store.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn native_matrix_invalidation_timeout() {
    for transport in [true, false] {
        let f = Fixture::new();
        let held = hold(&f.store, |_| {}).await;
        let result = if transport {
            let mut pending = Box::pin(f.store.invalidate_matrix_transport(f.transport_negative()));
            enqueued(&f.store, pending.as_mut()).await;
            pending.await
        } else {
            let mut pending = Box::pin(f.store.invalidate_matrix_room(f.room_negative()));
            enqueued(&f.store, pending.as_mut()).await;
            pending.await
        };
        assert!(matches!(result, Err(Error::OutcomeUnknown)));
        assert_eq!(f.count("SELECT COUNT(*) FROM current_matrix_routes"), 2);
        held.release();
        if transport {
            assert!(!f.transport(0).await.available);
        } else {
            assert!(!f.room(0).await.available);
        }
        // A separate query observed eventual mutation; the original result did not change.
        assert!(matches!(result, Err(Error::OutcomeUnknown)));
        f.store.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn native_matrix_invalidation_cancelled_work() {
    let f = Fixture::new();
    let held = hold(&f.store, |_| {}).await;
    let mut positive = f.transports[0].clone();
    positive.generation = 2;
    let mut observation = Box::pin(f.store.observe_matrix_transport(positive));
    enqueued(&f.store, observation.as_mut()).await;
    let mut task = Box::pin(f.store.create_canonical_task(
        "cancelled_task".into(),
        "session_0".into(),
        "Cancelled work".into(),
        writer_time().unwrap(),
    ));
    enqueued(&f.store, task.as_mut()).await;
    drop((observation, task));
    held.release();
    let transport = f.transport(0).await;
    assert!(transport.available);
    assert_eq!(transport.observation.generation, 1);
    assert_eq!(
        f.count("SELECT COUNT(*) FROM canonical_tasks WHERE id='cancelled_task'"),
        0
    );
    assert_eq!(f.count("SELECT COUNT(*) FROM current_matrix_routes"), 2);
    f.store.shutdown().await.unwrap();
}
