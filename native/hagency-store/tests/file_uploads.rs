mod common;
use common::*;
use hagency_core::{attachments::AttachmentMetadata, replies::*, tasks::*, uploads::*};
use hagency_store::{DomainRepository, EffectOutcome, Error, UploadAdmission, UploadIdentity};
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
    fn reserve(&mut self, call: &str) -> UploadAdmission {
        self.db
            .reserve_upload(&self.cap, &input(call), 1010)
            .unwrap()
    }
    fn ready(&mut self, call: &str) -> (UploadIdentity, StageCommitment) {
        let a = self.reserve(call);
        let stage = stage(call);
        self.db
            .bind_upload_stage(&self.cap, a.preparation.as_ref().unwrap(), &stage, 1011)
            .unwrap();
        self.db
            .observe_upload_staged(
                &a.identity,
                &stage,
                UploadStageObservation::FileAndDirectorySynced,
                1012,
            )
            .unwrap();
        (a.identity, stage)
    }
}

fn input(call: &str) -> UploadRequest {
    UploadRequest {
        call_id: call.into(),
        request_digest: "1".repeat(64),
        metadata: AttachmentMetadata {
            filename: "结果.txt".into(),
            mime_type: Some("text/plain".into()),
            declared_size: Some(12),
        },
    }
}
fn stage(call: &str) -> StageCommitment {
    StageCommitment {
        namespace_digest: "2".repeat(64),
        operation_id: call.into(),
        receipt_digest: "3".repeat(64),
        len: 12,
    }
}
fn acceptance() -> UploadAcceptance {
    UploadAcceptance {
        receipt_id: "private_receipt".into(),
        receipt_digest: "4".repeat(64),
    }
}

#[test]
fn native_upload_send_claim_association() {
    let mut f = Fixture::new(true, None);
    let (a, _) = f.ready("first");
    let (b, _) = f.ready("second");
    let claim_a = f.db.claim_upload(&f.cap, &a, 1020, 1000).unwrap().unwrap();
    let claim_b = f.db.claim_upload(&f.cap, &b, 1020, 1000).unwrap().unwrap();
    assert_eq!(claim_a.fence(), claim_b.fence());
    let send_a = f.db.begin_upload(&f.cap, &claim_a, 1021).unwrap();
    let send_b = f.db.begin_upload(&f.cap, &claim_b, 1021).unwrap();
    assert!(send_a.matches_claim(&claim_a));
    assert!(send_a.matches_claim(&claim_a.clone()));
    assert!(send_b.matches_claim(&claim_b));
    assert!(!send_a.matches_claim(&claim_b));
    assert!(!send_b.matches_claim(&claim_a));
    // A replacement claim for the same upload must not match an older fence.
    let (c, _) = f.ready("replacement");
    let old = f.db.claim_upload(&f.cap, &c, 1022, 1).unwrap().unwrap();
    let current = f.db.claim_upload(&f.cap, &c, 1024, 100).unwrap().unwrap();
    let send_c = f.db.begin_upload(&f.cap, &current, 1025).unwrap();
    assert!(send_c.matches_claim(&current));
    assert!(!send_c.matches_claim(&old));
    assert!(f.db.begin_upload(&f.cap, &old, 1026).is_err());
    f.db.validate_upload_send(&f.cap, &claim_a, 1026).unwrap();
    f.db.cancel_upload(&a, 1027).unwrap();
    // Historical association survives cancellation; current permission does not.
    assert!(send_a.matches_claim(&claim_a));
    assert!(f.db.validate_upload_send(&f.cap, &claim_a, 1028).is_err());
    // Unrelated current work remains valid and still cannot stand in for A.
    f.db.validate_upload_send(&f.cap, &claim_b, 1028).unwrap();
    assert!(!send_a.matches_claim(&claim_b));
    assert!(f.db.claim_upload(&f.cap, &a, 1028, 100).is_err());
}

#[test]
fn native_upload_reservation() {
    // Any future transport/console deserializer or secret Debug derivation
    // makes these trait selections ambiguous at compile time.
    trait Private<A> {
        fn check() {}
    }
    impl<T: ?Sized> Private<()> for T {}
    impl<T: serde::de::DeserializeOwned> Private<u8> for T {}
    impl<T: serde::Serialize> Private<u16> for T {}
    impl<T: std::fmt::Debug> Private<u32> for T {}
    let _ = <hagency_store::UploadPreparation as Private<_>>::check;
    let _ = <hagency_store::UploadIdentity as Private<_>>::check;
    let _ = <hagency_store::UploadClaim as Private<_>>::check;
    let _ = <hagency_store::UploadSend as Private<_>>::check;
    let _ = <UploadStageObservation as Private<_>>::check;
    let mut f = Fixture::new(true, None);
    let a = f.reserve("one");
    assert!(a.preparation.is_some());
    let again = f.reserve("one");
    assert!(again.preparation.is_none());
    assert!(again.receipt.replayed);
    assert_eq!(a.identity.id(), again.identity.id());
    let mut changed = input("one");
    changed.metadata.filename = "changed".into();
    assert!(matches!(
        f.db.reserve_upload(&f.cap, &changed, 1011),
        Err(Error::Conflict)
    ));
    let mut wrong = f.cap.clone();
    wrong.secret = "b".repeat(64);
    assert!(matches!(
        f.db.reserve_upload(&wrong, &input("one"), 1011),
        Err(Error::RunnerAuthority)
    ));
    assert!(matches!(
        f.db.restore_upload(&wrong, &input("one")),
        Err(Error::RunnerAuthority)
    ));
    // A committed but discarded original return cannot mint another capture grant.
    drop(f.reserve("lost"));
    f = f.restart();
    let restored =
        f.db.restore_upload(&f.cap, &input("lost"))
            .unwrap()
            .unwrap();
    let receipt = f.db.inspect_upload(&restored).unwrap();
    assert_eq!(receipt.stage, UploadStageState::Unbound);
    let replay = f.db.reserve_upload(&f.cap, &input("lost"), 2000).unwrap();
    assert!(replay.preparation.is_none());
    assert!(
        f.db.reserve_upload(&f.cap, &input("new_after_restart"), 2000)
            .is_err()
    );
    let encoded = serde_json::to_string(&receipt).unwrap();
    for hidden in [
        "@owner",
        "direct",
        "workspace",
        "mxc:",
        "receipt_digest",
        "scope",
        "capability",
        "namespace",
        "filename",
    ] {
        assert!(!encoded.contains(hidden));
    }
}

#[test]
fn native_upload_staging_and_send() {
    let mut f = Fixture::new(true, None);
    let a = f.reserve("one");
    let s = stage("one");
    assert!(
        f.db.claim_upload(&f.cap, &a.identity, 1011, 100)
            .unwrap()
            .is_none()
    );
    assert!(
        f.db.observe_upload_staged(
            &a.identity,
            &s,
            UploadStageObservation::FileAndDirectorySynced,
            1011
        )
        .is_err()
    );
    f.db.bind_upload_stage(&f.cap, a.preparation.as_ref().unwrap(), &s, 1012)
        .unwrap();
    let mut changed = s.clone();
    changed.receipt_digest = "5".repeat(64);
    assert!(matches!(
        f.db.bind_upload_stage(&f.cap, a.preparation.as_ref().unwrap(), &changed, 1013),
        Err(Error::Conflict)
    ));
    assert!(
        f.db.bind_upload_stage(&f.cap, a.preparation.as_ref().unwrap(), &s, 1013)
            .unwrap()
            .replayed
    );
    let b = f.reserve("two");
    assert!(matches!(
        f.db.bind_upload_stage(&f.cap, b.preparation.as_ref().unwrap(), &s, 1013),
        Err(Error::Conflict)
    ));
    let r =
        f.db.observe_upload_staged(
            &a.identity,
            &s,
            UploadStageObservation::OutcomeUnknown,
            1014,
        )
        .unwrap();
    assert_eq!(r.stage, UploadStageState::Unknown);
    assert!(
        f.db.claim_upload(&f.cap, &a.identity, 1015, 100)
            .unwrap()
            .is_none()
    );
    f.db.observe_upload_staged(
        &a.identity,
        &s,
        UploadStageObservation::FileAndDirectorySynced,
        1016,
    )
    .unwrap();
    let old =
        f.db.claim_upload(&f.cap, &a.identity, 1020, 10)
            .unwrap()
            .unwrap();
    assert!(
        f.db.claim_upload(&f.cap, &a.identity, 1021, 10)
            .unwrap()
            .is_none()
    );
    let claim =
        f.db.claim_upload(&f.cap, &a.identity, 1030, 100)
            .unwrap()
            .unwrap();
    assert_eq!(claim.fence(), old.fence() + 1);
    assert!(f.db.begin_upload(&f.cap, &old, 1031).is_err());
    assert!(
        f.db.record_upload_acceptance(&a.identity, old.fence(), &s, &acceptance(), 1031)
            .is_err()
    );
    assert!(
        f.db.record_upload_acceptance(&a.identity, claim.fence(), &s, &acceptance(), 1031)
            .is_err()
    );
    let send = f.db.begin_upload(&f.cap, &claim, 1032).unwrap();
    assert_eq!(send.stage().len, 12);
    assert_eq!(send.route().thread_root, None);
    assert!(matches!(send.route().privacy, RoomPrivacy::Direct { .. }));
    assert!(f.db.begin_upload(&f.cap, &claim, 1033).is_err());
    f.db.validate_upload_send(&f.cap, &claim, 1033).unwrap();
    assert!(
        f.db.claim_upload(&f.cap, &a.identity, 1200, 100)
            .unwrap()
            .is_none()
    );
    assert!(f.db.validate_upload_send(&f.cap, &claim, 1200).is_err());
    for observation in [
        UploadStageObservation::OutcomeUnknown,
        UploadStageObservation::FileAndDirectorySynced,
    ] {
        assert_eq!(
            f.db.observe_upload_staged(&a.identity, &s, observation, 1201)
                .unwrap()
                .stage,
            UploadStageState::Staged
        );
    }
}

#[test]
fn native_upload_scope_fencing() {
    for mode in [
        "done",
        "epoch",
        "expiry",
        "workspace",
        "promoted",
        "member",
        "device",
        "owner",
        "registration",
        "revoke",
    ] {
        let mut f = Fixture::new(true, None);
        let (id, _) = f.ready("one");
        let claim =
            f.db.claim_upload(&f.cap, &id, 1020, 60000)
                .unwrap()
                .unwrap();
        let mut now = 1030;
        match mode {
            "done" => f.done(),
            "epoch" => {
                f.sql().execute("UPDATE canonical_tasks SET config=json_set(config,'$.execution_epoch',9) WHERE id='task'",[]).unwrap();
            }
            "expiry" => now = 61005,
            "workspace" => {
                f.sql()
                    .execute(
                        "DELETE FROM resource_leases WHERE dispatch_id='dispatch'",
                        [],
                    )
                    .unwrap();
            }
            "promoted" => {
                f.room.generation = 2;
                f.room.privacy = RoomPrivacy::Group {};
                f.room.joined.insert("@third:example.test".into());
                f.db.observe_matrix_room(&f.room, 1025).unwrap();
            }
            "member" => {
                f.db.invalidate_matrix_room(
                    &MatrixRoomInvalidation {
                        engagement_id: f.engagement.clone(),
                        registration_generation: 1,
                        transport_generation: 1,
                        room_id: f.room.room_id.clone(),
                        generation: 2,
                        reason: "owner_left".into(),
                    },
                    1025,
                )
                .unwrap();
            }
            "device" => {
                f.transport.generation = 2;
                f.transport.device_id = "NEW_DEVICE".into();
                f.db.observe_matrix_transport(&f.transport, 1025).unwrap();
            }
            "owner" => {
                f.sql()
                    .execute("UPDATE projects SET owner_mxid='@new:example.test'", [])
                    .unwrap();
            }
            "registration" => {
                let mut r = registration();
                r.generation = 2;
                f.db.register(&r).unwrap();
            }
            "revoke" => {
                f.db.revoke("revoke", &f.engagement).unwrap();
            }
            _ => unreachable!(),
        }
        assert!(f.db.begin_upload(&f.cap, &claim, now).is_err(), "{mode}");
        assert_eq!(
            f.db.inspect_upload(&id).unwrap().upload,
            UploadState::Claimed
        );
    }
    let mut f = Fixture::new(true, None);
    let (id, _) = f.ready("possible");
    let claim =
        f.db.claim_upload(&f.cap, &id, 1020, 60000)
            .unwrap()
            .unwrap();
    f.db.begin_upload(&f.cap, &claim, 1021).unwrap();
    f.room.generation = 2;
    f.room.privacy = RoomPrivacy::Group {};
    f.room.joined.insert("@third:example.test".into());
    f.db.observe_matrix_room(&f.room, 1022).unwrap();
    assert!(f.db.validate_upload_send(&f.cap, &claim, 1023).is_err());
    assert!(f.db.inspect_upload(&id).unwrap().outcome_unknown);
}

#[test]
fn native_upload_historical_settlement() {
    // Stage binding is committed before storage. Reopening can acknowledge an
    // independently retained original receipt, never obtain new preparation.
    let mut staged_fixture = Fixture::new(true, None);
    let reserved = staged_fixture.reserve("stage_lost");
    let committed = stage("stage_lost");
    staged_fixture
        .db
        .bind_upload_stage(
            &staged_fixture.cap,
            reserved.preparation.as_ref().unwrap(),
            &committed,
            1011,
        )
        .unwrap();
    staged_fixture = staged_fixture.restart();
    let stage_id = staged_fixture
        .db
        .restore_upload(&staged_fixture.cap, &input("stage_lost"))
        .unwrap()
        .unwrap();
    assert!(
        staged_fixture
            .db
            .upload_stage_commitment(&stage_id)
            .unwrap()
            .as_ref()
            == Some(&committed)
    );
    staged_fixture
        .db
        .observe_upload_staged(
            &stage_id,
            &committed,
            UploadStageObservation::FileAndDirectorySynced,
            2000,
        )
        .unwrap();
    assert!(
        staged_fixture
            .db
            .claim_upload(&staged_fixture.cap, &stage_id, 2001, 100)
            .is_err()
    );
    let mut f = Fixture::new(true, None);
    let (id, s) = f.ready("one");
    let claim = f.db.claim_upload(&f.cap, &id, 1020, 100).unwrap().unwrap();
    let send = f.db.begin_upload(&f.cap, &claim, 1021).unwrap();
    f.db.cancel_upload(&id, 1022).unwrap();
    f.db.revoke("revoke", &f.engagement).unwrap();
    assert!(f.db.validate_upload_send(&f.cap, &claim, 1023).is_err());
    let before = f.db.mark_upload_uncertain(&id, send.fence(), 1023).unwrap();
    assert!(before.outcome_unknown && before.cancel_requested);
    f = f.restart();
    let restored = f.db.restore_upload(&f.cap, &input("one")).unwrap().unwrap();
    assert!(f.db.inspect_upload(&restored).unwrap().outcome_unknown);
    assert_eq!(f.db.upload_fence(&restored).unwrap(), send.fence());
    assert!(f.db.upload_stage_commitment(&restored).unwrap().as_ref() == Some(&s));
    let mut wrong = s.clone();
    wrong.receipt_digest = "9".repeat(64);
    assert!(
        f.db.record_upload_acceptance(&restored, send.fence(), &wrong, &acceptance(), 2000)
            .is_err()
    );
    let accepted =
        f.db.record_upload_acceptance(&restored, send.fence(), &s, &acceptance(), 2001)
            .unwrap();
    assert_eq!(accepted.upload, UploadState::Accepted);
    assert!(accepted.cancel_requested);
    assert!(!accepted.outcome_unknown);
    assert!(
        f.db.record_upload_acceptance(&restored, send.fence(), &s, &acceptance(), 2002)
            .unwrap()
            .replayed
    );
    let mut changed = acceptance();
    changed.receipt_digest = "6".repeat(64);
    assert!(matches!(
        f.db.record_upload_acceptance(&restored, send.fence(), &s, &changed, 2002),
        Err(Error::Conflict)
    ));
    assert!(f.db.claim_upload(&f.cap, &restored, 2002, 100).is_err());
    assert!(f.db.begin_upload(&f.cap, &claim, 2002).is_err());
    let a = f.db.reserve_upload(&f.cap, &input("one"), 2003).unwrap();
    assert!(a.preparation.is_none());
    f = f.restart();
    assert_eq!(
        f.db.inspect_upload(&restored).unwrap().upload,
        UploadState::Accepted
    );
}

#[test]
fn native_upload_atomic_bounds() {
    let mut f = Fixture::new(true, None);
    f.sql().execute_batch("CREATE TRIGGER upload_fail BEFORE INSERT ON file_uploads BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    assert!(matches!(
        f.db.reserve_upload(&f.cap, &input("fails"), 1010),
        Err(Error::Sqlite(_))
    ));
    assert!(
        f.db.restore_upload(&f.cap, &input("fails"))
            .unwrap()
            .is_none()
    );
    f.sql().execute_batch("DROP TRIGGER upload_fail;").unwrap();
    let (id, s) = f.ready("one");
    let c = f.db.claim_upload(&f.cap, &id, 1020, 1000).unwrap().unwrap();
    f.sql().execute_batch("CREATE TRIGGER upload_fail BEFORE UPDATE ON file_uploads WHEN NEW.upload_state='write_possible' BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    assert!(f.db.begin_upload(&f.cap, &c, 1021).is_err());
    assert_eq!(
        f.db.inspect_upload(&id).unwrap().upload,
        UploadState::Claimed
    );
    f.sql().execute_batch("DROP TRIGGER upload_fail;").unwrap();
    f.db.begin_upload(&f.cap, &c, 1022).unwrap();
    f.sql().execute_batch("CREATE TRIGGER upload_fail BEFORE UPDATE ON file_uploads WHEN NEW.upload_state='accepted' BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    assert!(
        f.db.record_upload_acceptance(&id, c.fence(), &s, &acceptance(), 1023)
            .is_err()
    );
    assert!(f.db.inspect_upload(&id).unwrap().outcome_unknown);
    f.sql().execute_batch("DROP TRIGGER upload_fail;").unwrap();
    for n in 1..MAX_DISPATCH_UPLOADS {
        f.reserve(&format!("n{n}"));
    }
    assert!(matches!(
        f.db.reserve_upload(&f.cap, &input("too_many"), 1024),
        Err(Error::Capacity)
    ));
    assert!(f.reserve("one").preparation.is_none());
    // Fill retained historical rows in ONE fixture transaction; quota counting
    // remains production SQL. Synthetic retired identities exercise only the
    // global counter; authority is separately tested with real dispatches.
    let sql = f.sql();
    sql.execute_batch("PRAGMA foreign_keys=OFF;").unwrap();
    sql.execute_batch("WITH RECURSIVE n(x) AS (SELECT 16 UNION ALL SELECT x+1 FROM n WHERE x<4095) INSERT INTO file_uploads(id,dispatch_id,call_id,request_digest,capability_digest,scope_fingerprint,route,preparation_hash,stage_state,upload_state,created_at,updated_at) SELECT 'history_'||x,'historical_'||x,'old',request_digest,capability_digest,scope_fingerprint,route,preparation_hash,'unbound','pending',0,0 FROM n CROSS JOIN file_uploads WHERE file_uploads.id=(SELECT id FROM file_uploads LIMIT 1);").unwrap();
    assert_eq!(
        sql.query_row("SELECT COUNT(*) FROM file_uploads", [], |r| r
            .get::<_, usize>(0))
            .unwrap(),
        MAX_UPLOADS
    );
    // Free a per-dispatch slot without changing total to isolate the global bound.
    sql.execute(
        "UPDATE file_uploads SET dispatch_id='historical' WHERE call_id='n1'",
        [],
    )
    .unwrap();
    assert!(matches!(
        f.db.reserve_upload(&f.cap, &input("global_full"), 1024),
        Err(Error::Capacity)
    ));
    assert!(f.reserve("one").preparation.is_none());
    let mut bad = stage("bad");
    bad.len = MAX_UPLOAD_BYTES + 1;
    assert!(bad.validate().is_err());
    let mut bad = input("bad");
    bad.request_digest = "A".repeat(64);
    assert!(bad.validate().is_err());
}

#[test]
fn native_upload_schema_migration() {
    let f = Fixture::new(true, None);
    let root = f.root;
    drop(f.db);
    let path = root.path().join("state/domain.sqlite3");
    let sql = rusqlite::Connection::open(&path).unwrap();
    remove_upload_schema(&sql);
    sql.pragma_update(None, "user_version", 18).unwrap();
    drop(sql);
    let db = DomainRepository::open(&root.path().join("state")).unwrap();
    let sql = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        sql.pragma_query_value(None, "user_version", |r| r.get::<_, u64>(0))
            .unwrap(),
        23
    );
    assert_eq!(
        sql.query_row("SELECT COUNT(*) FROM file_uploads", [], |r| r
            .get::<_, u64>(0))
            .unwrap(),
        0
    );
    drop(sql);
    drop(db);
    let db = DomainRepository::open(&root.path().join("state")).unwrap();
    drop(db);
    let sql = rusqlite::Connection::open(&path).unwrap();
    sql.execute_batch("ALTER TABLE file_uploads DROP COLUMN updated_at;")
        .unwrap();
    drop(sql);
    assert!(matches!(
        DomainRepository::open(&root.path().join("state")),
        Err(Error::Schema)
    ));
}

#[path = "file_uploads/settlement.rs"]
mod settlement;
