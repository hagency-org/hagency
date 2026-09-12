mod common;
use common::*;
use hagency_core::file_delivery::*;
use hagency_core::{attachments::AttachmentMetadata, replies::*, tasks::*, uploads::*};
use hagency_store::{
    DomainRepository, EffectOutcome, Error, FileDeliveryAdmission, FilePublicationClaim,
    FilePublicationSend, UploadClaim,
};
use serde_json::json;
use std::collections::BTreeSet;

struct Fixture {
    root: tempfile::TempDir,
    db: DomainRepository,
    engagement: String,
    binding: SessionBinding,
    room: MatrixRoomObservation,
    transport: MatrixTransportObservation,
    cap: RunnerCapability,
}
fn dispatch(id: &str, session: &str, task: Option<&str>) -> DispatchInput {
    DispatchInput {
        id: id.into(),
        session_id: session.into(),
        task_id: task.map(str::to_owned),
        resources: vec![ResourceLease {
            id: "workspace".into(),
            exclusive: true,
        }],
        payload: json!({"instruction":"Verify result"}),
    }
}
impl Fixture {
    fn new(direct: bool, root: Option<&str>) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let mut db = DomainRepository::open(&temp.path().join("state")).unwrap();
        db.register(&registration()).unwrap();
        let pool = resource("pool", "seat", 1000);
        db.put_resource(&pool).unwrap();
        let p = proof(&request("one", "Worker", &pool, 100));
        let e = db.admit(&p, 1000).unwrap();
        db.approve("approve", &p, 1000).unwrap();
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
            engagement_id: e.id.clone(),
            registration_generation: 1,
            generation: 1,
            sender_mxid: "@worker:example.test".into(),
            device_id: "DEVICE_1".into(),
        };
        db.observe_matrix_transport(&transport, 1001).unwrap();
        let room = MatrixRoomObservation {
            engagement_id: e.id.clone(),
            registration_generation: 1,
            transport_generation: 1,
            room_id: if direct {
                "!direct:example.test"
            } else {
                "!project:example.test"
            }
            .into(),
            generation: 1,
            privacy: if direct {
                RoomPrivacy::Direct {
                    human_mxid: "@owner:example.test".into(),
                }
            } else {
                RoomPrivacy::Group {}
            },
            joined: BTreeSet::from(["@worker:example.test".into(), "@owner:example.test".into()]),
            invite_only: true,
            encrypted: true,
        };
        db.observe_matrix_room(&room, 1002).unwrap();
        let binding = SessionBinding {
            id: "session".into(),
            engagement_id: e.id.clone(),
            room_id: room.room_id.clone(),
            thread_root: root.map(str::to_owned),
        };
        db.resolve_verified_matrix_session(&binding, 1003).unwrap();
        db.create_canonical_task("task", &binding.id, "Verify task", 1004)
            .unwrap();
        db.register_workspace("workspace").unwrap();
        db.enqueue_dispatch(&dispatch("dispatch", &binding.id, Some("task")))
            .unwrap();
        let cap = db
            .claim_dispatch("runner", 1005, 60_000, 120_000, 8)
            .unwrap()
            .unwrap();
        db.start_dispatch(&cap, 1006).unwrap();
        Self {
            root: temp,
            db,
            engagement: e.id,
            binding,
            room,
            transport,
            cap,
        }
    }
    fn done(&mut self) {
        self.db
            .mutate_task(
                &self.cap,
                "task",
                "done",
                &TaskMutation::Transition {
                    status: TaskState::Done,
                    waiting_reason: None,
                    waiting_until: None,
                },
                1007,
            )
            .unwrap();
    }
    fn sql(&self) -> rusqlite::Connection {
        rusqlite::Connection::open(self.root.path().join("state/domain.sqlite3")).unwrap()
    }
    fn restart(self) -> Self {
        let Self {
            root,
            db,
            engagement,
            binding,
            room,
            transport,
            cap,
        } = self;
        drop(db);
        let db = DomainRepository::open(&root.path().join("state")).unwrap();
        Self {
            root,
            db,
            engagement,
            binding,
            room,
            transport,
            cap,
        }
    }
    fn reserve(&mut self, call: &str) -> FileDeliveryAdmission {
        self.db
            .reserve_file_delivery(&self.cap, &input(call), 1010)
            .unwrap()
    }
    // Explicit domain host observations. This fixture does not claim SDK or IO proof.
    fn accepted(&mut self, call: &str) -> (FileDeliveryAdmission, UploadClaim, UploadClaim) {
        let a = self.reserve(call);
        let stage = stage(call);
        self.db
            .bind_file_delivery_stage(
                &self.cap,
                &a.identity,
                a.upload.preparation.as_ref().unwrap(),
                &captured(),
                &stage,
                1011,
            )
            .unwrap();
        self.db
            .observe_upload_staged(
                &a.upload.identity,
                &stage,
                UploadStageObservation::FileAndDirectorySynced,
                1012,
            )
            .unwrap();
        let old = self
            .db
            .claim_upload(&self.cap, &a.upload.identity, 1012, 1)
            .unwrap()
            .unwrap();
        let claim = self
            .db
            .claim_upload(&self.cap, &a.upload.identity, 1013, 60000)
            .unwrap()
            .unwrap();
        let send = self.db.begin_upload(&self.cap, &claim, 1014).unwrap();
        self.db
            .record_upload_acceptance(
                &a.upload.identity,
                claim.fence(),
                &stage,
                &upload_acceptance(),
                1015,
            )
            .unwrap();
        drop(send);
        (a, claim, old)
    }
    fn publication(
        &mut self,
        call: &str,
    ) -> (
        FileDeliveryAdmission,
        FilePublicationClaim,
        FilePublicationSend,
    ) {
        let (a, upload, old_upload) = self.accepted(call);
        let claim = self
            .db
            .claim_file_publication(&self.cap, &a.identity, 1020, 60000)
            .unwrap()
            .unwrap();
        let send = self
            .db
            .begin_file_publication(&self.cap, &claim, 1021)
            .unwrap();
        assert!(send.matches_upload_claim(&upload));
        assert!(!send.matches_upload_claim(&old_upload));
        assert!(send.matches_claim(&claim));
        (a, claim, send)
    }
}
fn input(call: &str) -> FileDeliveryRequest {
    FileDeliveryRequest {
        call_id: call.into(),
        request_digest: "1".repeat(64),
        filename: "结果.txt".into(),
        caption: Some("private caption".into()),
    }
}
fn captured() -> CapturedFile {
    CapturedFile {
        size: 12,
        sha256: "2".repeat(64),
    }
}
fn stage(call: &str) -> StageCommitment {
    StageCommitment {
        namespace_digest: "3".repeat(64),
        operation_id: call.into(),
        receipt_digest: "4".repeat(64),
        len: 12,
    }
}
fn upload_acceptance() -> UploadAcceptance {
    UploadAcceptance {
        receipt_id: "private_upload".into(),
        receipt_digest: "5".repeat(64),
    }
}
fn acceptance(send: &FilePublicationSend) -> FileDeliveryAcceptance {
    FileDeliveryAcceptance {
        transaction_id: send.locator().transaction_id.clone(),
        content_digest: send.locator().content_digest.clone(),
        event_id: "$accepted".into(),
        receipt_id: "private_event".into(),
        receipt_digest: "6".repeat(64),
    }
}
fn count(sql: &rusqlite::Connection, table: &str) -> usize {
    sql.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

#[test]
fn native_file_delivery_metadata_atomic() {
    let mut f = Fixture::new(true, Some("$root"));
    let sql = f.sql();
    sql.execute_batch("CREATE TRIGGER refuse_file BEFORE INSERT ON file_deliveries BEGIN SELECT RAISE(ABORT,'fixture rollback'); END;").unwrap();
    assert!(
        f.db.reserve_file_delivery(&f.cap, &input("one"), 1010)
            .is_err()
    );
    assert_eq!(count(&sql, "file_deliveries"), 0);
    assert_eq!(count(&sql, "file_uploads"), 0);
    sql.execute_batch("DROP TRIGGER refuse_file;").unwrap();
    let a = f.reserve("one");
    assert!(a.upload.preparation.is_some());
    let r = f.reserve("one");
    assert!(r.upload.preparation.is_none());
    assert!(r.receipt.replayed);
    assert_eq!(r.identity.id(), a.identity.id());
    for mode in ["filename", "caption", "selection"] {
        let mut changed = input("one");
        match mode {
            "filename" => changed.filename = "other.txt".into(),
            "caption" => changed.caption = Some("changed".into()),
            _ => changed.request_digest = "7".repeat(64),
        }
        assert!(
            matches!(
                f.db.reserve_file_delivery(&f.cap, &changed, 1011),
                Err(Error::Conflict)
            ),
            "{mode}"
        );
    }
    let mut foreign = f.cap.clone();
    foreign.secret = "8".repeat(64);
    assert!(matches!(
        f.db.reserve_file_delivery(&foreign, &input("one"), 1011),
        Err(Error::RunnerAuthority)
    ));
    // Existing upload-only rows cannot be retrospectively assigned file metadata.
    let request = input("legacy");
    let legacy = UploadRequest {
        call_id: request.call_id.clone(),
        request_digest: hagency_core::canonical::payload_digest(&json!([
            "file_request_v1",
            request
        ]))
        .unwrap(),
        metadata: AttachmentMetadata {
            filename: request.filename.clone(),
            mime_type: Some(FILE_MIME.into()),
            declared_size: None,
        },
    };
    f.db.reserve_upload(&f.cap, &legacy, 1011).unwrap();
    assert!(matches!(
        f.db.reserve_file_delivery(&f.cap, &request, 1012),
        Err(Error::Conflict)
    ));
    assert_eq!(count(&sql, "file_deliveries"), 1);
    drop(sql);
    drop(f.reserve("lost"));
    f = f.restart();
    let replay = f.reserve("lost");
    assert!(replay.upload.preparation.is_none());
    assert!(
        f.db.restore_file_delivery(&f.cap, &input("lost"))
            .unwrap()
            .is_some()
    );
    assert!(
        f.db.reserve_file_delivery(&f.cap, &input("new"), 2000)
            .is_err()
    );
}

#[test]
fn native_file_delivery_capture_binding() {
    let mut f = Fixture::new(true, None);
    let a = f.reserve("a");
    let b = f.reserve("b");
    assert!(matches!(
        f.db.bind_file_delivery_stage(
            &f.cap,
            &a.identity,
            b.upload.preparation.as_ref().unwrap(),
            &captured(),
            &stage("a"),
            1011
        ),
        Err(Error::RunnerAuthority)
    ));
    let mut oversized = stage("a");
    oversized.len = MAX_FILE_BYTES + 1;
    assert!(
        f.db.bind_file_delivery_stage(
            &f.cap,
            &a.identity,
            a.upload.preparation.as_ref().unwrap(),
            &captured(),
            &oversized,
            1011
        )
        .is_err()
    );
    assert!(
        f.db.upload_stage_commitment(&a.upload.identity)
            .unwrap()
            .is_none()
    );
    let sql = f.sql();
    sql.execute_batch("CREATE TRIGGER refuse_capture BEFORE UPDATE OF captured ON file_deliveries BEGIN SELECT RAISE(ABORT,'fixture rollback'); END;").unwrap();
    assert!(
        f.db.bind_file_delivery_stage(
            &f.cap,
            &a.identity,
            a.upload.preparation.as_ref().unwrap(),
            &captured(),
            &stage("a"),
            1011
        )
        .is_err()
    );
    assert!(
        f.db.upload_stage_commitment(&a.upload.identity)
            .unwrap()
            .is_none()
    );
    assert!(
        f.db.inspect_file_delivery(&f.cap, a.identity.id())
            .unwrap()
            .captured
            .is_none()
    );
    sql.execute_batch("DROP TRIGGER refuse_capture;").unwrap();
    let bound =
        f.db.bind_file_delivery_stage(
            &f.cap,
            &a.identity,
            a.upload.preparation.as_ref().unwrap(),
            &captured(),
            &stage("a"),
            1012,
        )
        .unwrap();
    assert_eq!(bound.captured, Some(captured()));
    assert!(
        f.db.bind_file_delivery_stage(
            &f.cap,
            &a.identity,
            a.upload.preparation.as_ref().unwrap(),
            &captured(),
            &stage("a"),
            1013
        )
        .unwrap()
        .replayed
    );
    for mode in ["size", "hash", "stage"] {
        let mut capture = captured();
        let mut changed = stage("a");
        match mode {
            "size" => {
                capture.size += 1;
                changed.len += 1;
            }
            "hash" => capture.sha256 = "8".repeat(64),
            _ => changed.receipt_digest = "8".repeat(64),
        }
        assert!(
            f.db.bind_file_delivery_stage(
                &f.cap,
                &a.identity,
                a.upload.preparation.as_ref().unwrap(),
                &capture,
                &changed,
                1014
            )
            .is_err(),
            "{mode}"
        );
    }
    assert_eq!(
        f.db.inspect_file_delivery(&f.cap, a.identity.id())
            .unwrap()
            .captured,
        Some(captured())
    );
    // The upload-only seam must not allow backfilling original facts after IO.
    f.db.bind_upload_stage(
        &f.cap,
        b.upload.preparation.as_ref().unwrap(),
        &stage("b"),
        1015,
    )
    .unwrap();
    f.db.observe_upload_staged(
        &b.upload.identity,
        &stage("b"),
        UploadStageObservation::FileAndDirectorySynced,
        1016,
    )
    .unwrap();
    assert!(matches!(
        f.db.bind_file_delivery_stage(
            &f.cap,
            &b.identity,
            b.upload.preparation.as_ref().unwrap(),
            &captured(),
            &stage("b"),
            1017
        ),
        Err(Error::State)
    ));
    assert!(
        f.db.inspect_file_delivery(&f.cap, b.identity.id())
            .unwrap()
            .captured
            .is_none()
    );
}

#[test]
fn native_file_delivery_publication_nonreissue() {
    let mut f = Fixture::new(true, Some("$root"));
    let pending = f.reserve("pending");
    assert!(
        f.db.claim_file_publication(&f.cap, &pending.identity, 1011, 100)
            .unwrap()
            .is_none()
    );
    let (cancelled, _, _) = f.accepted("accepted_cancelled");
    let receipt =
        f.db.cancel_file_delivery(&cancelled.identity, FileDeliveryFailure::Cancelled, 1016)
            .unwrap();
    assert_eq!(receipt.upload, UploadState::Accepted);
    assert_eq!(receipt.event, FileEventState::Pending);
    assert_eq!(receipt.status, FileDeliveryStatus::Failed);
    assert!(
        f.db.claim_file_publication(&f.cap, &cancelled.identity, 1017, 100)
            .is_err()
    );
    let (a, upload, old_upload) = f.accepted("one");
    let before = f.db.canonical_task("task").unwrap();
    let status = f.db.inspect_file_delivery(&f.cap, a.identity.id()).unwrap();
    assert_eq!(status.upload, UploadState::Accepted);
    assert_eq!(status.event, FileEventState::Pending);
    assert_eq!(status.status, FileDeliveryStatus::Queued);
    let old =
        f.db.claim_file_publication(&f.cap, &a.identity, 1020, 10)
            .unwrap()
            .unwrap();
    assert!(
        f.db.claim_file_publication(&f.cap, &a.identity, 1021, 10)
            .unwrap()
            .is_none()
    );
    let claim =
        f.db.claim_file_publication(&f.cap, &a.identity, 1030, 100)
            .unwrap()
            .unwrap();
    assert_eq!(claim.fence(), old.fence() + 1);
    assert!(f.db.begin_file_publication(&f.cap, &old, 1031).is_err());
    let send = f.db.begin_file_publication(&f.cap, &claim, 1031).unwrap();
    assert!(send.matches_upload_claim(&upload));
    assert!(!send.matches_upload_claim(&old_upload));
    assert!(!send.matches_claim(&old));
    assert_eq!(send.metadata().caption, input("one").caption);
    assert_eq!(send.locator().route.thread_root, Some("$root".into()));
    assert_eq!(send.captured(), &captured());
    let (_, other_request, _) = f.accepted("different_request");
    assert!(!send.matches_upload_claim(&other_request));
    let mut other_fixture = Fixture::new(true, Some("$root"));
    let (_, other_cap, _) = other_fixture.accepted("one");
    assert!(!send.matches_upload_claim(&other_cap));
    assert!(f.db.begin_file_publication(&f.cap, &claim, 1032).is_err());
    f.db.validate_file_publication(&f.cap, &claim, 1032)
        .unwrap();
    let retained_identity = send.identity().clone();
    assert_eq!(retained_identity.id(), a.identity.id());
    drop(send);
    assert!(
        f.db.validate_file_publication(&f.cap, &claim, 1130)
            .is_err()
    );
    assert!(
        f.db.claim_file_publication(&f.cap, &a.identity, 1131, 100)
            .unwrap()
            .is_none()
    );
    f.db.mark_file_publication_uncertain(&retained_identity, claim.fence(), 1131)
        .unwrap();
    assert!(f.db.begin_file_publication(&f.cap, &claim, 1132).is_err());
    let r =
        f.db.cancel_file_delivery(&a.identity, FileDeliveryFailure::Cancelled, 1133)
            .unwrap();
    assert_eq!(r.status, FileDeliveryStatus::OutcomeUnknown);
    assert_eq!(r.upload, UploadState::Accepted);
    assert_eq!(f.db.canonical_task("task").unwrap().status, before.status);
    f = f.restart();
    assert_eq!(
        f.db.inspect_file_delivery(&f.cap, a.identity.id())
            .unwrap()
            .event,
        FileEventState::WritePossible
    );
}

#[test]
fn native_file_delivery_current_scope() {
    for mode in [
        "done",
        "epoch",
        "lease",
        "workspace",
        "registration",
        "room",
        "transport",
        "cancel",
    ] {
        let mut f = Fixture::new(true, None);
        let (a, claim, _send) = f.publication("one");
        match mode {
            "done" => f.done(),
            "epoch" => {
                f.sql()
                    .execute(
                        "UPDATE canonical_tasks SET config=json_set(config,'$.execution_epoch',9) WHERE id='task'",
                        [],
                    )
                    .unwrap();
            }
            "lease" => {
                f.sql()
                    .execute("UPDATE runner_dispatches SET lease_until=1021", [])
                    .unwrap();
            }
            "workspace" => {
                f.sql().execute("DELETE FROM resource_leases", []).unwrap();
            }
            "registration" => {
                let mut r = registration();
                r.generation += 1;
                f.db.register(&r).unwrap();
            }
            "room" => {
                let mut r = f.room.clone();
                r.generation += 1;
                r.joined.insert("@stranger:example.test".into());
                f.db.observe_matrix_room(&r, 1022).unwrap();
            }
            "transport" => {
                let mut t = f.transport.clone();
                t.generation += 1;
                t.device_id = "CHANGED".into();
                f.db.observe_matrix_transport(&t, 1022).unwrap();
            }
            _ => {
                f.db.cancel_file_delivery(&a.identity, FileDeliveryFailure::Cancelled, 1022)
                    .unwrap();
            }
        }
        assert!(
            f.db.validate_file_publication(&f.cap, &claim, 1023)
                .is_err(),
            "{mode}"
        );
        assert!(
            f.db.claim_file_publication(&f.cap, &a.identity, 1023, 100)
                .is_err(),
            "{mode}"
        );
        let r = f.db.inspect_file_delivery(&f.cap, a.identity.id()).unwrap();
        assert_eq!(r.upload, UploadState::Accepted);
        assert_ne!(r.status, FileDeliveryStatus::Delivered);
    }
}

#[test]
fn native_file_delivery_historical_settlement() {
    let mut f = Fixture::new(true, None);
    let (a, claim, send) = f.publication("one");
    let observed = acceptance(&send);
    let locator = send.locator().clone();
    let settlement =
        f.db.restore_file_delivery_settlement(&locator)
            .unwrap()
            .unwrap();
    let mut changed = observed.clone();
    changed.content_digest = "9".repeat(64);
    assert!(
        f.db.record_file_delivery_settlement(&settlement, &changed, 1022)
            .is_err()
    );
    let sql = f.sql();
    sql.execute_batch("CREATE TRIGGER refuse_delivery BEFORE UPDATE OF acceptance ON file_deliveries BEGIN SELECT RAISE(ABORT,'fixture lost receipt'); END;").unwrap();
    assert!(
        f.db.record_file_delivery_settlement(&settlement, &observed, 1022)
            .is_err()
    );
    assert_eq!(
        f.db.inspect_file_delivery_settlement(&settlement)
            .unwrap()
            .event,
        FileEventState::WritePossible
    );
    sql.execute_batch("DROP TRIGGER refuse_delivery;").unwrap();
    drop(sql);
    f.db.cancel_file_delivery(&a.identity, FileDeliveryFailure::Cancelled, 1023)
        .unwrap();
    f.done();
    drop(settlement);
    drop(send);
    drop(claim);
    f = f.restart();
    // The reopened lookup and first receipt commit need no capability argument.
    let settlement =
        f.db.restore_file_delivery_settlement(&locator)
            .unwrap()
            .unwrap();
    let delivered =
        f.db.record_file_delivery_settlement(&settlement, &observed, 2000)
            .unwrap();
    assert!(!delivered.replayed);
    assert_eq!(delivered.status, FileDeliveryStatus::Delivered);
    assert!(delivered.cancel_requested);
    assert!(
        f.db.record_file_delivery_settlement(&settlement, &observed, 2001)
            .unwrap()
            .replayed
    );
    let mut changed = observed.clone();
    changed.event_id = "$other".into();
    assert!(matches!(
        f.db.record_file_delivery_settlement(&settlement, &changed, 2002),
        Err(Error::Conflict)
    ));
    for mode in [
        "fence",
        "upload",
        "stage",
        "route",
        "digest",
        "transaction",
        "receipt",
    ] {
        let mut bad = locator.clone();
        match mode {
            "fence" => bad.fence += 1,
            "upload" => bad.upload_fence += 1,
            "stage" => bad.stage.receipt_digest = "9".repeat(64),
            "route" => bad.route.room_generation += 1,
            "digest" => bad.content_digest = "9".repeat(64),
            "transaction" => bad.transaction_id = "other".into(),
            _ => bad.upload_receipt_digest = "9".repeat(64),
        }
        assert!(
            f.db.restore_file_delivery_settlement(&bad).is_err(),
            "{mode}"
        );
    }
    assert!(
        f.db.claim_file_publication(&f.cap, &a.identity, 2003, 100)
            .is_err()
    );
}

#[test]
fn native_file_publication_content_association() {
    let mut f = Fixture::new(true, Some("$original_root"));
    let (a, claim, send) = f.publication("content");
    let locator = send.locator().clone();
    let request = send.metadata().clone();
    let capture = send.captured().clone();
    let observed = acceptance(&send);
    let original_digest =
        hagency_core::canonical::payload_digest(&json!([&request, &capture])).unwrap();
    for mode in [
        "call",
        "digest",
        "filename",
        "caption",
        "no_caption",
        "size",
        "hash",
    ] {
        let mut changed = request.clone();
        let mut facts = capture.clone();
        match mode {
            "call" => changed.call_id = "other".into(),
            "digest" => changed.request_digest = "7".repeat(64),
            "filename" => changed.filename = "other.txt".into(),
            "caption" => changed.caption = Some("other caption".into()),
            "no_caption" => changed.caption = None,
            "size" => facts.size += 1,
            _ => facts.sha256 = "8".repeat(64),
        }
        changed.validate().unwrap();
        facts.validate().unwrap();
        // A coherent replacement and its newly computed content hash do not
        // establish association with the independently retained original row.
        // The actual private SDK journal mutation test belongs to ADR098.
        let replacement = json!([&changed, &facts]);
        let replacement_digest = hagency_core::canonical::payload_digest(&replacement).unwrap();
        assert_ne!(replacement_digest, original_digest);
        let (changed, facts): (FileDeliveryRequest, CapturedFile) =
            serde_json::from_value(replacement).unwrap();
        assert!(
            matches!(
                f.db.restore_file_delivery_settlement_for_content(&locator, &changed, &facts),
                Err(Error::Conflict)
            ),
            "{mode}"
        );
        assert_eq!(
            f.db.inspect_file_delivery(&f.cap, a.identity.id())
                .unwrap()
                .event,
            FileEventState::WritePossible
        );
    }
    let mut absent = locator.clone();
    absent.delivery_id = format!("file_{}", "0".repeat(32));
    assert!(
        f.db.restore_file_delivery_settlement_for_content(&absent, &request, &capture)
            .unwrap()
            .is_none()
    );
    let mut wrong_locator = locator.clone();
    wrong_locator.content_digest = original_digest;
    assert!(
        f.db.restore_file_delivery_settlement_for_content(&wrong_locator, &request, &capture)
            .is_err()
    );
    f.db.cancel_file_delivery(&a.identity, FileDeliveryFailure::Cancelled, 1023)
        .unwrap();
    f.done();
    drop(send);
    drop(claim);
    f = f.restart();
    let settled =
        f.db.restore_file_delivery_settlement_for_content(&locator, &request, &capture)
            .unwrap()
            .unwrap();
    assert_eq!(
        f.db.inspect_file_delivery_settlement(&settled)
            .unwrap()
            .event,
        FileEventState::WritePossible
    );
    let receipt =
        f.db.record_file_delivery_settlement(&settled, &observed, 2000)
            .unwrap();
    assert_eq!(receipt.event, FileEventState::Delivered);
    assert!(!receipt.replayed);
    assert!(receipt.cancel_requested);
    assert!(
        f.db.record_file_delivery_settlement(&settled, &observed, 2001)
            .unwrap()
            .replayed
    );
    assert!(
        f.db.claim_file_publication(&f.cap, &a.identity, 2002, 100)
            .is_err()
    );
    let mut changed = request.clone();
    changed.filename = "changed_after_delivered.txt".into();
    assert!(matches!(
        f.db.restore_file_delivery_settlement_for_content(&locator, &changed, &capture),
        Err(Error::Conflict)
    ));
}

#[test]
fn native_file_delivery_status_bounds() {
    let mut f = Fixture::new(true, None);
    let a = f.reserve("one");
    let receipt = f.db.inspect_file_delivery(&f.cap, a.identity.id()).unwrap();
    assert_eq!(receipt.status, FileDeliveryStatus::Queued);
    let mut other = f.cap.clone();
    other.secret = "9".repeat(64);
    assert!(matches!(
        f.db.inspect_file_delivery(&other, a.identity.id()),
        Err(Error::RunnerAuthority)
    ));
    assert!(matches!(
        f.db.inspect_file_delivery(&f.cap, &format!("file_{}", "0".repeat(32))),
        Err(Error::RunnerAuthority)
    ));
    let encoded = serde_json::to_string(&receipt).unwrap();
    for hidden in [
        "private caption",
        "request_digest",
        "room_id",
        "workspace",
        "secret",
        "namespace",
        "mxc:",
    ] {
        assert!(!encoded.contains(hidden));
    }
    assert_eq!(
        f.db.cancel_file_delivery(&a.identity, FileDeliveryFailure::SourceRefused, 1011)
            .unwrap()
            .status,
        FileDeliveryStatus::Failed
    );
    for i in 1..MAX_DISPATCH_FILE_DELIVERIES {
        f.reserve(&format!("n{i}"));
    }
    assert!(matches!(
        f.db.reserve_file_delivery(&f.cap, &input("full"), 1012),
        Err(Error::Capacity)
    ));
    assert!(f.reserve("one").upload.preparation.is_none());
    // Synthetic retained history exercises only counters; real grants above use actual dispatches.
    let sql = f.sql();
    sql.execute_batch("PRAGMA foreign_keys=OFF; WITH RECURSIVE n(x) AS (SELECT 16 UNION ALL SELECT x+1 FROM n WHERE x<4095) INSERT INTO file_deliveries(id,upload_id,dispatch_id,call_id,request,request_hash,event_state,transaction_id,created_at,updated_at) SELECT 'history_'||x,'old_upload_'||x,'retired_'||x,'old',request,request_hash,'pending','old_tx_'||x,0,0 FROM n CROSS JOIN file_deliveries WHERE file_deliveries.id=(SELECT id FROM file_deliveries LIMIT 1); UPDATE file_deliveries SET dispatch_id='retired' WHERE call_id='n1';").unwrap();
    assert_eq!(count(&sql, "file_deliveries"), MAX_FILE_DELIVERIES);
    assert!(matches!(
        f.db.reserve_file_delivery(&f.cap, &input("global"), 1013),
        Err(Error::Capacity)
    ));
    assert!(f.reserve("one").upload.preparation.is_none());
    let long_root = format!("${}", "r".repeat(MAX_FILE_RECORD_BYTES));
    let mut large = Fixture::new(true, Some(&long_root));
    assert!(matches!(
        large
            .db
            .reserve_file_delivery(&large.cap, &input("encoded_bound"), 1010),
        Err(Error::Capacity)
    ));
    assert_eq!(count(&large.sql(), "file_deliveries"), 0);
    assert_eq!(count(&large.sql(), "file_uploads"), 0);
    let mut request = input("bad");
    request.filename = "x".repeat(256);
    assert!(request.validate().is_err());
    request = input("bad");
    request.caption = Some("x".repeat(1001));
    assert!(request.validate().is_err());
    let mut captured = captured();
    captured.size = MAX_FILE_BYTES + 1;
    assert!(captured.validate().is_err());
    captured.size = 0;
    captured.sha256 = "A".repeat(64);
    assert!(captured.validate().is_err());
}

#[test]
fn native_file_delivery_schema_migration() {
    let mut f = Fixture::new(true, None);
    let (a, _, _) = f.accepted("old");
    let original = f.db.inspect_upload(&a.upload.identity).unwrap();
    let root = f.root;
    drop(f.db);
    let path = root.path().join("state/domain.sqlite3");
    let sql = rusqlite::Connection::open(&path).unwrap();
    sql.execute_batch(
        "DROP TABLE approval_responses; DROP TABLE received_files; DROP TABLE file_deliveries; PRAGMA user_version=19;",
    )
    .unwrap();
    drop(sql);
    let db = DomainRepository::open(&root.path().join("state")).unwrap();
    assert_eq!(db.inspect_upload(&a.upload.identity).unwrap(), original);
    let sql = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        sql.pragma_query_value(None, "user_version", |r| r.get::<_, u64>(0))
            .unwrap(),
        23
    );
    assert_eq!(count(&sql, "file_deliveries"), 0);
    drop(db);
    drop(sql);
    drop(DomainRepository::open(&root.path().join("state")).unwrap());
    let sql = rusqlite::Connection::open(&path).unwrap();
    sql.execute_batch("ALTER TABLE file_deliveries DROP COLUMN updated_at;")
        .unwrap();
    drop(sql);
    assert!(matches!(
        DomainRepository::open(&root.path().join("state")),
        Err(Error::Schema)
    ));
}

#[test]
fn native_file_delivery_completion_guard() {
    // A begun publication is neither delivered nor failed: its external write
    // outcome is unknown, so ordinary dispatch completion refuses.
    let mut f = Fixture::new(true, None);
    let (a, claim, send) = f.publication("one");
    assert!(matches!(
        f.db.complete_dispatch(&f.cap, &json!({"result": "done"}), 1030),
        Err(Error::State)
    ));
    assert_eq!(
        f.db.inspect_file_delivery(&f.cap, a.identity.id())
            .unwrap()
            .event,
        FileEventState::WritePossible
    );
    let observed = acceptance(&send);
    let settlement =
        f.db.restore_file_delivery_settlement(send.locator())
            .unwrap()
            .unwrap();
    f.db.record_file_delivery_settlement(&settlement, &observed, 1031)
        .unwrap();
    drop(send);
    drop(claim);
    f.db.complete_dispatch(&f.cap, &json!({"result": "done"}), 1032)
        .unwrap();
    let state: String = f
        .sql()
        .query_row(
            "SELECT state FROM runner_dispatches WHERE id='dispatch'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(state, "completed");

    // A cancelled publication is still a possible external write. Its recorded
    // failure settles nothing; completion stays refused.
    let mut f = Fixture::new(true, None);
    let (a, claim, send) = f.publication("two");
    drop(send);
    drop(claim);
    let cancelled =
        f.db.cancel_file_delivery(&a.identity, FileDeliveryFailure::Cancelled, 1030)
            .unwrap();
    assert_eq!(cancelled.status, FileDeliveryStatus::OutcomeUnknown);
    assert!(matches!(
        f.db.complete_dispatch(&f.cap, &json!({"result": "done"}), 1031),
        Err(Error::State)
    ));

    // An accepted upload whose event was never begun is settled by its failure.
    let mut f = Fixture::new(true, None);
    let (a, _upload, _old) = f.accepted("four");
    assert!(matches!(
        f.db.complete_dispatch(&f.cap, &json!({}), 1030),
        Err(Error::State)
    ));
    let failed =
        f.db.cancel_file_delivery(&a.identity, FileDeliveryFailure::Cancelled, 1031)
            .unwrap();
    assert_eq!(failed.status, FileDeliveryStatus::Failed);
    f.db.complete_dispatch(&f.cap, &json!({}), 1032).unwrap();

    // A reserved delivery that never reached publication is still unsettled.
    let mut f = Fixture::new(true, None);
    let a = f.reserve("three");
    assert!(matches!(
        f.db.complete_dispatch(&f.cap, &json!({}), 1030),
        Err(Error::State)
    ));
    f.db.cancel_file_delivery(&a.identity, FileDeliveryFailure::SourceRefused, 1031)
        .unwrap();
    f.db.complete_dispatch(&f.cap, &json!({}), 1032).unwrap();
}
