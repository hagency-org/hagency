use super::*;
use hagency_store::{DomainStore, UploadSettlement};
use std::{path::Path, sync::Arc, time::Duration};

fn possible(f: &mut Fixture, call: &str) -> hagency_store::UploadSend {
    let (id, _) = f.ready(call);
    let claim = f.db.claim_upload(&f.cap, &id, 1020, 100).unwrap().unwrap();
    f.db.begin_upload(&f.cap, &claim, 1021).unwrap()
}
fn restore(f: &Fixture, send: &hagency_store::UploadSend) -> UploadSettlement {
    f.db.restore_upload_settlement(
        send.identity().id(),
        send.fence(),
        send.stage(),
        send.route(),
    )
    .unwrap()
    .unwrap()
}

#[test]
fn native_upload_settlement_exact() {
    trait Sealed<A> {
        fn check() {}
    }
    impl<T: ?Sized> Sealed<()> for T {}
    impl<T: Clone> Sealed<u8> for T {}
    impl<T: std::fmt::Debug> Sealed<u16> for T {}
    impl<T: serde::Serialize> Sealed<u32> for T {}
    impl<T: serde::de::DeserializeOwned> Sealed<u64> for T {}
    let _ = <UploadSettlement as Sealed<_>>::check;
    let mut f = Fixture::new(true, None);
    let send = possible(&mut f, "first");
    let restored = restore(&f, &send);
    assert_eq!(restored.id(), send.identity().id());
    let receipt = f.db.inspect_upload_settlement(&restored).unwrap();
    assert_eq!(receipt.upload, UploadState::WritePossible);
    assert!(receipt.outcome_unknown);
    let absent = format!("upload_{}", "f".repeat(32));
    assert!(
        f.db.restore_upload_settlement(&absent, send.fence(), send.stage(), send.route())
            .unwrap()
            .is_none()
    );
    for (id, fence) in [
        ("upload_bad".to_owned(), 1),
        (format!("upload_{}", "F".repeat(32)), 1),
        (send.identity().id().into(), 0),
        (send.identity().id().into(), hagency_core::JSON_SAFE_MAX + 1),
    ] {
        assert!(
            f.db.restore_upload_settlement(&id, fence, send.stage(), send.route())
                .is_err()
        );
    }
    for variant in 0..8 {
        let mut stage = send.stage().clone();
        let mut route = send.route().clone();
        let mut fence = send.fence();
        match variant {
            0 => fence += 1,
            1 => stage.receipt_digest = "e".repeat(64),
            2 => stage.operation_id = "different".into(),
            3 => stage.namespace_digest = "e".repeat(64),
            4 => stage.len += 1,
            5 => route.thread_root = Some("$different".into()),
            6 => route.transport_generation += 1,
            7 => route.owner_mxid = "@other:example.test".into(),
            _ => unreachable!(),
        }
        assert!(
            f.db.restore_upload_settlement(send.identity().id(), fence, &stage, &route)
                .is_err()
        );
    }
    // This is a real route issued by the existing SessionBinding policy, whose
    // opaque EventId root is intentionally not restricted to 255 bytes.
    let long_root = format!("${}", "r".repeat(600));
    let mut threaded = Fixture::new(true, Some(&long_root));
    let threaded_send = possible(&mut threaded, "long_thread");
    let threaded_restored = restore(&threaded, &threaded_send);
    assert_eq!(threaded_restored.id(), threaded_send.identity().id());
    let mut huge = threaded_send.route().clone();
    huge.thread_root = Some(format!("${}", "r".repeat(64 * 1024)));
    assert!(matches!(
        threaded.db.restore_upload_settlement(
            threaded_send.identity().id(),
            threaded_send.fence(),
            threaded_send.stage(),
            &huge
        ),
        Err(Error::Capacity)
    ));
    let pending = f.reserve("pending");
    assert!(
        f.db.restore_upload_settlement(pending.identity.id(), 1, send.stage(), send.route())
            .is_err()
    );
    let (id, stage) = f.ready("claimed");
    let claim = f.db.claim_upload(&f.cap, &id, 1022, 100).unwrap().unwrap();
    assert!(
        f.db.restore_upload_settlement(id.id(), claim.fence(), &stage, send.route())
            .is_err()
    );
    // Merely restored history does not change one-shot send state.
    assert!(
        f.db.claim_upload(&f.cap, send.identity(), 1023, 100)
            .unwrap()
            .is_none()
    );
}

#[test]
fn native_upload_settlement_retirement() {
    for mode in ["cancel", "revoke", "promote", "done", "expire", "restart"] {
        let mut f = Fixture::new(true, None);
        let send = possible(&mut f, "one");
        let before = restore(&f, &send);
        let now = if mode == "expire" { 61006 } else { 1030 };
        match mode {
            "cancel" => {
                f.db.cancel_upload(send.identity(), 1024).unwrap();
            }
            "revoke" => {
                f.db.revoke("retire", &f.engagement).unwrap();
            }
            "promote" => {
                f.room.generation = 2;
                f.room.privacy = RoomPrivacy::Group {};
                f.room.joined.insert("@third:example.test".into());
                f.db.observe_matrix_room(&f.room, 1025).unwrap();
            }
            "done" => f.done(),
            "expire" => f.db.reconcile_dispatches(now).unwrap(),
            "restart" => {
                f = f.restart();
            }
            _ => unreachable!(),
        }
        let after = restore(&f, &send);
        let accepted =
            f.db.record_upload_settlement(&before, &acceptance(), now)
                .unwrap();
        assert_eq!(accepted.upload, UploadState::Accepted);
        assert_eq!(accepted.cancel_requested, mode == "cancel");
        assert!(!accepted.outcome_unknown);
        assert!(
            f.db.record_upload_settlement(&after, &acceptance(), now + 1)
                .unwrap()
                .replayed
        );
        assert!(
            f.db.claim_upload(&f.cap, send.identity(), now + 2, 100)
                .is_err()
        );
    }
}

#[test]
fn native_upload_settlement_atomic() {
    let mut f = Fixture::new(true, None);
    let send = possible(&mut f, "one");
    let restored = restore(&f, &send);
    f.sql().execute_batch("CREATE TRIGGER refuse_settlement BEFORE UPDATE OF acceptance ON file_uploads WHEN NEW.acceptance IS NOT NULL BEGIN SELECT RAISE(ABORT,'fixture failure'); END;").unwrap();
    assert!(
        f.db.record_upload_settlement(&restored, &acceptance(), 1025)
            .is_err()
    );
    let unchanged = f.db.inspect_upload_settlement(&restored).unwrap();
    assert_eq!(unchanged.upload, UploadState::WritePossible);
    assert!(unchanged.outcome_unknown);
    f.sql()
        .execute_batch("DROP TRIGGER refuse_settlement;")
        .unwrap();
    let receipt =
        f.db.record_upload_settlement(&restored, &acceptance(), 1026)
            .unwrap();
    assert!(!receipt.replayed);
    assert!(
        f.db.record_upload_settlement(&restored, &acceptance(), 1027)
            .unwrap()
            .replayed
    );
    let changed = UploadAcceptance {
        receipt_digest: "e".repeat(64),
        ..acceptance()
    };
    assert!(matches!(
        f.db.record_upload_settlement(&restored, &changed, 1028),
        Err(Error::Conflict)
    ));
    assert_eq!(
        f.db.inspect_upload_settlement(&restored).unwrap().upload,
        UploadState::Accepted
    );
    // Negative persisted-row corruption proves record rechecks the complete
    // frozen route even after a sealed handle was constructed. No authenticated
    // Matrix observation is claimed for this deliberate SQL mutation.
    f.sql()
        .execute(
            "UPDATE file_uploads SET route=json_set(route,'$.transport_generation',2) WHERE id=?1",
            [send.identity().id()],
        )
        .unwrap();
    assert!(f.db.inspect_upload_settlement(&restored).is_err());
    assert!(
        f.db.record_upload_settlement(&restored, &acceptance(), 1029)
            .is_err()
    );
}

#[tokio::test]
async fn native_upload_settlement_worker() {
    let mut f = Fixture::new(true, None);
    let send = possible(&mut f, "one");
    let store = DomainStore::start(f.db, 4).unwrap();
    let first = Arc::new(
        store
            .restore_upload_settlement(
                send.identity().id().into(),
                send.fence(),
                send.stage().clone(),
                send.route().clone(),
            )
            .await
            .unwrap()
            .unwrap(),
    );
    let second = Arc::new(
        store
            .restore_upload_settlement(
                send.identity().id().into(),
                send.fence(),
                send.stage().clone(),
                send.route().clone(),
            )
            .await
            .unwrap()
            .unwrap(),
    );
    let (a, b) = tokio::join!(
        store.record_upload_settlement(first.clone(), acceptance()),
        store.record_upload_settlement(second, acceptance())
    );
    let a = a.unwrap();
    let b = b.unwrap();
    assert_ne!(a.replayed, b.replayed);
    assert_eq!(a.upload, UploadState::Accepted);
    assert_eq!(b.upload, UploadState::Accepted);
    assert_eq!(
        store.inspect_upload_settlement(first).await.unwrap().upload,
        UploadState::Accepted
    );
    let (result, snapshot) = store.shutdown_observed().await;
    if result.is_err() {
        panic!("upload settlement fixture shutdown failed: {result:?}; {snapshot:?}");
    }
}

// The fixture handoff intentionally contains only locator and opaque historical
// receipt DATA. It establishes no real SDK/HTTP observation or sender authority.
fn handoff(send: &hagency_store::UploadSend) -> serde_json::Value {
    json!({"id":send.identity().id(),"fence":send.fence(),"stage":send.stage(),"route":send.route(),"acceptance":acceptance()})
}
fn child(root: &Path, phase: &str) {
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "settlement::native_upload_settlement_process",
            "--nocapture",
        ])
        .env_clear()
        .env("HAGENCY_SETTLEMENT_FIXTURE_PHASE", phase)
        .env("HAGENCY_SETTLEMENT_FIXTURE_ROOT", root)
        .stdout(std::process::Stdio::null());
    for key in ["SystemRoot", "WINDIR", "TEMP", "TMP"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    let mut process = command.spawn().unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(status) = process.try_wait().unwrap() {
            assert!(status.success(), "fixture child failed in {phase}");
            break;
        }
        if std::time::Instant::now() >= deadline {
            process.kill().unwrap();
            process.wait().unwrap();
            panic!("fixture child deadline in {phase}");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
#[test]
fn native_upload_settlement_process() {
    if let Some(phase) = std::env::var_os("HAGENCY_SETTLEMENT_FIXTURE_PHASE") {
        let root =
            std::path::PathBuf::from(std::env::var_os("HAGENCY_SETTLEMENT_FIXTURE_ROOT").unwrap());
        if phase == "write" {
            let mut f = Fixture::new(true, None);
            let send = possible(&mut f, "child_only");
            f.db.cancel_upload(send.identity(), 1023).unwrap();
            f.db.revoke("child_retired", &f.engagement).unwrap();
            std::fs::write(
                root.join("locator.json"),
                serde_json::to_vec(&handoff(&send)).unwrap(),
            )
            .unwrap();
            drop(send);
            drop(f.cap);
            drop(f.db);
            // Move the closed private fixture store to the host-provisioned test
            // root; the original child secret was never serialized or returned.
            std::fs::rename(f.root.path().join("state"), root.join("state")).unwrap();
            return;
        }
        assert!(phase == "settle" || phase == "replay");
        let path = root.join("locator.json");
        assert!(std::fs::metadata(&path).unwrap().len() <= 8192);
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let keys = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        assert_eq!(
            keys,
            BTreeSet::from(["id", "fence", "stage", "route", "acceptance"])
        );
        let stage: StageCommitment = serde_json::from_value(value["stage"].clone()).unwrap();
        let route: ReplyRoute = serde_json::from_value(value["route"].clone()).unwrap();
        let evidence = UploadAcceptance {
            receipt_id: value["acceptance"]["receipt_id"].as_str().unwrap().into(),
            receipt_digest: value["acceptance"]["receipt_digest"]
                .as_str()
                .unwrap()
                .into(),
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let store = DomainStore::start(DomainRepository::open(&root.join("state")).unwrap(), 2)
                .unwrap();
            let restored = Arc::new(
                store
                    .restore_upload_settlement(
                        value["id"].as_str().unwrap().into(),
                        value["fence"].as_u64().unwrap(),
                        stage,
                        route,
                    )
                    .await
                    .unwrap()
                    .unwrap(),
            );
            let receipt = store
                .record_upload_settlement(restored.clone(), evidence)
                .await
                .unwrap();
            assert_eq!(receipt.upload, UploadState::Accepted);
            assert!(receipt.cancel_requested && !receipt.outcome_unknown);
            assert_eq!(receipt.replayed, phase == "replay");
            assert_eq!(
                store
                    .inspect_upload_settlement(restored)
                    .await
                    .unwrap()
                    .upload,
                UploadState::Accepted
            );
            let (result, snapshot) = store.shutdown_observed().await;
            if result.is_err() {
                panic!("upload settlement child shutdown failed: {result:?}; {snapshot:?}");
            }
        });
        return;
    }
    let root = tempfile::tempdir().unwrap();
    child(root.path(), "write");
    child(root.path(), "settle");
    child(root.path(), "replay");
}
