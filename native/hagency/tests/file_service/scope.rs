use super::{admission_tests::*, *};
use hagency_core::{file_delivery::*, replies::MatrixRoomInvalidation, uploads::*};

#[tokio::test]
async fn native_file_service_authority_queue_retirement_and_lower_limit() {
    let mut h = Harness::new(8).await;
    if !h.initialize().await {
        h.fake.no_request().await;
        h.close().await;
        return;
    }
    let handle = h.owner.handle();
    let mut foreign = h.cap.clone();
    foreign.secret = "f".repeat(64);
    assert_eq!(
        handle
            .submit(foreign, input("foreign"))
            .unwrap()
            .wait()
            .await,
        Err(FileError::Unauthorized)
    );
    h.empty().await;
    let mut escape = input("escape");
    escape.path = "../secret".into();
    assert!(matches!(
        handle.submit(h.cap.clone(), escape),
        Err(FileError::Invalid)
    ));
    assert_eq!(h.count(), 0);
    std::fs::write(h.f.root.path().join("work/missing.bin"), b"123456789").unwrap();
    let oversized = handle
        .submit(h.cap.clone(), input("lower_limit"))
        .unwrap()
        .wait()
        .await
        .unwrap();
    h.empty().await;
    let receipt =
        h.f.store
            .inspect_file_delivery(h.cap.clone(), oversized.delivery_id)
            .await
            .unwrap();
    assert_eq!(receipt.error_code, Some(FileDeliveryFailure::SourceRefused));
    assert!(receipt.captured.is_none());
    std::fs::write(h.f.root.path().join("work/missing.bin"), b"1234").unwrap();
    let gate = h.gate();
    let queued = handle
        .submit(h.cap.clone(), input("queued"))
        .unwrap()
        .wait()
        .await
        .unwrap();
    gate.entered().await;
    // Authenticated host negative evidence, while the actual source worker is
    // queued at its boundary. No SQL availability or expiry setter is used.
    h.f.store
        .invalidate_matrix_room(MatrixRoomInvalidation {
            engagement_id: h.f.identity.transport.engagement_id.clone(),
            registration_generation: 1,
            transport_generation: 1,
            room_id: "!direct:example.test".into(),
            generation: 2,
            reason: "fixture authenticated owner departure".into(),
        })
        .await
        .unwrap();
    gate.release();
    h.empty().await;
    let historical = handle
        .inspect(h.cap.clone(), queued.delivery_id.clone())
        .await
        .unwrap();
    assert_eq!(historical.status, FileStatus::Failed);
    let receipt =
        h.f.store
            .inspect_file_delivery(h.cap.clone(), queued.delivery_id)
            .await
            .unwrap();
    assert!(receipt.captured.is_none());
    assert_eq!(receipt.stage, UploadStageState::Unbound);
    assert_eq!(
        handle
            .submit(h.cap.clone(), input("after_retirement"))
            .unwrap()
            .wait()
            .await,
        Err(FileError::Unauthorized)
    );
    h.empty().await;
    assert_eq!(h.count(), 2);
    h.fake.no_request().await;
    h.close().await;
}

#[tokio::test]
async fn native_file_service_authority_expired_attempt() {
    let mut h = Harness::new(128).await;
    if !h.initialize().await {
        h.fake.no_request().await;
        h.close().await;
        return;
    }
    let handle = h.owner.handle();
    let gate = h.gate();
    let queued = handle
        .submit(h.cap.clone(), input("expired"))
        .unwrap()
        .wait()
        .await
        .unwrap();
    gate.entered().await;
    // The actual host reconciliation command observes a time beyond the issued
    // lease; it performs the real expired-attempt transition. This is not a
    // wall-clock timing claim or a SQL mutation of authority rows.
    h.f.store
        .reconcile_dispatches(now() + 120_000)
        .await
        .unwrap();
    gate.release();
    h.empty().await;
    let receipt =
        h.f.store
            .inspect_file_delivery(h.cap.clone(), queued.delivery_id.clone())
            .await
            .unwrap();
    assert!(receipt.captured.is_none());
    assert_eq!(
        handle
            .inspect(h.cap.clone(), queued.delivery_id)
            .await
            .unwrap()
            .status,
        FileStatus::Failed
    );
    assert_eq!(
        handle
            .submit(h.cap.clone(), input("expired_new"))
            .unwrap()
            .wait()
            .await,
        Err(FileError::Unauthorized)
    );
    h.empty().await;
    h.fake.no_request().await;
    h.close().await;
}

#[test]
fn native_file_service_protocol_missing_failure_stays_unknown() {
    let receipt = FileDeliveryReceipt {
        id: "file_receipt".into(),
        filename: "safe.bin".into(),
        captured: None,
        stage: UploadStageState::Unbound,
        upload: UploadState::Pending,
        event: FileEventState::Pending,
        status: FileDeliveryStatus::Failed,
        cancel_requested: false,
        error_code: None,
        event_id: None,
        replayed: false,
    };
    let projected = FileView::from_receipt(receipt.clone(), false);
    assert_eq!(projected.status, FileStatus::OutcomeUnknown);
    assert_eq!(
        projected.error_code.as_deref(),
        Some(crate::file_service::types::UnknownOrigin::Remote.code())
    );
    projected.validate().unwrap();
    // The two producers must be distinguishable, and both must validate.
    assert_ne!(
        crate::file_service::types::UnknownOrigin::Remote.code(),
        crate::file_service::types::UnknownOrigin::Custody.code()
    );
    let custody = FileView {
        delivery_id: projected.delivery_id.clone(),
        filename: projected.filename.clone(),
        status: FileStatus::OutcomeUnknown,
        replayed: false,
        error_code: Some(
            crate::file_service::types::UnknownOrigin::Custody
                .code()
                .into(),
        ),
    };
    custody.validate().unwrap();
    // The closed set still refuses an invented code for this status.
    let mut invented = custody.clone();
    invented.error_code = Some("outcome_unknown_somewhere".into());
    assert!(invented.validate().is_err());
    let mut contradictory = receipt;
    contradictory.status = FileDeliveryStatus::Delivered;
    contradictory.error_code = Some(FileDeliveryFailure::SourceRefused);
    assert_eq!(
        FileView::from_receipt(contradictory, false).status,
        FileStatus::OutcomeUnknown
    );
}

/// The domain's trusted adapter observations are correlation DATA in this
/// projection test, as in hagency-store/tests/file_delivery.rs. This creates no
/// SDK proof or qualified source/media receipt and makes no network delivery claim.
#[tokio::test]
async fn native_file_service_protocol_delivered_after_cancellation() {
    let mut h = Harness::new(128).await;
    let qualified = h.initialize().await;
    let handle = h.owner.handle();
    let request = input("settled_after_cancel");
    let mut admission =
        h.f.store
            .reserve_file_delivery(h.cap.clone(), request.request().unwrap())
            .await
            .unwrap();
    let stage = StageCommitment {
        namespace_digest: "3".repeat(64),
        operation_id: admission.upload.identity.id().into(),
        receipt_digest: "4".repeat(64),
        len: 12,
    };
    h.f.store
        .bind_file_delivery_stage(
            h.cap.clone(),
            admission.identity.clone(),
            Arc::new(admission.upload.preparation.take().unwrap()),
            CapturedFile {
                size: 12,
                sha256: "2".repeat(64),
            },
            stage.clone(),
        )
        .await
        .unwrap();
    h.f.store
        .observe_upload_staged(
            admission.upload.identity.clone(),
            stage.clone(),
            UploadStageObservation::FileAndDirectorySynced,
        )
        .await
        .unwrap();
    let upload =
        h.f.store
            .claim_upload(h.cap.clone(), admission.upload.identity.clone(), 30_000)
            .await
            .unwrap()
            .unwrap();
    let sent_upload =
        h.f.store
            .begin_upload(h.cap.clone(), upload.clone())
            .await
            .unwrap();
    h.f.store
        .record_upload_acceptance(
            admission.upload.identity.clone(),
            upload.fence(),
            stage,
            UploadAcceptance {
                receipt_id: "private_upload".into(),
                receipt_digest: "5".repeat(64),
            },
        )
        .await
        .unwrap();
    drop(sent_upload);
    let claim =
        h.f.store
            .claim_file_publication(h.cap.clone(), admission.identity.clone(), 30_000)
            .await
            .unwrap()
            .unwrap();
    let send =
        h.f.store
            .begin_file_publication(h.cap.clone(), claim)
            .await
            .unwrap();
    let locator = send.locator().clone();
    let accepted = FileDeliveryAcceptance {
        transaction_id: locator.transaction_id.clone(),
        content_digest: locator.content_digest.clone(),
        event_id: "$accepted_after_cancel".into(),
        receipt_id: "private_event".into(),
        receipt_digest: "6".repeat(64),
    };
    let cancelled =
        h.f.store
            .cancel_file_delivery(admission.identity.clone(), FileDeliveryFailure::Cancelled)
            .await
            .unwrap();
    assert!(cancelled.cancel_requested);
    assert_eq!(cancelled.status, FileDeliveryStatus::OutcomeUnknown);
    assert_eq!(cancelled.event, FileEventState::WritePossible);
    assert_eq!(
        handle
            .inspect(h.cap.clone(), admission.identity.id().into())
            .await
            .unwrap()
            .status,
        FileStatus::OutcomeUnknown
    );
    drop(send);
    let settlement =
        h.f.store
            .restore_file_delivery_settlement(locator)
            .await
            .unwrap()
            .unwrap();
    let delivered =
        h.f.store
            .record_file_delivery_settlement(Arc::new(settlement), accepted)
            .await
            .unwrap();
    assert_eq!(delivered.status, FileDeliveryStatus::Delivered);
    assert_eq!(delivered.event, FileEventState::Delivered);
    assert!(delivered.cancel_requested);
    assert_eq!(delivered.error_code, Some(FileDeliveryFailure::Cancelled));
    assert!(
        !delivered.replayed,
        "actual first historical domain settlement"
    );
    let view = handle
        .inspect(h.cap.clone(), admission.identity.id().into())
        .await
        .unwrap();
    assert_eq!(view.status, FileStatus::Delivered);
    assert_eq!(view.error_code, None);
    view.validate().unwrap();
    if qualified {
        let replay = handle
            .submit(h.cap.clone(), request)
            .unwrap()
            .wait()
            .await
            .unwrap();
        assert_eq!(replay.delivery_id, view.delivery_id);
        assert_eq!(replay.status, FileStatus::Delivered);
        assert_eq!(replay.error_code, None);
        h.empty().await;
    }
    let retained =
        h.f.store
            .inspect_file_delivery(h.cap.clone(), admission.identity.id().into())
            .await
            .unwrap();
    assert!(retained.cancel_requested);
    assert_eq!(retained.error_code, Some(FileDeliveryFailure::Cancelled));
    assert_eq!(h.count(), 1);
    h.fake.no_request().await;
    h.close().await;
}
