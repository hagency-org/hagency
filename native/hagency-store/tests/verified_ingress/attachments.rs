use super::*;
use hagency_core::attachments::*;

fn file(f: &Fixture, session: &str, id: &str, at: u64) -> MatrixAttachmentObservation {
    let mut event = f.event(session, id, None, &["@a:example.test"], at);
    event.event.kind = "m.file".into();
    event.event.body = "报告.txt".into();
    event.encrypted = true;
    MatrixAttachmentObservation {
        event,
        metadata: AttachmentMetadata {
            filename: "报告.txt".into(),
            mime_type: Some("text/plain".into()),
            declared_size: Some(12),
        },
        sdk_identity: "1".repeat(64),
        manifest_id: hagency_core::canonical::digest(&json!(id)).unwrap(),
        content_digest: hagency_core::canonical::digest(&json!([id, "ciphertext"])).unwrap(),
    }
}
fn read_dispatch(id: &str) -> DispatchInput {
    DispatchInput {
        id: id.into(),
        session_id: "a".into(),
        task_id: None,
        resources: vec![],
        payload: json!({"test":"scoped host attachment read"}),
    }
}
fn direct_start(f: &mut Fixture, id: &str, at: u64) -> RunnerCapability {
    let first = f.db.inbox("a", 0, 100, None).unwrap()[0].message.sequence;
    f.db.enqueue_inbox_dispatch(&read_dispatch(id), &[first])
        .unwrap();
    let cap =
        f.db.claim_dispatch("files", at, 60000, 120000, 8)
            .unwrap()
            .unwrap();
    assert_eq!(cap.dispatch_id, id);
    f.db.start_dispatch(&cap, at + 1).unwrap();
    cap
}
fn count(f: &Fixture, table: &str) -> u64 {
    assert!(
        [
            "matrix_attachments",
            "admitted_messages",
            "session_attachment_visibility",
            "dispatch_attachment_windows"
        ]
        .contains(&table)
    );
    f.sql()
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

#[test]
fn native_attachment_admission() {
    let mut f = Fixture::new(true);
    let input = file(&f, "a", "file", 1004);
    let r = f.db.admit_matrix_attachment(&input, 1005).unwrap();
    assert!(r.wake && r.projected);
    assert!(
        !f.db
            .admit_matrix_attachment(&input, 1006)
            .unwrap()
            .projected
    );
    assert_eq!(
        f.db.matrix_attachment_receipt(&input)
            .unwrap()
            .unwrap()
            .sequence,
        r.sequence
    );
    assert_eq!(count(&f, "matrix_attachments"), 1);
    assert_eq!(count(&f, "session_attachment_visibility"), 1);
    let encoded = serde_json::to_string(&f.db.inbox("a", 0, 100, None).unwrap()).unwrap();
    for hidden in [
        &input.sdk_identity,
        &input.manifest_id,
        &input.content_digest,
    ] {
        assert!(!encoded.contains(hidden));
    }
    assert!(!encoded.contains("mxc://"));
    for field in ["metadata", "manifest", "sdk", "content"] {
        let mut changed = input.clone();
        match field {
            "metadata" => changed.metadata.filename = "changed.txt".into(),
            "manifest" => changed.manifest_id = "2".repeat(64),
            "sdk" => changed.sdk_identity = "3".repeat(64),
            _ => changed.content_digest = "4".repeat(64),
        }
        assert!(matches!(
            f.db.admit_matrix_attachment(&changed, 1007),
            Err(Error::Conflict)
        ));
        assert!(matches!(
            f.db.matrix_attachment_receipt(&changed),
            Err(Error::Conflict)
        ));
    }
    let legacy = file(&f, "a", "legacy-file", 1007);
    f.db.admit_matrix_event(&legacy.event, 1008).unwrap();
    assert!(matches!(
        f.db.admit_matrix_attachment(&legacy, 1009),
        Err(Error::Conflict)
    ));
    assert!(matches!(
        f.db.admit_matrix_event(&input.event, 1009),
        Err(Error::Conflict)
    ));
    let before = count(&f, "admitted_messages");
    for bad in ["../escape", "", "a/b", "a\\b", "x\n.txt"] {
        let mut bad_input = file(&f, "a", "bad-name", 1010);
        bad_input.metadata.filename = bad.into();
        assert!(f.db.admit_matrix_attachment(&bad_input, 1011).is_err());
    }
    let mut bad_input = file(&f, "a", "unverified", 1010);
    bad_input.event.encrypted = false;
    assert!(f.db.admit_matrix_attachment(&bad_input, 1011).is_err());
    assert_eq!(count(&f, "admitted_messages"), before);
    // Actual writer INSERT failure must roll back the source event and projections.
    f.sql().execute_batch("CREATE TRIGGER reject_attachment BEFORE INSERT ON matrix_attachments BEGIN SELECT RAISE(ABORT,'fixture write refusal'); END;").unwrap();
    let failed = file(&f, "a", "write-failure", 1010);
    assert!(f.db.admit_matrix_attachment(&failed, 1011).is_err());
    assert_eq!(count(&f, "admitted_messages"), before);
    f.sql()
        .execute_batch("DROP TRIGGER reject_attachment;")
        .unwrap();
    // Populate the real bounded table to its cap; synthetic old rows are count
    // fixtures only, not claims that 4096 authenticated events were observed.
    f.sql().execute_batch("BEGIN; WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<4095) INSERT INTO admitted_messages(source_key,digest,config) SELECT 'fixture_'||x,'fixture_digest','{}' FROM n;
        INSERT INTO matrix_ingress_events(engagement_id,source_key,message_sequence,scope_digest,digest,config,source_session_id) SELECT e.engagement_id,m.source_key,m.sequence,e.scope_digest,'fixture_digest','{}','a' FROM admitted_messages m CROSS JOIN (SELECT * FROM matrix_ingress_events WHERE source_session_id='a' LIMIT 1) e WHERE m.source_key LIKE 'fixture_%';
        INSERT INTO matrix_attachments SELECT e.engagement_id,e.source_key,e.message_sequence,'a','fixture_digest','fixture_content','{}','fixture_sdk','fixture_manifest' FROM matrix_ingress_events e WHERE e.source_key LIKE 'fixture_%'; COMMIT;").unwrap();
    assert_eq!(count(&f, "matrix_attachments"), 4096);
    let capacity = file(&f, "a", "capacity", 1012);
    assert!(matches!(
        f.db.admit_matrix_attachment(&capacity, 1013),
        Err(Error::Capacity)
    ));
    assert_eq!(count(&f, "admitted_messages"), before + 4095);
    // Historical exact replay still works at the bound.
    assert!(f.db.matrix_attachment_receipt(&input).unwrap().is_some());
}

#[test]
fn native_attachment_frozen_visibility() {
    let mut f = Fixture::new(true);
    let earlier = file(&f, "a", "early", 1004);
    f.db.admit_matrix_attachment(&earlier, 1005).unwrap();
    let queued = file(&f, "a", "queued-after-trigger", 1005);
    f.db.admit_matrix_attachment(&queued, 1006).unwrap();
    let cap = direct_start(&mut f, "read", 1006);
    let ticket = f.db.authorize_attachment(&cap, "$early", 1008).unwrap();
    assert!(
        f.db.authorize_attachment(&cap, "$queued-after-trigger", 1008)
            .is_err()
    );
    assert_eq!(ticket.metadata().filename, "报告.txt");
    assert_eq!(ticket.manifest_id(), earlier.manifest_id);
    f.db.revalidate_attachment(&cap, &ticket, 1009).unwrap();
    let later = file(&f, "a", "later", 1009);
    f.db.admit_matrix_attachment(&later, 1010).unwrap();
    assert!(f.db.authorize_attachment(&cap, "$later", 1011).is_err());
    // Re-enqueue identical content cannot advance the original cutoffs.
    f.db.enqueue_inbox_dispatch(&read_dispatch("read"), &[ticket.source_sequence()])
        .unwrap();
    assert!(f.db.authorize_attachment(&cap, "$later", 1012).is_err());
    let sql = f.sql();
    let cutoff: u64 = sql
        .query_row(
            "SELECT projection_cutoff FROM dispatch_attachment_windows WHERE dispatch_id='read'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    // Inject a late session projection of an already old source, independently
    // of its source sequence. Only the projection cutoff excludes this record.
    sql.execute(
        "DELETE FROM session_attachment_visibility WHERE message_sequence=?1",
        [ticket.source_sequence()],
    )
    .unwrap();
    sql.execute_batch("INSERT INTO session_attachment_visibility(session_id,engagement_id,source_key,message_sequence) SELECT 'a',engagement_id,source_key,message_sequence FROM matrix_attachments WHERE source_session_id='a' AND message_sequence=1;").unwrap();
    let new:u64=sql.query_row("SELECT projection_sequence FROM session_attachment_visibility WHERE message_sequence=1",[],|r|r.get(0)).unwrap();
    assert!(new > cutoff);
    assert!(f.db.authorize_attachment(&cap, "$early", 1013).is_err());
    assert!(f.db.revalidate_attachment(&cap, &ticket, 1013).is_err());
}

#[test]
fn native_attachment_followup_visibility() {
    let mut f = Fixture::new(false);
    let input = file(&f, "a", "request-file", 1004);
    let r = f.db.admit_matrix_attachment(&input, 1005).unwrap();
    let task =
        f.db.create_verified_task_intent(&f.intent("a", "task", r.sequence), 1006)
            .unwrap();
    assert_ne!(task.session_id, "a");
    f.activate(1007);
    let cap = f.start("first", &task, &[r.sequence], 1010);
    let ticket =
        f.db.authorize_attachment(&cap, "$request-file", 1012)
            .unwrap();
    assert_eq!(ticket.source_scope().session_id, "a");
    // Same room and Agent, but no task-input provenance into this child session.
    let unrelated = file(&f, "a", "unrelated", 1012);
    f.db.admit_matrix_attachment(&unrelated, 1013).unwrap();
    f.done(&cap, &task, 1014);
    f.db.complete_dispatch(&cap, &json!({"done":true}), 1015)
        .unwrap();
    let followup = f.event(
        &task.session_id,
        "followup",
        Some("$request-file"),
        &["@a:example.test"],
        1016,
    );
    let follow = f.db.admit_matrix_event(&followup, 1017).unwrap();
    let cap2 = f.start("second", &task, &[follow.sequence], 1018);
    assert!(
        f.db.authorize_attachment(&cap2, "$request-file", 1020)
            .is_ok()
    );
    assert!(
        f.db.authorize_attachment(&cap2, "$unrelated", 1020)
            .is_err()
    );
    assert!(f.db.revalidate_attachment(&cap2, &ticket, 1020).is_err());
    // The same original ciphertext can be observed by another authenticated
    // Agent SDK, without exposing that SDK's private manifest to the first.
    let mut other = input.clone();
    other.event.scope = f.db.matrix_ingress_scope("b").unwrap();
    other.sdk_identity = "b".repeat(64);
    other.manifest_id = "c".repeat(64);
    let mut contradictory = other.clone();
    contradictory.content_digest = "e".repeat(64);
    assert!(matches!(
        f.db.admit_matrix_attachment(&contradictory, 1021),
        Err(Error::Conflict)
    ));
    assert!(f.db.admit_matrix_attachment(&other, 1021).is_ok());
    other.content_digest = "d".repeat(64);
    assert!(matches!(
        f.db.admit_matrix_attachment(&other, 1022),
        Err(Error::Conflict)
    ));
}

#[test]
fn native_attachment_ticket_revalidation() {
    for case in ["expiry", "promotion", "revoke", "restart"] {
        let mut f = Fixture::new(true);
        let input = file(&f, "a", "file", 1004);
        f.db.admit_matrix_attachment(&input, 1005).unwrap();
        let cap = direct_start(&mut f, "read", 1006);
        let ticket = f.db.authorize_attachment(&cap, "$file", 1008).unwrap();
        match case {
            "expiry" => {
                assert!(f.db.revalidate_attachment(&cap, &ticket, 200000).is_err());
            }
            "promotion" => {
                let mut room = f.room.clone();
                room.generation = 2;
                room.privacy = RoomPrivacy::Group {};
                room.joined.insert("@other:example.test".into());
                f.db.observe_matrix_room(&room, 1009).unwrap();
                assert!(f.db.revalidate_attachment(&cap, &ticket, 1010).is_err());
            }
            "revoke" => {
                f.db.revoke("revoke_file_access", &f.agents[0]).unwrap();
                assert!(f.db.revalidate_attachment(&cap, &ticket, 1011).is_err());
            }
            _ => {
                drop(f.db);
                f.db = DomainRepository::open(&f.root.path().join("state")).unwrap();
                assert!(f.db.revalidate_attachment(&cap, &ticket, 1011).is_err());
                assert_eq!(count(&f, "matrix_attachments"), 1);
                assert_eq!(count(&f, "dispatch_attachment_windows"), 1);
            }
        }
        // Historical acknowledgement remains possible; it does not re-admit or
        // restore the current runner after any negative authority observation.
        assert!(f.db.matrix_attachment_receipt(&input).unwrap().is_some());
    }
}

#[test]
fn native_attachment_schema_migration() {
    let mut f = Fixture::new(true);
    let event = f.event("a", "old-input", None, &[], 1004);
    f.db.admit_matrix_event(&event, 1004).unwrap();
    let cap = direct_start(&mut f, "old", 1004);
    f.db.complete_dispatch(&cap, &json!({}), 1006).unwrap();
    let path = f.root.path().join("state");
    drop(f.db);
    let sql = rusqlite::Connection::open(path.join("domain.sqlite3")).unwrap();
    remove_attachment_schema(&sql);
    sql.pragma_update(None, "user_version", 17).unwrap();
    drop(sql);
    for _ in 0..2 {
        let db = DomainRepository::open(&path).unwrap();
        drop(db);
    }
    let sql = rusqlite::Connection::open(path.join("domain.sqlite3")).unwrap();
    assert_eq!(
        sql.pragma_query_value(None, "user_version", |r| r.get::<_, u64>(0))
            .unwrap(),
        23
    );
    for table in [
        "matrix_attachments",
        "session_attachment_visibility",
        "dispatch_attachment_windows",
    ] {
        assert_eq!(
            sql.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r
                .get::<_, u64>(0))
                .unwrap(),
            0
        );
    }
    sql.execute_batch("ALTER TABLE dispatch_attachment_windows RENAME COLUMN projection_cutoff TO missing_cutoff;").unwrap();
    drop(sql);
    assert!(matches!(DomainRepository::open(&path), Err(Error::Schema)));
}

#[test]
fn native_receive_visible_context() {
    let mut f = Fixture::new(true);
    for i in 0..20 {
        f.db.admit_matrix_attachment(&file(&f, "a", &format!("page_{i}"), 1004 + i), 1030 + i)
            .unwrap();
    }
    let items = f.db.inbox("a", 0, 100, None).unwrap();
    // Real selected trigger is last; prior attachments are visible context.
    f.db.enqueue_inbox_dispatch(
        &read_dispatch("page"),
        &[items.last().unwrap().message.sequence],
    )
    .unwrap();
    let cap =
        f.db.claim_dispatch("reader", 1100, 60_000, 120_000, 8)
            .unwrap()
            .unwrap();
    f.db.start_dispatch(&cap, 1101).unwrap();
    let first = f.db.visible_attachments(&cap, 0, 16, 1102).unwrap();
    assert_eq!(first.items.len(), 16);
    assert!(first.next.is_some());
    let second =
        f.db.visible_attachments(&cap, first.next.unwrap(), 16, 1103)
            .unwrap();
    assert_eq!(second.items.len(), 4);
    assert!(second.next.is_none());
    assert_eq!(second.items.last().unwrap().event_id, "$page_19");
    for item in first.items.iter().chain(&second.items) {
        let t =
            f.db.authorize_attachment(&cap, &item.event_id, 1103)
                .unwrap();
        assert_eq!(t.source_sequence(), item.sequence);
        assert_eq!(t.metadata(), &item.metadata);
    }
    f.db.admit_matrix_attachment(&file(&f, "a", "after_page", 1103), 1104)
        .unwrap();
    assert_eq!(
        f.db.visible_attachments(&cap, first.next.unwrap(), 16, 1105)
            .unwrap(),
        second
    );
    let mut wrong = cap.clone();
    wrong.secret = "f".repeat(64);
    assert!(f.db.visible_attachments(&wrong, 0, 16, 1106).is_err());
    for limit in [0, 17, usize::MAX] {
        assert!(f.db.visible_attachments(&cap, 0, limit, 1106).is_err());
    }
    let encoded = serde_json::to_string(&first).unwrap();
    for key in [
        "manifest",
        "sdk_identity",
        "content_digest",
        "path",
        "capability",
    ] {
        assert!(!encoded.contains(key));
    }
    // This fixture raises only the stored privacy floor to test both read
    // predicates. It is not transport observation or crypto proof.
    let sql = f.sql();
    sql.execute(
        "UPDATE matrix_session_routes SET ingress_since=1020 WHERE session_id='a'",
        [],
    )
    .unwrap();
    let page = f.db.visible_attachments(&cap, 0, 16, 1107).unwrap();
    assert_eq!(page.items.len(), 4);
    assert!(f.db.authorize_attachment(&cap, "$page_0", 1107).is_err());
    f.db.revoke("retire", &f.agents[0]).unwrap();
    assert!(f.db.visible_attachments(&cap, 0, 16, 1108).is_err());
}
