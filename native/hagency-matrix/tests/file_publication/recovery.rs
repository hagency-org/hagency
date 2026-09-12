use super::*;
const ENV: &str = "HAGENCY_FILE_PUBLICATION_RECOVERY_FIXTURE";

pub(super) async fn settle_ack_loss() {
    late_recovery(RecoveryCase::LostSettleAck).await;
    late_recovery(RecoveryCase::CompleteBeforeSettle).await;
}

pub(super) async fn settled_receipt_substitution() {
    late_recovery(RecoveryCase::ChangedSettledReceipt).await;
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RecoveryCase {
    LostSettleAck,
    CompleteBeforeSettle,
    ChangedSettledReceipt,
}

async fn late_recovery(case: RecoveryCase) {
    let mut f = Fixture::new().await;
    let Some((original, id, _)) = accepted(&mut f, "settle-ack-loss", None).await else {
        f.finish().await;
        return;
    };
    let (claim, send) = publication(&f, id.clone()).await;
    let mut op = original
        .prepare_file_publication(claim, send)
        .map_err(|e| e.error())
        .unwrap();
    let peer = {
        let owner = f.collector.inner.owner.lock().await;
        let owner = owner.as_ref().unwrap();
        let peer = owner.outgoing_fixture(true).await;
        if case == RecoveryCase::CompleteBeforeSettle {
            f.collector.inner.outgoing_fault.store(2, Ordering::SeqCst);
        } else {
            owner.outgoing_settle_reply_fault().await;
        }
        peer
    };
    let cancel = CancellationToken::new();
    let (result, ()) = common::scripted(op.run(&cancel), async {
        let (request, _) = wire(&mut f.fake, &peer, ruma::room_id!("!direct:example.test")).await;
        request.json(200, json!({"event_id":"$settle-ack-lost"}));
    })
    .await;
    assert_eq!(result, Err(Error::OutcomeUnknown));
    assert_eq!(op.outcome().unwrap(), Some(Err(Error::OutcomeUnknown)));
    let domain = f
        .base
        .store
        .inspect_file_delivery(f.cap.clone(), id.id().into())
        .await
        .unwrap();
    assert_eq!(
        domain.status,
        hagency_core::file_delivery::FileDeliveryStatus::Delivered
    );
    assert_eq!(f.collector.close().await, Err(Error::Busy));
    let original = f
        .collector
        .inner
        .owner
        .lock()
        .await
        .as_ref()
        .unwrap()
        .outgoing(crate::outgoing::state::Command::Read)
        .await
        .unwrap();
    if case == RecoveryCase::CompleteBeforeSettle {
        let attempt = original
            .attempt
            .as_ref()
            .expect("real Complete awaits Settle");
        assert!(attempt.phase == crate::outgoing::state::Phase::Complete);
        assert_eq!(attempt.id, id.id());
        assert!(
            !original
                .receipts
                .iter()
                .any(|receipt| receipt.id == id.id())
        );
    } else {
        assert!(original.attempt.is_none());
        assert_eq!(
            original
                .receipts
                .iter()
                .filter(|receipt| receipt.kind == crate::outgoing::state::Kind::File
                    && receipt.id == id.id())
                .count(),
            1
        );
    }
    if case == RecoveryCase::ChangedSettledReceipt {
        let sql = rusqlite::Connection::open_with_flags(
            f.base.root.path().join("domain/domain.sqlite3"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let acceptance = || {
            sql.query_row(
                "SELECT acceptance FROM file_deliveries WHERE id=?1",
                [id.id()],
                |row| row.get::<_, String>(0),
            )
            .unwrap()
        };
        let original_acceptance = acceptance();
        f.collector
            .inner
            .owner
            .lock()
            .await
            .as_ref()
            .unwrap()
            .outgoing_settled_receipt_fixture(true)
            .await;
        let changed = f
            .collector
            .inner
            .owner
            .lock()
            .await
            .as_ref()
            .unwrap()
            .outgoing(crate::outgoing::state::Command::Read)
            .await
            .unwrap();
        let before = original
            .receipts
            .iter()
            .find(|receipt| receipt.id == id.id())
            .unwrap();
        let after = changed
            .receipts
            .iter()
            .find(|receipt| receipt.id == id.id())
            .unwrap();
        assert!(changed.attempt.is_none());
        assert_eq!(
            (before.id.as_str(), before.fence),
            (after.id.as_str(), after.fence)
        );
        assert!(before.kind == after.kind);
        assert!(crate::outgoing::state::digest(&after.attempt_digest));
        assert_ne!(before.attempt_digest, after.attempt_digest);
        assert_eq!(
            f.collector.resume_outgoing_custody(&cancel).await,
            Err(Error::Conflict)
        );
        assert_eq!(acceptance(), original_acceptance);
        assert_eq!(op.outcome().unwrap(), Some(Err(Error::OutcomeUnknown)));
        assert_eq!(f.collector.close().await, Err(Error::Busy));
        f.fake.no_request().await;
        // Restore only the actual original SDK receipt saved before mutation,
        // then reopen protected storage and prove normal recovery still works.
        f.collector
            .inner
            .owner
            .lock()
            .await
            .as_ref()
            .unwrap()
            .outgoing_settled_receipt_fixture(false)
            .await;
        let restored = f
            .collector
            .inner
            .owner
            .lock()
            .await
            .as_ref()
            .unwrap()
            .outgoing(crate::outgoing::state::Command::Read)
            .await
            .unwrap();
        let restored = restored
            .receipts
            .iter()
            .find(|receipt| receipt.id == id.id())
            .unwrap();
        assert_eq!(restored.attempt_digest, before.attempt_digest);
        assert_eq!(acceptance(), original_acceptance);
    }
    f.collector.reopen_upload_owner(&cancel).await.unwrap();
    let settled = f.collector.resume_outgoing_custody(&cancel).await.unwrap();
    assert_eq!(settled.state, OutgoingState::Delivered);
    assert_eq!(settled.id.as_deref(), Some(id.id()));
    assert!(settled.replayed);
    assert_eq!(
        op.outcome().unwrap().unwrap().unwrap().state,
        OutgoingState::Delivered
    );
    assert_eq!(op.run(&cancel).await, Err(Error::Conflict));
    assert_eq!(
        f.collector
            .resume_outgoing_custody(&cancel)
            .await
            .unwrap()
            .state,
        OutgoingState::Idle
    );
    f.fake.no_request().await;
    drop((op, peer));
    f.finish().await;
}

pub(super) async fn child() -> bool {
    let Some(path) = std::env::var_os(ENV) else {
        return false;
    };
    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.len() < 4096);
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    let root = std::path::Path::new(&path).parent().unwrap();
    let db = hagency_store::DomainRepository::open(&root.join("domain")).unwrap();
    let domain = hagency_store::DomainStore::start(db, 8).unwrap();
    let identity = crate::HostIdentity {
        server_name: "example.test".into(),
        registration_fingerprint: "a".repeat(64),
        transport: hagency_core::replies::MatrixTransportObservation {
            engagement_id: value["engagement"].as_str().unwrap().into(),
            registration_generation: 1,
            generation: 1,
            sender_mxid: "@worker:example.test".into(),
            device_id: "DEVICE_1".into(),
        },
    };
    let config = crate::HostConfig::new(
        identity,
        value["origin"].as_str().unwrap(),
        common::TOKEN,
        root.join("sdk"),
        [42; 32],
        vec![crate::HostRoom {
            room_id: "!direct:example.test".into(),
            generation: 1,
            privacy: hagency_core::replies::RoomPrivacy::Direct {
                human_mxid: "@owner:example.test".into(),
            },
        }],
        common::limits(),
    )
    .unwrap()
    .with_root_pem(include_bytes!("../fixtures/ca.pem"))
    .unwrap();
    let collector = Collector::new(config, domain.clone()).unwrap();
    let sql = rusqlite::Connection::open(root.join("domain/domain.sqlite3")).unwrap();
    let id = value["id"].as_str().unwrap();
    let before: String = sql
        .query_row(
            "SELECT event_state FROM file_deliveries WHERE id=?1",
            [id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(before, "write_possible");
    let result = collector
        .resume_outgoing_custody(&CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(result.state, OutgoingState::Delivered);
    assert_eq!(result.id.as_deref(), Some(id));
    assert!(result.replayed);
    let after: String = sql
        .query_row(
            "SELECT event_state FROM file_deliveries WHERE id=?1",
            [id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(after, "delivered");
    assert_eq!(
        collector
            .resume_outgoing_custody(&CancellationToken::new())
            .await
            .unwrap()
            .state,
        OutgoingState::Idle
    );
    drop(sql);
    collector.close().await.unwrap();
    drop(collector);
    domain.shutdown().await.unwrap();
    true
}

pub(super) async fn restart(f: Fixture, id: &str) {
    let (base, collector, mut fake) = f.into_file_teardown();
    assert_eq!(collector.close().await, Err(Error::Busy));
    {
        let mut owner = collector.inner.owner.lock().await;
        let attempt = owner
            .as_ref()
            .unwrap()
            .outgoing(crate::outgoing::state::Command::Read)
            .await
            .unwrap()
            .attempt
            .unwrap();
        assert!(attempt.phase == crate::outgoing::state::Phase::Complete);
        assert_eq!(attempt.observation().unwrap().event_id, "$historical-file");
        owner.take().unwrap().close().await.unwrap();
    }
    let weak = Arc::downgrade(&collector.inner);
    drop(collector);
    assert!(weak.upgrade().is_none());
    base.store.shutdown().await.unwrap();
    let common::Fixture {
        root,
        store,
        identity,
    } = base;
    drop(store);
    let path = root.path().join("file-recovery-fixture.json");
    std::fs::write(
        &path,
        serde_json::to_vec(
            &json!({"id":id,"origin":fake.endpoint,"engagement":identity.transport.engagement_id}),
        )
        .unwrap(),
    )
    .unwrap();
    let stdout = std::fs::File::create(root.path().join("file-child.stdout")).unwrap();
    let stderr = std::fs::File::create(root.path().join("file-child.stderr")).unwrap();
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "upload::file_tests::native_file_publication_historical",
            "--nocapture",
        ])
        .env_clear()
        .env(ENV, &path)
        .stdout(stdout)
        .stderr(stderr);
    #[cfg(windows)]
    if let Some(system_root) = std::env::var_os("SystemRoot") {
        command.env("SystemRoot", system_root);
    }
    let mut child = command.spawn().unwrap();
    let result = tokio::task::spawn_blocking(move || {
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if std::time::Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("file recovery child timed out");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    })
    .await
    .unwrap();
    assert!(
        result.success(),
        "file recovery child: {}",
        std::fs::read_to_string(root.path().join("file-child.stderr")).unwrap()
    );
    assert!(
        std::fs::read_to_string(root.path().join("file-child.stdout"))
            .unwrap()
            .contains("1 passed")
    );
    fake.no_request().await;
    fake.close().await;
}
