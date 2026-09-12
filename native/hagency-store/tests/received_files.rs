mod common;
use common::*;
use hagency_core::{
    attachments::*, ingress::MatrixEventObservation, messages::InboundMessage, received_files::*,
    replies::*, tasks::*,
};
use hagency_store::{DomainRepository, EffectOutcome, Error, ReceiveAdmission};
use std::collections::BTreeSet;

#[test]
fn native_receive_write_capability() {
    let mut f = Fixture::new(1);
    let admission = f.reserve(0);
    let write =
        f.db.start_received_file_write(
            &f.cap,
            &admission.reservation.unwrap(),
            &ReceivedFileFacts {
                size: 3,
                sha256: hagency_core::project::hash(b"abc"),
            },
            2011,
        )
        .unwrap();
    assert!(write.matches_capability(&f.cap));
    for field in [
        "dispatch",
        "runner",
        "fence",
        "secret",
        "invalid_secret",
        "invalid_fence",
    ] {
        let mut changed = f.cap.clone();
        match field {
            "dispatch" => changed.dispatch_id = "other".into(),
            "runner" => changed.runner_id = "other".into(),
            "fence" => changed.fence += 1,
            "secret" => {
                changed.secret = if changed.secret == "a".repeat(64) {
                    "b".repeat(64)
                } else {
                    "a".repeat(64)
                }
            }
            "invalid_secret" => changed.secret = "invalid".into(),
            "invalid_fence" => changed.fence = 0,
            _ => unreachable!(),
        }
        assert!(!write.matches_capability(&changed), "{field}");
    }
    assert!(write.matches_capability(&f.cap));
}

struct Fixture {
    root: tempfile::TempDir,
    db: DomainRepository,
    engagement: String,
    cap: RunnerCapability,
}
impl Fixture {
    fn new(count: usize) -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut db = DomainRepository::open(&root.path().join("state")).unwrap();
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
                receipt: "domain host observation only".into(),
            },
        )
        .unwrap();
        db.observe_matrix_transport(
            &MatrixTransportObservation {
                engagement_id: e.id.clone(),
                registration_generation: 1,
                generation: 1,
                sender_mxid: "@worker:example.test".into(),
                device_id: "DEVICE_1".into(),
            },
            1001,
        )
        .unwrap();
        db.observe_matrix_room(
            &MatrixRoomObservation {
                engagement_id: e.id.clone(),
                registration_generation: 1,
                transport_generation: 1,
                room_id: "!project:example.test".into(),
                generation: 1,
                privacy: RoomPrivacy::Group {},
                joined: BTreeSet::from([
                    "@worker:example.test".into(),
                    "@owner:example.test".into(),
                ]),
                invite_only: true,
                encrypted: true,
            },
            1002,
        )
        .unwrap();
        let cap = Self::add(&mut db, &e.id, "s0", "work0", count);
        Self {
            root,
            db,
            engagement: e.id,
            cap,
        }
    }
    fn add(
        db: &mut DomainRepository,
        engagement: &str,
        session: &str,
        workspace: &str,
        count: usize,
    ) -> RunnerCapability {
        db.resolve_verified_matrix_session(
            &SessionBinding {
                id: session.into(),
                engagement_id: engagement.into(),
                room_id: "!project:example.test".into(),
                thread_root: Some(format!("$thread_{session}")),
            },
            1003,
        )
        .unwrap();
        db.create_canonical_task(session, session, "Receive facts", 1004)
            .unwrap();
        db.register_workspace(workspace).unwrap();
        for i in 0..count {
            let event = format!("${session}_{i}");
            db.admit_matrix_attachment(
                &MatrixAttachmentObservation {
                    event: MatrixEventObservation {
                        scope: db.matrix_ingress_scope(session).unwrap(),
                        event: InboundMessage {
                            server_name: "example.test".into(),
                            room_id: "!project:example.test".into(),
                            event_id: event.clone(),
                            sender_mxid: "@owner:example.test".into(),
                            thread_root: Some(format!("$thread_{session}")),
                            body: "untrusted file metadata".into(),
                            kind: "m.file".into(),
                            origin_ts: 1010 + i as u64,
                        },
                        mentions: if i + 1 == count {
                            BTreeSet::from(["@worker:example.test".into()])
                        } else {
                            BTreeSet::new()
                        },
                        encrypted: true,
                    },
                    metadata: AttachmentMetadata {
                        filename: format!("input_{i}.bin"),
                        mime_type: Some("application/octet-stream".into()),
                        declared_size: Some(3),
                    },
                    sdk_identity: "1".repeat(64),
                    manifest_id: hagency_core::project::hash(event.as_bytes()),
                    content_digest: hagency_core::project::hash(
                        format!("cipher_{event}").as_bytes(),
                    ),
                },
                1100 + i as u64,
            )
            .unwrap();
        }
        let selection = db
            .select_receive_inbox(&ReceiveInboxPlan {
                dispatch_id: session.into(),
                session_id: session.into(),
                task_id: session.into(),
                workspace_id: workspace.into(),
            })
            .unwrap();
        assert!(matches!(selection,ReceiveInboxSelection::Selected{count:n,..} if n==count));
        let cap = db
            .claim_dispatch(&format!("runner_{session}"), 2000, 60_000, 120_000, 8)
            .unwrap()
            .unwrap();
        assert_eq!(cap.dispatch_id, session);
        let scope = db.owned_dispatch_scope(&cap, 2001).unwrap();
        db.start_owned_dispatch(&cap, scope.fingerprint(), 2002)
            .unwrap();
        cap
    }
    fn reserve(&mut self, index: usize) -> ReceiveAdmission {
        self.db
            .reserve_received_file(
                &self.cap,
                &format!("$s0_{index}"),
                MAX_RECEIVED_FILE_BYTES,
                2010,
            )
            .unwrap()
    }
    fn sql(&self) -> rusqlite::Connection {
        rusqlite::Connection::open(self.root.path().join("state/domain.sqlite3")).unwrap()
    }
    fn reopen(self) -> Self {
        let Self {
            root,
            db,
            engagement,
            cap,
        } = self;
        drop(db);
        let db = DomainRepository::open(&root.path().join("state")).unwrap();
        Self {
            root,
            db,
            engagement,
            cap,
        }
    }
}
fn facts(bytes: &[u8]) -> ReceivedFileFacts {
    ReceivedFileFacts {
        size: bytes.len() as u64,
        sha256: hagency_core::project::hash(bytes),
    }
}

#[test]
fn native_receive_record_binding() {
    verify_schema_upgrade();
    let mut f = Fixture::new(3);
    let a = f.reserve(0);
    let original = a.reservation.unwrap();
    let ticket = f.db.authorize_attachment(&f.cap, "$s0_0", 2011).unwrap();
    assert!(original.matches_ticket(&ticket));
    assert!(!original.matches_ticket(&f.db.authorize_attachment(&f.cap, "$s0_1", 2011).unwrap()));
    assert_eq!(original.metadata().filename, "input_0.bin");
    assert_eq!(original.scope_fingerprint().len(), 64);
    let replay = f.reserve(0);
    assert!(replay.observation.replayed && replay.reservation.is_none());
    assert!(matches!(
        f.db.reserve_received_file(&f.cap, "$s0_0", 32, 2011),
        Err(Error::Conflict)
    ));
    let mut wrong = f.cap.clone();
    wrong.secret = "f".repeat(64);
    assert!(f.db.inspect_received_file(&wrong, a.identity.id()).is_err());
    // Complete host facts are correlation data; no SDK/file proof is claimed.
    let captured = facts(b"abc");
    let write =
        f.db.start_received_file_write(&f.cap, &original, &captured, 2012)
            .unwrap();
    assert_eq!(write.facts(), &captured);
    assert_eq!(write.identity().id(), a.identity.id());
    assert!(write.matches_ticket(&ticket));
    assert_eq!(write.limit(), MAX_RECEIVED_FILE_BYTES);
    assert_eq!(write.scope_fingerprint(), original.scope_fingerprint());
    assert_eq!(write.metadata(), original.metadata());
    for changed in [facts(b"xyz"), facts(b"abcd")] {
        assert!(matches!(
            f.db.start_received_file_write(&f.cap, &original, &changed, 2013),
            Err(Error::Conflict)
        ));
        assert!(matches!(
            f.db.record_received_file_ready(&f.cap, &a.identity, &changed, 2013),
            Err(Error::Conflict)
        ));
    }
    let second = f.reserve(1);
    let second = second.reservation.unwrap();
    assert!(
        f.db.start_received_file_write(&f.cap, &second, &captured, 122_001)
            .is_err()
    );
    // Coherent mutation of immutable source metadata still conflicts with the
    // original reservation. This is a corrupted-domain association fixture.
    let sql = f.sql();
    let original_meta: String = sql
        .query_row(
            "SELECT metadata FROM matrix_attachments WHERE message_sequence=?1",
            [ticket.source_sequence()],
            |r| r.get(0),
        )
        .unwrap();
    let mut metadata: AttachmentMetadata = serde_json::from_str(&original_meta).unwrap();
    metadata.filename = "changed.bin".into();
    sql.execute(
        "UPDATE matrix_attachments SET metadata=?2 WHERE message_sequence=?1",
        rusqlite::params![
            ticket.source_sequence(),
            serde_json::to_string(&metadata).unwrap()
        ],
    )
    .unwrap();
    assert!(matches!(
        f.db.reserve_received_file(&f.cap, "$s0_0", MAX_RECEIVED_FILE_BYTES, 2015),
        Err(Error::Conflict)
    ));
    sql.execute(
        "UPDATE matrix_attachments SET metadata=?2 WHERE message_sequence=?1",
        rusqlite::params![ticket.source_sequence(), original_meta],
    )
    .unwrap();
    let safe = serde_json::to_string(&a.observation).unwrap();
    for private in [
        "capability",
        "ticket",
        "path",
        "manifest",
        "scope",
        "device",
    ] {
        assert!(!safe.contains(private), "{private}");
    }
}

#[test]
fn native_receive_original_write_once() {
    // Compile-time ambiguity if a write grant gains Clone or Deserialize.
    trait NotClone<A> {
        fn check() {}
    }
    impl<T: ?Sized> NotClone<()> for T {}
    impl<T: Clone> NotClone<u8> for T {}
    let _ = <hagency_store::ReceiveWrite as NotClone<_>>::check;
    trait NotDeserialize<A> {
        fn check() {}
    }
    impl<T: ?Sized> NotDeserialize<()> for T {}
    impl<T: serde::de::DeserializeOwned> NotDeserialize<u8> for T {}
    let _ = <hagency_store::ReceiveWrite as NotDeserialize<_>>::check;
    let mut f = Fixture::new(3);
    let a = f.reserve(0);
    let r = a.reservation.unwrap();
    let bytes = facts(b"abc");
    // Actual committed WritePossible reply is discarded, not a fake phase setter.
    drop(
        f.db.start_received_file_write(&f.cap, &r, &bytes, 2011)
            .unwrap(),
    );
    assert!(matches!(
        f.db.start_received_file_write(&f.cap, &r, &bytes, 2012),
        Err(Error::OutcomeUnknown)
    ));
    assert_eq!(
        f.db.inspect_received_file(&f.cap, a.identity.id())
            .unwrap()
            .state,
        ReceivedFileState::WritePossible
    );
    let ready =
        f.db.record_received_file_ready(&f.cap, &a.identity, &bytes, 2013)
            .unwrap();
    assert_eq!(ready.state, ReceivedFileState::Ready);
    assert!(!ready.replayed);
    assert!(
        f.db.record_received_file_ready(&f.cap, &a.identity, &bytes, 2014)
            .unwrap()
            .replayed
    );
    assert_eq!(
        f.db.record_received_file_negative(&f.cap, &a.identity, ReceiveFailure::Cancelled)
            .unwrap()
            .state,
        ReceivedFileState::Ready
    );
    let pending = f.reserve(1);
    drop(
        f.db.start_received_file_write(&f.cap, pending.reservation.as_ref().unwrap(), &bytes, 2015)
            .unwrap(),
    );
    let reserved = f.reserve(2);
    assert_eq!(
        f.db.canonical_task("s0").unwrap().status,
        TaskState::InProgress
    );
    let sql = f.sql();
    for table in ["file_deliveries", "file_uploads"] {
        assert_eq!(
            sql.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r
                .get::<_, u64>(0))
                .unwrap(),
            0
        );
    }
    drop(sql);
    let f = f.reopen();
    assert_eq!(
        f.db.inspect_received_file(&f.cap, a.identity.id())
            .unwrap()
            .state,
        ReceivedFileState::Ready
    );
    for id in [pending.identity, reserved.identity] {
        assert_eq!(
            f.db.inspect_received_file(&f.cap, id.id()).unwrap().state,
            ReceivedFileState::OutcomeUnknown
        );
    }
    assert!(f.db.visible_attachments(&f.cap, 0, 16, 2016).is_err());
    assert!(
        f.db.inspect_received_file(&f.cap, "receive_ffffffffffffffffffffffffffffffff")
            .is_err()
    );
}

#[test]
fn native_receive_domain_capacity() {
    let mut f = Fixture::new(9);
    let mut first_id = String::new();
    for i in 0..8 {
        let a = f.reserve(i);
        if i == 0 {
            first_id = a.identity.id().into();
        }
        f.db.record_received_file_negative(&f.cap, &a.identity, ReceiveFailure::OutcomeUnknown)
            .unwrap();
    }
    assert!(matches!(
        f.db.reserve_received_file(&f.cap, "$s0_8", MAX_RECEIVED_FILE_BYTES, 2010),
        Err(Error::Capacity)
    ));
    // Every counted row below is an actual current reservation, not synthetic
    // authority or a lowered capacity limit.
    for n in 1..4 {
        let name = format!("s{n}");
        let cap = Fixture::add(&mut f.db, &f.engagement, &name, &format!("work{n}"), 8);
        for i in 0..8 {
            f.db.reserve_received_file(
                &cap,
                &format!("${name}_{i}"),
                MAX_RECEIVED_FILE_BYTES,
                2010,
            )
            .unwrap();
        }
    }
    let cap = Fixture::add(&mut f.db, &f.engagement, "s4", "work4", 1);
    assert!(matches!(
        f.db.reserve_received_file(&cap, "$s4_0", 1, 2010),
        Err(Error::Capacity)
    ));
    let sql = f.sql();
    let counts: (usize, u64) = sql
        .query_row(
            "SELECT COUNT(*),SUM(byte_limit) FROM received_files",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(counts, (32, MAX_RECEIVED_RESERVED_BYTES));
    drop(sql);
    let original = f.db.inspect_received_file(&f.cap, &first_id).unwrap();
    let f = f.reopen();
    assert_eq!(
        f.db.inspect_received_file(&f.cap, &original.id).unwrap(),
        original
    );
    assert_eq!(
        f.sql()
            .query_row("SELECT COUNT(*) FROM received_files", [], |r| r
                .get::<_, usize>(0))
            .unwrap(),
        32
    );
}

#[test]
fn native_receive_inbox_selection() {
    let mut f = Fixture::new(2);
    let original = ReceiveInboxPlan {
        dispatch_id: "s0".into(),
        session_id: "s0".into(),
        task_id: "s0".into(),
        workspace_id: "work0".into(),
    };
    assert_eq!(
        f.db.select_receive_inbox(&original).unwrap(),
        ReceiveInboxSelection::Selected {
            dispatch_id: "s0".into(),
            count: 2,
            replayed: true
        }
    );
    let mut next = original.clone();
    next.dispatch_id = "next".into();
    assert_eq!(
        f.db.select_receive_inbox(&next).unwrap(),
        ReceiveInboxSelection::NoWake
    );
    let mut changed = original.clone();
    changed.workspace_id = "other".into();
    assert!(matches!(
        f.db.select_receive_inbox(&changed),
        Err(Error::Conflict)
    ));
    let mut input = MatrixEventObservation {
        scope: f.db.matrix_ingress_scope("s0").unwrap(),
        event: InboundMessage {
            server_name: "example.test".into(),
            room_id: "!project:example.test".into(),
            event_id: "$later".into(),
            sender_mxid: "@owner:example.test".into(),
            thread_root: Some("$thread_s0".into()),
            body: "later addressed input".into(),
            kind: "m.text".into(),
            origin_ts: 2011,
        },
        mentions: BTreeSet::from(["@worker:example.test".into()]),
        encrypted: true,
    };
    let receipt = f.db.admit_matrix_event(&input, 2012).unwrap();
    assert!(receipt.wake);
    let sql = f.sql();
    // Corrupt an already-admitted copied input without inventing ingress proof.
    // Selection must recheck the actual original provenance before any INSERT.
    let original_config: String = sql
        .query_row(
            "SELECT config FROM session_inputs WHERE session_id='s0' AND message_sequence=?1",
            [receipt.sequence],
            |r| r.get(0),
        )
        .unwrap();
    sql.execute("UPDATE session_inputs SET config=json_set(config,'$.body','substituted') WHERE session_id='s0' AND message_sequence=?1", [receipt.sequence]).unwrap();
    assert!(matches!(
        f.db.select_receive_inbox(&next),
        Err(Error::RunnerAuthority)
    ));
    assert_eq!(
        sql.query_row(
            "SELECT COUNT(*) FROM runner_dispatches WHERE id='next'",
            [],
            |r| r.get::<_, u64>(0)
        )
        .unwrap(),
        0
    );
    sql.execute(
        "UPDATE session_inputs SET config=?2 WHERE session_id='s0' AND message_sequence=?1",
        rusqlite::params![receipt.sequence, original_config],
    )
    .unwrap();
    let before: String = sql
        .query_row(
            "SELECT input FROM runner_dispatches WHERE id='s0'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        f.db.select_receive_inbox(&original).unwrap(),
        ReceiveInboxSelection::Selected {
            dispatch_id: "s0".into(),
            count: 2,
            replayed: true
        }
    );
    assert_eq!(
        sql.query_row(
            "SELECT input FROM runner_dispatches WHERE id='s0'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        before
    );
    // The actual selection transaction must roll back its dispatch/window too
    // when original input assignment fails after those INSERTs.
    sql.execute_batch("CREATE TRIGGER refuse_receive_input BEFORE INSERT ON dispatch_inputs WHEN NEW.dispatch_id='next' BEGIN SELECT RAISE(ABORT,'fixture assignment failure'); END;").unwrap();
    assert!(f.db.select_receive_inbox(&next).is_err());
    assert_eq!(
        sql.query_row(
            "SELECT COUNT(*) FROM runner_dispatches WHERE id='next'",
            [],
            |r| r.get::<_, usize>(0)
        )
        .unwrap(),
        0
    );
    assert_eq!(
        sql.query_row(
            "SELECT COUNT(*) FROM dispatch_attachment_windows WHERE dispatch_id='next'",
            [],
            |r| r.get::<_, usize>(0)
        )
        .unwrap(),
        0
    );
    assert!(
        sql.query_row(
            "SELECT dispatch_id FROM session_inputs WHERE session_id='s0' AND message_sequence=?1",
            [receipt.sequence],
            |r| r.get::<_, Option<String>>(0)
        )
        .unwrap()
        .is_none()
    );
    sql.execute_batch("DROP TRIGGER refuse_receive_input;")
        .unwrap();
    let first = f.db.select_receive_inbox(&next).unwrap();
    assert_eq!(
        first,
        ReceiveInboxSelection::Selected {
            dispatch_id: "next".into(),
            count: 1,
            replayed: false
        }
    );
    let replay = f.db.select_receive_inbox(&next).unwrap();
    assert_eq!(
        replay,
        ReceiveInboxSelection::Selected {
            dispatch_id: "next".into(),
            count: 1,
            replayed: true
        }
    );

    // Predicate-level privacy fixture: raise only the floor, never establish a
    // replacement route/capability. Old original input must not replay or wake.
    sql.execute(
        "UPDATE matrix_session_routes SET ingress_since=3000 WHERE session_id='s0'",
        [],
    )
    .unwrap();
    assert!(matches!(
        f.db.select_receive_inbox(&original),
        Err(Error::RunnerAuthority)
    ));
    input.event.event_id = "$below_floor".into();
    input.event.origin_ts = 2500;
    assert!(f.db.admit_matrix_event(&input, 3001).is_err());
    let mut bounded = next.clone();
    bounded.dispatch_id = "bounded".into();
    assert_eq!(
        f.db.select_receive_inbox(&bounded).unwrap(),
        ReceiveInboxSelection::NoWake
    );

    // Actual accepted long background rows cannot displace the wake trigger or
    // exceed the dispatch JSON bound, including JSON string escaping.
    input.event.origin_ts = 3001;
    input.mentions.clear();
    input.event.body = "x".repeat(32 * 1024);
    for event in ["$large_background_a", "$large_background_b"] {
        input.event.event_id = event.into();
        assert!(!f.db.admit_matrix_event(&input, 3002).unwrap().wake);
    }
    input.event.event_id = "$bounded_trigger".into();
    input.event.body = "use the original file".into();
    input.mentions.insert("@worker:example.test".into());
    let trigger = f.db.admit_matrix_event(&input, 3003).unwrap();
    assert_eq!(
        f.db.select_receive_inbox(&bounded).unwrap(),
        ReceiveInboxSelection::Selected {
            dispatch_id: "bounded".into(),
            count: 2,
            replayed: false,
        }
    );
    let frozen: String = sql
        .query_row(
            "SELECT input FROM runner_dispatches WHERE id='bounded'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(frozen.len() <= 64 * 1024);
    let parsed: serde_json::Value = serde_json::from_str(&frozen).unwrap();
    let items = parsed["payload"]["inbox"].as_array().unwrap();
    assert_eq!(
        items.last().unwrap()["message"]["sequence"],
        trigger.sequence
    );
    assert_eq!(
        items.last().unwrap()["message"]["event_id"],
        "$bounded_trigger"
    );

    input.event.event_id = "$oversized_escaped_trigger".into();
    input.event.body = "\"".repeat(32 * 1024);
    let oversized = f.db.admit_matrix_event(&input, 3004).unwrap();
    bounded.dispatch_id = "oversized".into();
    assert!(matches!(
        f.db.select_receive_inbox(&bounded),
        Err(Error::Capacity)
    ));
    assert_eq!(
        sql.query_row(
            "SELECT COUNT(*) FROM runner_dispatches WHERE id='oversized'",
            [],
            |r| r.get::<_, u64>(0)
        )
        .unwrap(),
        0
    );
    assert!(
        sql.query_row(
            "SELECT dispatch_id FROM session_inputs WHERE session_id='s0' AND message_sequence=?1",
            [oversized.sequence],
            |r| r.get::<_, Option<String>>(0)
        )
        .unwrap()
        .is_none()
    );
}

fn verify_schema_upgrade() {
    let f = Fixture::new(1);
    let sql = f.sql();
    let original: (String, String, String) = sql
        .query_row(
            "SELECT metadata,manifest_id,content_digest FROM matrix_attachments",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    let Fixture { root, db, .. } = f;
    drop(db);
    sql.execute_batch(
        "DROP TABLE approval_responses; DROP TABLE received_files; PRAGMA user_version=20;",
    )
    .unwrap();
    drop(sql);
    let db = DomainRepository::open(&root.path().join("state")).unwrap();
    let sql = rusqlite::Connection::open(root.path().join("state/domain.sqlite3")).unwrap();
    assert_eq!(
        sql.query_row("PRAGMA user_version", [], |r| r.get::<_, u64>(0))
            .unwrap(),
        23
    );
    assert_eq!(
        sql.query_row(
            "SELECT metadata,manifest_id,content_digest FROM matrix_attachments",
            [],
            |r| Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?
            ))
        )
        .unwrap(),
        original
    );
    assert_eq!(
        sql.query_row("SELECT COUNT(*) FROM received_files", [], |r| r
            .get::<_, usize>(0))
            .unwrap(),
        0
    );
    drop(db);
    sql.execute_batch("ALTER TABLE received_files DROP COLUMN binding_digest;")
        .unwrap();
    drop(sql);
    assert!(matches!(
        DomainRepository::open(&root.path().join("state")),
        Err(Error::Schema)
    ));
}
