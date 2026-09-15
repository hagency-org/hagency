use super::{admission_tests::*, *};

#[tokio::test]
async fn native_file_service_shutdown_actual_worker_receipt_loss_and_unwind() {
    for mode in [0, 1, 2] {
        let h = Harness::new(128).await;
        // Even directory-unconfirmed Windows initialization retains an actual
        // media owner; closing that owner still has a separately testable ACK.
        let _qualified = h.initialize().await;
        h.owner
            .handle
            .registry
            .tests
            .close_mode
            .store(mode, Ordering::Release);
        let expected = if mode == 0 {
            Ok(())
        } else {
            Err(FileError::Unknown)
        };
        h.close_with(expected).await;
    }
}

#[tokio::test]
async fn native_file_service_shutdown_blocked_original_job() {
    let mut h = Harness::new(128).await;
    if !h.initialize().await {
        h.fake.no_request().await;
        h.close().await;
        return;
    }
    let gate = h.gate();
    let handle = h.owner.handle();
    let queued = handle
        .submit(h.cap.clone(), input("blocked_shutdown"))
        .unwrap()
        .wait()
        .await
        .unwrap();
    gate.entered().await;
    h.owner.quiesce();
    h.shared.workspace.retire();
    assert!(matches!(
        handle.submit(h.cap.clone(), input("new_shutdown")),
        Err(FileError::Unavailable)
    ));
    // The original close receiver is retained after caller timeout. No second
    // Close command is queued, and no source object is dropped on that timeout.
    assert_eq!(h.owner.close().await, Err(FileError::Unknown));
    assert!(h.owner.pending_close.is_some());
    assert!(h.owner.close_result.is_none());
    assert_eq!(h.count(), 1);
    gate.release();
    assert_eq!(h.owner.close().await, Ok(()));
    assert_eq!(h.owner.close().await, Ok(()));
    let receipt =
        h.f.store
            .inspect_file_delivery(h.cap.clone(), queued.delivery_id)
            .await
            .unwrap();
    assert!(receipt.captured.is_none());
    assert_eq!(
        receipt.error_code,
        Some(hagency_core::file_delivery::FileDeliveryFailure::Cancelled)
    );
    h.fake.no_request().await;
    h.close().await;
}

#[test]
fn native_file_service_shutdown_media_creation_requires_atomic_fresh_directory() {
    let root = tempfile::tempdir().unwrap();
    let parent = root.path().join("private");
    hagency_store::private::directory(&parent).unwrap();
    let existing = parent.join("existing");
    hagency_store::private::directory(&existing).unwrap();
    let setup = Setup {
        directory: existing.clone(),
        namespace: "creation_fixture".into(),
        limit: 128,
    };
    assert!(matches!(recovery::open(&setup), Err(FileError::Unknown)));
    assert_eq!(
        std::fs::read_dir(&existing).unwrap().count(),
        0,
        "an existing empty directory must not be repaired into a new journal"
    );
    let fresh = Setup {
        directory: parent.join("fresh"),
        namespace: "creation_fixture".into(),
        limit: 128,
    };
    let original = recovery::open(&fresh).unwrap();
    assert!(
        matches!(recovery::open(&fresh), Err(FileError::Unknown)),
        "actual original media lock remains exclusive"
    );
    drop(original);
    let reopened = recovery::open(&fresh).unwrap();
    assert_eq!(reopened.recovery(), hagency_media_store::Recovery::Clean);
    drop(reopened);
    std::fs::remove_file(fresh.directory.join("media.journal")).unwrap();
    assert!(matches!(recovery::open(&fresh), Err(FileError::Unknown)));
    assert!(!fresh.directory.join("media.journal").exists());
    #[cfg(unix)]
    {
        let link = parent.join("link");
        std::os::unix::fs::symlink(&existing, &link).unwrap();
        assert!(matches!(
            recovery::open(&Setup {
                directory: link,
                namespace: "creation_fixture".into(),
                limit: 128
            }),
            Err(FileError::Unavailable)
        ));
    }
}
