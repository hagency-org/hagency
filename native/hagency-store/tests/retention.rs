//! The admitted-corpus retention slice (ADR-125): the pin predicate, the
//! archive move, the bounded window and the migration, each pinned by its
//! own scenario. Every SQL statement binds its parameters; no assertion
//! relies on row order beyond what an ORDER BY gives.
mod common;
use common::*;
use hagency_core::{ingress::*, messages::*, replies::*, task_intents::TaskDefinition, tasks::*};
use hagency_store::{DomainRepository, Error};
use rusqlite::{Connection, params};
use serde_json::json;
use std::collections::BTreeSet;

struct Fixture {
    root: tempfile::TempDir,
    db: DomainRepository,
    engagement: String,
    agent: String,
}

impl Fixture {
    /// One group-room agent with a live verified route, in the
    /// verified-ingress harness shape. A fresh private database per test, so
    /// agent and runner names never collide across tests.
    fn new(agent: &str) -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut db = DomainRepository::open(&root.path().join("state")).unwrap();
        db.register(&registration()).unwrap();
        let pool = resource("pool", "seat", 1000);
        db.put_resource(&pool).unwrap();
        let name = format!("corpus_{agent}");
        let proof = proof(&request(agent, &name, &pool, 100));
        let engagement = db.admit(&proof, 1000).unwrap();
        db.approve(&format!("approve_{agent}"), &proof, 1000)
            .unwrap();
        let effect = db.claim_effect().unwrap().unwrap();
        db.observe_effect(
            &effect.id,
            effect.fence,
            &hagency_store::EffectOutcome::Applied {
                receipt: "fixture account".into(),
            },
        )
        .unwrap();
        let mxid = format!("@{agent}:example.test");
        db.observe_matrix_transport(
            &MatrixTransportObservation {
                engagement_id: engagement.id.clone(),
                registration_generation: 1,
                generation: 1,
                sender_mxid: mxid.clone(),
                device_id: format!("DEVICE_{agent}"),
            },
            1001,
        )
        .unwrap();
        db.observe_matrix_room(
            &MatrixRoomObservation {
                engagement_id: engagement.id.clone(),
                registration_generation: 1,
                transport_generation: 1,
                room_id: "!project:example.test".into(),
                generation: 1,
                privacy: RoomPrivacy::Group {},
                joined: BTreeSet::from_iter([
                    "@owner:example.test".into(),
                    mxid.clone(),
                    registration().representative_mxid,
                    registration().approval_bot_mxid,
                ]),
                invite_only: true,
                encrypted: false,
            },
            1002,
        )
        .unwrap();
        db.resolve_verified_matrix_session(
            &SessionBinding {
                id: format!("session_{agent}"),
                engagement_id: engagement.id.clone(),
                room_id: "!project:example.test".into(),
                thread_root: None,
            },
            1003,
        )
        .unwrap();
        db.register_workspace("work").unwrap();
        Self {
            root,
            db,
            engagement: engagement.id,
            agent: agent.to_owned(),
        }
    }
    /// One admitted wake event through the real verified-ingress path.
    fn admit(&mut self, id: &str, at: u64) -> u64 {
        let session = format!("session_{}", self.agent);
        let event = MatrixEventObservation {
            scope: self.db.matrix_ingress_scope(&session).unwrap(),
            event: InboundMessage {
                server_name: "example.test".into(),
                room_id: "!project:example.test".into(),
                event_id: format!("${id}"),
                sender_mxid: "@owner:example.test".into(),
                thread_root: None,
                body: format!("Message {id}"),
                kind: "m.text".into(),
                origin_ts: at,
            },
            mentions: std::collections::BTreeSet::from_iter([format!(
                "@{}:example.test",
                self.agent
            )]),
            encrypted: false,
        };
        self.db.admit_matrix_event(&event, at).unwrap().sequence
    }
    /// One admitted threaded REPLY through the same real path, its thread
    /// root bound to the given event id (read 6's caller shape). The fixture
    /// registers a thread-bound verified session beside the room session
    /// (the `native_verified_ingress_task_activation_existing_thread`
    /// shape): the admit path compares the event's `thread_root` against
    /// the session route's, so a threaded event under the room session's
    /// `thread_root: None` route refuses with `RunnerAuthority` — exactly
    /// where the two R6 fixtures failed. Returns the sequence AND the
    /// thread session id: the reply's `session_inputs` row lives under the
    /// thread session, so the task request must be scoped there too. The
    /// thread route's `scope_digest` still equals the room route's (the
    /// digest covers transport/room identity, never the thread binding),
    /// so read 6's `scope_digest=?3` matching is unchanged.
    fn admit_threaded(&mut self, id: &str, at: u64, thread_root: &str) -> (u64, String) {
        // The binding id is a VALID OPAQUE IDENTIFIER derived from the
        // thread root (the raw event id carries '$', which the identifier
        // rules reject — the macOS oracle's Err at this call): a short hex
        // fold. The `thread_root` itself still rides in the SessionBinding,
        // which is what the route check compares against.
        let thread_id = thread_root.bytes().fold(0u64, |acc, byte| {
            acc.wrapping_mul(31).wrapping_add(byte as u64)
        });
        let thread_session = format!("session_{}_t{:x}", self.agent, thread_id);
        self.db
            .resolve_verified_matrix_session(
                &SessionBinding {
                    id: thread_session.clone(),
                    engagement_id: self.engagement.clone(),
                    room_id: "!project:example.test".into(),
                    thread_root: Some(thread_root.into()),
                },
                at.saturating_sub(1),
            )
            .unwrap();
        let event = MatrixEventObservation {
            scope: self.db.matrix_ingress_scope(&thread_session).unwrap(),
            event: InboundMessage {
                server_name: "example.test".into(),
                room_id: "!project:example.test".into(),
                event_id: format!("${id}"),
                sender_mxid: "@owner:example.test".into(),
                thread_root: Some(thread_root.into()),
                body: format!("Message {id}"),
                kind: "m.text".into(),
                origin_ts: at,
            },
            mentions: std::collections::BTreeSet::from_iter([format!(
                "@{}:example.test",
                self.agent
            )]),
            encrypted: false,
        };
        let sequence = self.db.admit_matrix_event(&event, at).unwrap().sequence;
        (sequence, thread_session)
    }
    fn observation(&self, id: &str, at: u64) -> MatrixEventObservation {
        let scope = self
            .db
            .matrix_ingress_scope(&format!("session_{}", self.agent))
            .expect("scope resolves");
        MatrixEventObservation {
            scope,
            event: InboundMessage {
                server_name: "example.test".into(),
                room_id: "!project:example.test".into(),
                event_id: format!("${id}"),
                sender_mxid: "@owner:example.test".into(),
                thread_root: None,
                body: format!("Message {id}"),
                kind: "m.text".into(),
                origin_ts: at,
            },
            mentions: std::collections::BTreeSet::from_iter([format!(
                "@{}:example.test",
                self.agent
            )]),
            encrypted: false,
        }
    }
    fn sql(&self) -> Connection {
        Connection::open(self.root.path().join("state/domain.sqlite3")).unwrap()
    }
    fn count(&self, table: &str) -> u64 {
        self.sql()
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    }
}

/// Mark one admitted message's session input processed (the P2/P3' release
/// witness), bound parameters only.
fn processed(f: &Fixture, sequence: u64, at: u64) {
    f.sql()
        .execute(
            "UPDATE session_inputs SET processed_at=?2 WHERE message_sequence=?1",
            params![sequence, at],
        )
        .unwrap();
}

/// A canonical task row with the given terminal status, plus (optionally) a
/// task_inputs row and a task_intents root row for the message.
fn task(f: &Fixture, id: &str, status: &str, sequence: u64, intent: bool) {
    let config = json!({
        "id": id, "session_id": format!("session_{}", f.agent), "title": "T",
        "status": status, "execution_epoch": 1, "created_at": 1, "updated_at": 1,
    })
    .to_string();
    let sql = f.sql();
    sql.execute(
        "INSERT INTO canonical_tasks(id,session_id,config) VALUES(?1,?2,?3)",
        params![id, format!("session_{}", f.agent), config],
    )
    .unwrap();
    let encoded: String = sql
        .query_row(
            "SELECT config FROM admitted_messages WHERE sequence=?1",
            [sequence],
            |r| r.get(0),
        )
        .unwrap();
    sql.execute(
        "INSERT INTO task_inputs(task_id,message_sequence,config,wake) VALUES(?1,?2,?3,1)",
        params![id, sequence, encoded],
    )
    .unwrap();
    if intent {
        sql.execute(
            "INSERT INTO task_intents(task_id,request_scope,request_key,digest,session_id,root_sequence,state) VALUES(?1,?2,?3,?4,?5,?6,'pending')",
            params![id, "scope", format!("key_{id}"), "digest", format!("session_{}", f.agent), sequence],
        )
        .unwrap();
    }
}

#[tokio::test]
async fn native_retained_corpus_prunes_below_ceiling_only_when_no_live_reference() {
    let mut f = Fixture::new("prune");
    let mut sequences = Vec::new();
    for i in 0..103 {
        sequences.push(f.admit(&format!("plain{i}"), 2000 + i));
    }
    // The oldest three are processed and unreferenced; everything newer
    // carries an unprocessed session input (P2) and sits inside the P1
    // window of the effective ceiling. The corpus is sized above the floor
    // because the store clamps every ceiling up to it (the `Math.max(100,…)`
    // guard) — a smaller corpus can never be over the ceiling.
    for sequence in &sequences[..3] {
        processed(&f, *sequence, 3000);
    }
    let outcome = f.db.sweep_admitted_corpus(4000, 100, 512).unwrap();
    // The F3 measurement, logged for the report: the tick's own wall-clock.
    println!(
        "[corpus] sweep tick elapsed_ms={} pruned={} remaining={}",
        outcome.elapsed_ms, outcome.pruned, outcome.remaining
    );
    assert_eq!(outcome.pruned, 3);
    assert_eq!(outcome.archived, 3);
    assert_eq!(outcome.remaining, 0);
    assert_eq!(f.count("admitted_messages"), 100);
    assert_eq!(f.count("retained_message_archive"), 3);
    assert_eq!(f.count("matrix_ingress_events"), 100);
    // The pinned survivors keep their children.
    let pinned: u64 = f
        .sql()
        .query_row(
            "SELECT COUNT(*) FROM session_inputs WHERE processed_at IS NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(pinned, 100);
    // The receipt: one row, the messages phase, the over-ceiling figure.
    let (phase, pruned, remaining): (String, u64, u64) = f
        .sql()
        .query_row(
            "SELECT phase,pruned,remaining FROM retention_prune_receipts ORDER BY sequence DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!((phase.as_str(), pruned, remaining), ("messages", 3, 0));
}

#[tokio::test]
async fn native_retained_corpus_pending_pin_exceeds_ceiling() {
    let mut f = Fixture::new("pending");
    for i in 0..103 {
        f.admit(&format!("held{i}"), 2000 + i);
    }
    // Every row is pinned by an unprocessed session input (P2). The corpus
    // is above the floor because the store clamps every ceiling up to it.
    let outcome = f.db.sweep_admitted_corpus(4000, 100, 512).unwrap();
    assert_eq!(outcome.pruned, 0);
    assert_eq!(outcome.remaining, 3);
    assert_eq!(f.count("admitted_messages"), 103);
    assert_eq!(f.count("retained_message_archive"), 0);
    // The over-ceiling figure is REPORTED, never a refusal of admission.
    let status = f.db.retention_status(100).unwrap();
    assert_eq!(
        (status.corpus_rows, status.ceiling, status.over_by),
        (103, 100, 3)
    );
    // A further admission still lands: the bound never refuses work.
    let extra = f.admit("held103", 5000);
    assert!(extra > 0);
}

#[tokio::test]
async fn native_retained_corpus_processed_dispatch_does_not_pin() {
    let mut f = Fixture::new("dispatch");
    let old = f.admit("claimed", 2000);
    for i in 0..100 {
        f.admit(&format!("recent{i}"), 2001 + i);
    }
    // The corpus sits above the floor because the store clamps every
    // ceiling up to it; the completed-dispatch row is the one candidate
    // below the recency window of the effective ceiling.
    let input = DispatchInput {
        id: "dispatch_done".into(),
        session_id: format!("session_{}", f.agent),
        task_id: None,
        resources: vec![ResourceLease {
            id: "work".into(),
            exclusive: true,
        }],
        payload: json!({"instruction":"Handle the admitted input"}),
    };
    f.db.enqueue_inbox_dispatch(&input, &[old]).unwrap();
    let cap =
        f.db.claim_dispatch("runner_dispatch_done", 2002, 60_000, 120_000, 8)
            .unwrap()
            .unwrap();
    f.db.start_dispatch(&cap, 2003).unwrap();
    f.db.complete_dispatch(&cap, &json!({"ok":true}), 2004)
        .unwrap();
    // The stale non-null dispatch_id and the processed witness are exactly
    // the P3' release: a completed dispatch does not pin.
    let (dispatch_id, processed): (Option<String>, Option<u64>) = f
        .sql()
        .query_row(
            "SELECT dispatch_id,processed_at FROM session_inputs WHERE message_sequence=?1",
            [old],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert!(dispatch_id.is_some() && processed.is_some());
    let outcome = f.db.sweep_admitted_corpus(3000, 100, 512).unwrap();
    assert_eq!(outcome.pruned, 1);
    assert_eq!(f.count("admitted_messages"), 100);
    assert_eq!(
        f.count("session_inputs"),
        100,
        "the pruned row's child is removed; the survivors keep their own"
    );
    assert_eq!(f.count("dispatch_inputs"), 0);
}

#[tokio::test]
async fn native_retained_corpus_closed_task_input_does_not_pin() {
    let mut f = Fixture::new("taskdone");
    let old = f.admit("attached_to_task", 2000);
    for i in 0..100 {
        f.admit(&format!("recent{i}"), 2001 + i);
    }
    // The corpus sits above the floor because the store clamps every
    // ceiling up to it; the done-task row is the one candidate below the
    // recency window of the effective ceiling. The task lifecycle gate is
    // the CANONICAL task's own terminal state (A1): `task_intents.state=
    // 'closed'` has no production writer, so the release witness is config
    // status 'done'. The session input is marked processed first so ONLY
    // the task clause is under test.
    processed(&f, old, 2500);
    task(&f, "task_done", "done", old, true);
    let outcome = f.db.sweep_admitted_corpus(3000, 100, 512).unwrap();
    assert_eq!(outcome.pruned, 1, "a done task's input is a candidate");
    assert_eq!(f.count("task_inputs"), 0);
    assert_eq!(f.count("task_intents"), 0);
    // The mirror: an OPEN task pins the same shape.
    let mut f2 = Fixture::new("taskopen");
    let old2 = f2.admit("attached_open", 2000);
    for i in 0..100 {
        f2.admit(&format!("recent{i}"), 2001 + i);
    }
    processed(&f2, old2, 2500);
    task(&f2, "task_open", "in_progress", old2, true);
    let outcome2 = f2.db.sweep_admitted_corpus(3000, 100, 512).unwrap();
    assert_eq!(outcome2.pruned, 0, "an open task's root and input pin");
    assert_eq!(f2.count("task_inputs"), 1);
    assert_eq!(f2.count("task_intents"), 1);
}

#[tokio::test]
async fn native_retained_corpus_unknown_fate_is_retained() {
    let mut f = Fixture::new("unknown");
    let old = f.admit("unknown_fate", 2000);
    for i in 0..100 {
        f.admit(&format!("recent{i}"), 2001 + i);
    }
    // The corpus sits above the floor because the store clamps every
    // ceiling up to it. An unknown-outcome dispatch: the pin is the dispatch
    // state pair P4/P5 read from `runner_dispatches.state` directly (tick
    // contract D-1 — the `unresolved_dispatches` view is for reporting,
    // never pinning). The session input is marked processed so ONLY the
    // unknown-fate pin holds.
    let sql = f.sql();
    sql.execute(
        "INSERT INTO runner_dispatches(id,session_id,task_id,input,digest,state,fence,not_before) VALUES(?1,?2,NULL,'{}','digest','outcome_unknown',1,0)",
        params!["dispatch_unknown", format!("session_{}", f.agent)],
    )
    .unwrap();
    sql.execute(
        "INSERT INTO dispatch_inputs(dispatch_id,message_sequence) VALUES(?1,?2)",
        params!["dispatch_unknown", old],
    )
    .unwrap();
    drop(sql);
    processed(&f, old, 2500);
    let outcome = f.db.sweep_admitted_corpus(3000, 100, 512).unwrap();
    assert_eq!(outcome.pruned, 0, "unknown fate is retained indefinitely");
    assert_eq!(outcome.remaining, 1);
    assert_eq!(f.count("admitted_messages"), 101);
    assert_eq!(f.count("dispatch_inputs"), 1);
    assert_eq!(f.count("retained_message_archive"), 0);
}

#[tokio::test]
async fn native_retained_corpus_provenance_moves_with_the_message() {
    let mut f = Fixture::new("provenance");
    let old = f.admit("moves", 2000);
    for i in 0..100 {
        f.admit(&format!("recent{i}"), 2001 + i);
    }
    // The corpus sits above the floor because the store clamps every
    // ceiling up to it; the provenance row under test is the one candidate.
    processed(&f, old, 2500);
    let live_key: String = f
        .sql()
        .query_row(
            "SELECT source_key FROM matrix_ingress_events WHERE message_sequence=?1",
            [old],
            |r| r.get(0),
        )
        .unwrap();
    let event = f.observation("moves", 2000);
    let outcome = f.db.sweep_admitted_corpus(3000, 100, 512).unwrap();
    assert_eq!(outcome.pruned, 1);
    // The archive row carries the full ingress identity plus wake (P8'/A3/A4).
    let (engagement, source_key, scope, session, wake): (String, String, String, String, bool) = f
        .sql()
        .query_row(
            "SELECT engagement_id,source_key,scope_digest,source_session_id,wake \
             FROM retained_message_archive WHERE sequence=?1",
            [old],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .unwrap();
    assert_eq!(engagement, f.engagement);
    assert_eq!(
        source_key, live_key,
        "the archive's ingress identity is the pair the live row carried"
    );
    assert!(!scope.is_empty());
    assert_eq!(session, format!("session_{}", f.agent));
    assert!(wake, "wake moved with the message into the archive");
    assert_eq!(
        f.count("matrix_ingress_events"),
        100,
        "the live provenance row is deleted in the same transaction"
    );
    // An exact redelivery is still recognised as admitted: live miss, the
    // archive answers by identity, the receipt reconstructs wake/config.
    let replay = f.db.matrix_ingress_receipt(&event).unwrap().unwrap();
    assert_eq!(replay.sequence, old);
    assert!(replay.wake);
    assert!(!replay.created && !replay.projected);
    // A divergent redelivery under the same event id is still refused with
    // the same word — the archive-side divergence probe is engagement-scoped.
    let mut divergent = event.clone();
    divergent.event.body = "Changed under the same event id".into();
    assert!(f.db.matrix_ingress_receipt(&divergent).is_err());
}

#[tokio::test]
async fn native_retained_corpus_archive_is_bounded() {
    let mut f = Fixture::new("bounded");
    let mut sequences = Vec::new();
    for i in 0..205 {
        sequences.push(f.admit(&format!("window{i}"), 2000 + i));
    }
    // The corpus sits above the floor because the store clamps every
    // ceiling up to it; the archive's own bound needs more than one
    // ceiling of pruned rows to bite.
    for sequence in &sequences[..105] {
        processed(&f, *sequence, 3000);
    }
    let outcome = f.db.sweep_admitted_corpus(4000, 100, 512).unwrap();
    assert_eq!(outcome.pruned, 105);
    // Bounded to the same ceiling, pruned oldest-first IN THE SAME TICK.
    assert_eq!(f.count("retained_message_archive"), 100);
    let kept: Vec<u64> = {
        let sql = f.sql();
        let mut statement = sql
            .prepare("SELECT sequence FROM retained_message_archive ORDER BY sequence")
            .unwrap();
        statement
            .query_map([], |r| r.get::<_, u64>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    };
    assert_eq!(
        kept,
        sequences[5..105].to_vec(),
        "the newest pruned rows stay"
    );
}

#[tokio::test]
async fn native_retained_corpus_parity_with_javascript() {
    // The shared-subset oracle (native/scripts/corpus-retention-vectors.mjs):
    // the retained planMessagePrune partitioned a corpus of `total` rows into
    // pruned/retained; the native predicate must produce the SAME partition
    // on the shared clauses (recency window vs inbox membership) and agree on
    // archive membership. The native-only clauses (P2 claimed, P3', P4..P10)
    // have no retained counterpart and are pinned by the tests above — that
    // honest limit is stated in the oracle's header and in ADR-125.
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/corpus-retention-vectors.json")).unwrap();
    let vectors = &fixture["vectors"];
    // The fixture's sha pin, ENFORCED: the assert below hard-fails this
    // test whenever the retained backend-v2.js bytes drift from the
    // fixture — the file the port never edits, so any change to it is a
    // real event this test must surface (regenerate the fixture and
    // re-derive the vectors in the same commit). The digest matches the
    // oracle's `sha()`: utf-8 bytes, CRLF folded to LF. (The r4 review's
    // F-6: the earlier "provenance, not enforcement" wording understated
    // the gate — the assertion is a drift gate, and the description now
    // says so.)
    {
        use sha2::{Digest, Sha256};
        let source =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../backend-v2.js"))
                .expect("backend-v2.js is readable from the store test");
        let normalized = source.replace("\r\n", "\n");
        let pinned = vectors["backendSha256"].as_str().unwrap();
        let digest = Sha256::digest(normalized.as_bytes());
        assert_eq!(
            pinned,
            &format!("{digest:x}"),
            "the fixture's backendSha256 pin does not match the retained \
             backend-v2.js on this tree; the retained file changed under the \
             oracle — regenerate with `node \
             native/scripts/corpus-retention-vectors.mjs` and re-derive"
        );
    }
    let limit = vectors["observedLimit"].as_u64().unwrap();
    let total = vectors["total"].as_u64().unwrap();
    let pruned_count = vectors["prunedCount"].as_u64().unwrap();
    let retained_count = vectors["retainedCount"].as_u64().unwrap();
    let keep_unread = vectors["keep"]["unread"].as_array().unwrap().len() as u64;
    assert_eq!(pruned_count + retained_count, total);
    // The retained partition: every keep-set member survives (unread
    // membership), plus the one group-mention row the recency window holds
    // (it is not unread — beta is not a member of the empty group seed).
    assert!(
        keep_unread <= retained_count,
        "unread membership is a subset of the retained set"
    );
    let mut f = Fixture::new("parity");
    let mut sequences = Vec::new();
    for i in 0..total {
        sequences.push(f.admit(&format!("parity{i}"), 2000 + i));
    }
    // The vector's seed: the oldest `pruned_count` rows have no live
    // reference; every newer row is unread (P2) membership.
    for sequence in &sequences[..pruned_count as usize] {
        processed(&f, *sequence, 9000);
    }
    let outcome = f.db.sweep_admitted_corpus(10_000, limit, 512).unwrap();
    assert_eq!(outcome.pruned, pruned_count, "the pruned counts agree");
    // The one over-ceiling residue: the oldest UNREAD row sits just below the
    // recency window, P2 holds it, so it survives as corpus 121 against a
    // ceiling of 120 — reported, never a refusal (same shape as the retained
    // planner, which also keeps it).
    assert_eq!(outcome.remaining, 1);
    // Same partition, by position: the pruned set is exactly the oldest
    // `pruned_count` admissions (ORDER BY, never row order).
    let live: Vec<u64> = {
        let sql = f.sql();
        let mut statement = sql
            .prepare("SELECT sequence FROM admitted_messages ORDER BY sequence")
            .unwrap();
        statement
            .query_map([], |r| r.get::<_, u64>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    };
    assert_eq!(live.len() as u64, retained_count);
    assert_eq!(live, sequences[pruned_count as usize..].to_vec());
    // The archive-membership pair (A2): exactly the pruned rows are durably
    // recorded, no retained row is.
    assert_eq!(f.count("retained_message_archive"), pruned_count);
    let archived: u64 = f
        .sql()
        .query_row(
            "SELECT COUNT(*) FROM retained_message_archive WHERE sequence > ?1",
            [sequences[pruned_count as usize - 1]],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(archived, 0, "no retained row is in the archive");
}

#[tokio::test]
async fn native_retained_corpus_floor_is_hundred() {
    let mut f = Fixture::new("floor");
    for i in 0..120 {
        f.admit(&format!("floor{i}"), 2000 + i);
    }
    for i in 1..=120u64 {
        processed(&f, i, 9000);
    }
    // A ceiling below the floor is clamped up to 100 — the same
    // `Math.max(100, …)` guard as the retained env default.
    let status = f.db.retention_status(5).unwrap();
    assert_eq!((status.ceiling, status.over_by), (100, 20));
    let outcome = f.db.sweep_admitted_corpus(10_000, 5, 512).unwrap();
    assert_eq!(outcome.pruned, 20, "the sweep clamps the ceiling too");
    assert_eq!(f.count("admitted_messages"), 100);
}

/// R5 (impl review): the delete list's completeness depends on the pin
/// list's — the two attachment tables are NEVER deleted, safe only because
/// P9/P10 make any row carrying an attachment a non-candidate. Asserted,
/// not assumed: two past-window processed messages, one pinned by a
/// `matrix_attachments` row (P9), one additionally carrying a
/// `session_attachment_visibility` projection (P10), and neither is ever a
/// candidate.
#[tokio::test]
async fn native_retained_corpus_attachment_projection_pins_the_message() {
    let mut f = Fixture::new("attach");
    let pinned9 = f.admit("attachment9", 2000);
    let pinned10 = f.admit("attachment10", 2001);
    for i in 0..100 {
        f.admit(&format!("recent{i}"), 2002 + i);
    }
    processed(&f, pinned9, 2500);
    processed(&f, pinned10, 2500);
    {
        let sql = f.sql();
        for sequence in [pinned9, pinned10] {
            let (engagement, source_key): (String, String) = sql
                .query_row(
                    "SELECT engagement_id,source_key FROM matrix_ingress_events WHERE message_sequence=?1",
                    [sequence],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap();
            sql.execute(
                "INSERT INTO matrix_attachments(engagement_id,source_key,message_sequence,source_session_id,digest,content_digest,metadata,sdk_identity,manifest_id) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                params![engagement, source_key, sequence, format!("session_{}", f.agent), "digest", "content", "{}", "sdk", "manifest"],
            )
            .unwrap();
        }
        // P10: the visibility projection rides the second row (its FK
        // requires the attachment row to exist first).
        sql.execute(
            "INSERT INTO session_attachment_visibility(session_id,engagement_id,source_key,message_sequence) \
             SELECT ?1,engagement_id,source_key,message_sequence FROM matrix_attachments WHERE message_sequence=?2",
            params![format!("session_{}", f.agent), pinned10],
        )
        .unwrap();
    }
    let outcome = f.db.sweep_admitted_corpus(3000, 100, 512).unwrap();
    assert_eq!(
        outcome.pruned, 0,
        "attachment custody pins past-window rows"
    );
    assert_eq!(outcome.remaining, 2);
    assert_eq!(f.count("admitted_messages"), 102);
    assert_eq!(
        f.count("matrix_attachments"),
        2,
        "the projection survives with its parent"
    );
    assert_eq!(f.count("session_attachment_visibility"), 1);
    assert_eq!(f.count("retained_message_archive"), 0);
    // The mirror: the same two rows WITHOUT the projections are candidates.
    let mut f2 = Fixture::new("attachmirror");
    let plain1 = f2.admit("plain9", 2000);
    let plain2 = f2.admit("plain10", 2001);
    for i in 0..100 {
        f2.admit(&format!("recent{i}"), 2002 + i);
    }
    processed(&f2, plain1, 2500);
    processed(&f2, plain2, 2500);
    let outcome2 = f2.db.sweep_admitted_corpus(3000, 100, 512).unwrap();
    assert_eq!(
        outcome2.pruned, 2,
        "the same rows without projections are candidates"
    );
}

/// R6 (impl review), the hit: read 6's archive fallback resolves a pruned
/// thread root whose session route survives — but only on a matching
/// `scope_digest` (A2). The root is the one past-window row; the reply is
/// the newest admission.
#[tokio::test]
async fn native_retained_corpus_threaded_root_resolves_from_archive_by_scope_digest() {
    let mut f = Fixture::new("root6");
    let root = f.admit("root6", 2000);
    for i in 0..99 {
        f.admit(&format!("fill{i}"), 2001 + i);
    }
    let (reply, thread_session) = f.admit_threaded("reply6", 2101, "$root6");
    processed(&f, root, 2500);
    let outcome = f.db.sweep_admitted_corpus(3000, 100, 512).unwrap();
    assert_eq!(outcome.pruned, 1, "the root is the one past-window row");
    let live_root: u64 = f
        .sql()
        .query_row(
            "SELECT COUNT(*) FROM matrix_ingress_events WHERE message_sequence=?1",
            [root],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(live_root, 0, "the root's live provenance is gone");
    // The verified task request from the reply: read 6 must miss live and
    // hit the archive on the matching scope digest.
    let input = VerifiedTaskRequest {
        scope: f.db.matrix_ingress_scope(&thread_session).unwrap(),
        request_key: "root6_request".into(),
        source_sequence: reply,
        definition: TaskDefinition {
            title: "R6 archive hit".into(),
            ..Default::default()
        },
    };
    let task = f.db.create_verified_task_intent(&input, 3001).unwrap();
    let bound: u64 = f
        .sql()
        .query_row(
            "SELECT root_sequence FROM task_intents WHERE task_id=?1",
            [&task.task_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(bound, root, "the intent binds the archived root's sequence");
}

/// R6, the refusal: the same request against an archive row whose
/// `scope_digest` does not match the route refuses with `RunnerAuthority`
/// and creates nothing.
#[tokio::test]
async fn native_retained_corpus_threaded_root_refuses_on_scope_digest_mismatch() {
    let mut f = Fixture::new("root6miss");
    let root = f.admit("root6miss", 2000);
    for i in 0..99 {
        f.admit(&format!("fill{i}"), 2001 + i);
    }
    let (reply, thread_session) = f.admit_threaded("reply6miss", 2101, "$root6miss");
    processed(&f, root, 2500);
    let outcome = f.db.sweep_admitted_corpus(3000, 100, 512).unwrap();
    assert_eq!(outcome.pruned, 1);
    // Diverge the archived root's scope digest (the mismatch under test).
    f.sql()
        .execute(
            "UPDATE retained_message_archive SET scope_digest='mismatch' WHERE sequence=?1",
            [root],
        )
        .unwrap();
    let input = VerifiedTaskRequest {
        scope: f.db.matrix_ingress_scope(&thread_session).unwrap(),
        request_key: "root6miss_request".into(),
        source_sequence: reply,
        definition: TaskDefinition {
            title: "R6 archive miss".into(),
            ..Default::default()
        },
    };
    assert!(matches!(
        f.db.create_verified_task_intent(&input, 3001),
        Err(Error::RunnerAuthority)
    ));
    assert_eq!(f.count("task_intents"), 0, "the refusal creates nothing");
}

/// S4 (store review) + round 3: the archive insert is a keyed overwrite,
/// never a fatal uniqueness abort — and the one exception is stated, not
/// hidden. The stale same-pair row (the "earlier partial path" shape S4
/// names) is replaced by the live row's archive entry, while a write-only
/// NULL-engagement row coexists, because NULL never equals NULL in the
/// unique key and no archive read is scoped to a NULL engagement.
#[tokio::test]
async fn native_retained_corpus_archive_rearchive_is_keyed_not_fatal() {
    let mut f = Fixture::new("rekey");
    // The write-only row FIRST, so its low sequence falls inside the P1
    // window: a non-ingress admission with no provenance and no children.
    f.sql()
        .execute(
            "INSERT INTO admitted_messages(sequence,source_key,digest,config) VALUES(1,?1,?2,?3)",
            params![
                "raw_non_ingress",
                "raw",
                serde_json::json!({"id":"raw"}).to_string()
            ],
        )
        .unwrap();
    let live = f.admit("rekey", 2000);
    for i in 0..100 {
        f.admit(&format!("fill{i}"), 2001 + i);
    }
    processed(&f, live, 2500);
    // The live pair, captured BEFORE the sweep: the provenance row moves
    // with the message, so it cannot be joined after.
    let (pair_engagement, pair_source): (String, String) = {
        let sql = f.sql();
        sql.query_row(
            "SELECT engagement_id,source_key FROM matrix_ingress_events WHERE message_sequence=?1",
            [live],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap()
    };
    // The stale archive row carrying the live message's own
    // (engagement_id, source_key) pair — the hazard INSERT OR REPLACE
    // defuses: without it this shape aborts the whole tick on the UNIQUE.
    f.sql()
        .execute(
            "INSERT INTO retained_message_archive(sequence,engagement_id,source_key,scope_digest,digest,config,source_session_id,wake,pruned_at_ms) VALUES(999999,?1,?2,'stale_scope','stale','{}',NULL,0,1)",
            params![pair_engagement, pair_source],
        )
        .unwrap();
    let outcome = f.db.sweep_admitted_corpus(3000, 100, 512).unwrap();
    assert_eq!(outcome.pruned, 2, "the raw row and the live row both prune");
    // The keyed overwrite: exactly ONE row carries the live pair, and it is
    // the live row's archive entry — the stale digest is gone, the tick
    // never aborted.
    let (pair_rows, fresh_rows): (u64, u64) = {
        let sql = f.sql();
        let pair_rows = sql
            .query_row(
                "SELECT COUNT(*) FROM retained_message_archive WHERE engagement_id=?1 AND source_key=?2",
                params![pair_engagement, pair_source],
                |r| r.get(0),
            )
            .unwrap();
        let fresh_rows = sql
            .query_row(
                "SELECT COUNT(*) FROM retained_message_archive WHERE engagement_id=?1 AND source_key=?2 AND digest<>'stale'",
                params![pair_engagement, pair_source],
                |r| r.get(0),
            )
            .unwrap();
        (pair_rows, fresh_rows)
    };
    assert_eq!(
        pair_rows, 1,
        "the stale same-pair row was replaced, not joined"
    );
    assert_eq!(fresh_rows, 1, "the surviving row is the live row's entry");
    // The NULL-engagement exception, observed: the write-only row coexists.
    let null_rows: u64 = f
        .sql()
        .query_row(
            "SELECT COUNT(*) FROM retained_message_archive WHERE engagement_id IS NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        null_rows, 1,
        "NULL never equals NULL: no collision, no replace"
    );
    let total: u64 = f.count("retained_message_archive");
    assert_eq!(
        total, 2,
        "two archive rows: the keyed pair and the write-only one"
    );
}

/// Migration 026 over populated rows, in the `native_usage_migration` shape:
/// rewind a live head-26 database to 25 and reopen — 026 replays over a
/// database that already carries its objects, and every statement is
/// CREATE ... IF NOT EXISTS with no ALTER, so the replay is a no-op. The
/// migration creates the archive, the receipt table and the seven pin-probe
/// indexes and drains NOTHING; the sweep entry point does the draining.
#[tokio::test]
async fn native_retained_corpus_schema_upgrade() {
    let mut f = Fixture::new("upgrade");
    let prunable1 = f.admit("upgrade1", 2000);
    let prunable2 = f.admit("upgrade2", 2001);
    let pinned_pending = f.admit("upgrade3", 2002);
    let _ = pinned_pending; // P2 holds this row: its session input is never marked processed.
    let pinned_unknown = f.admit("upgrade4", 2003);
    let pinned_task = f.admit("upgrade5", 2004);
    let pinned_attachment = f.admit("upgrade6", 2005);
    // The window fill: the corpus must sit above the floor because the store
    // clamps every ceiling up to it (the `Math.max(100,…)` guard).
    for i in 7..=103 {
        f.admit(&format!("upgrade{i}"), 2000 + i);
    }
    processed(&f, prunable1, 2500);
    processed(&f, prunable2, 2500);
    // P2: unprocessed session input.
    // P5: unknown-fate dispatch with the session input processed.
    {
        let sql = f.sql();
        sql.execute(
            "INSERT INTO runner_dispatches(id,session_id,task_id,input,digest,state,fence,not_before) VALUES(?1,?2,NULL,'{}','digest','outcome_unknown',1,0)",
            params!["dispatch_upgrade", format!("session_{}", f.agent)],
        )
        .unwrap();
        sql.execute(
            "INSERT INTO dispatch_inputs(dispatch_id,message_sequence) VALUES(?1,?2)",
            params!["dispatch_upgrade", pinned_unknown],
        )
        .unwrap();
    }
    processed(&f, pinned_unknown, 2500);
    // P6'/P7': an open task's root and input.
    task(&f, "task_upgrade", "in_progress", pinned_task, true);
    // P9: an attachment row.
    {
        let sql = f.sql();
        let (engagement, source_key): (String, String) = sql
            .query_row(
                "SELECT engagement_id,source_key FROM matrix_ingress_events WHERE message_sequence=?1",
                [pinned_attachment],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        sql.execute(
            "INSERT INTO matrix_attachments(engagement_id,source_key,message_sequence,source_session_id,digest,content_digest,metadata,sdk_identity,manifest_id) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![engagement, source_key, pinned_attachment, format!("session_{}", f.agent), "digest", "content", "{}", "sdk", "manifest"],
        )
        .unwrap();
    }
    let state = f.root.path().join("state");
    drop(f.db);
    {
        let sql = Connection::open(state.join("domain.sqlite3")).unwrap();
        sql.pragma_update(None, "user_version", 25).unwrap();
    }
    // The double open: the second run is at head 26 and replays nothing.
    for _ in 0..2 {
        let db = DomainRepository::open(&state).unwrap();
        drop(db);
        let sql = Connection::open(state.join("domain.sqlite3")).unwrap();
        assert_eq!(
            sql.pragma_query_value(None, "user_version", |r| r.get::<_, u64>(0))
                .unwrap(),
            27
        );
        let archive: u64 = sql
            .query_row("SELECT COUNT(*) FROM retained_message_archive", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(archive, 0, "the migration creates and drains nothing");
        let indexes: u64 = sql
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name IN (?1,?2,?3,?4,?5,?6,?7)",
                params![
                    "session_inputs_message",
                    "dispatch_inputs_message",
                    "task_intents_root",
                    "task_inputs_message",
                    "ingress_event_message",
                    "matrix_attachment_message",
                    "attachment_visibility_message"
                ],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(indexes, 7, "the seven pin-probe indexes exist");
    }
    // Drain through the sweep entry point, one row per tick (the batch).
    let agent = f.agent.clone();
    let mut f2 = Fixture {
        root: f.root,
        db: DomainRepository::open(&state).unwrap(),
        engagement: f.engagement,
        agent,
    };
    let first = f2.db.sweep_admitted_corpus(5000, 100, 1).unwrap();
    assert_eq!(first.pruned, 1, "the batch bound stops at one row");
    assert_eq!(first.remaining, 2, "102 live rows against a ceiling of 100");
    let second = f2.db.sweep_admitted_corpus(5001, 100, 1).unwrap();
    assert_eq!(second.pruned, 1);
    assert_eq!(second.remaining, 1);
    let third = f2.db.sweep_admitted_corpus(5002, 100, 1).unwrap();
    assert_eq!(third.pruned, 0, "only pinned rows remain");
    // The archive holds the pruned content with its full identity.
    let archived: Vec<(u64, String, bool)> = {
        let sql = f2.sql();
        let mut statement = sql
            .prepare("SELECT sequence,engagement_id,wake FROM retained_message_archive ORDER BY sequence")
            .unwrap();
        statement
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    };
    assert_eq!(archived.len(), 2);
    assert_eq!(archived[0].0, prunable1);
    assert_eq!(archived[1].0, prunable2);
    assert_eq!(archived[0].1, f2.engagement);
    // Every pinned row survived with its children.
    assert_eq!(f2.count("admitted_messages"), 101);
    assert_eq!(
        f2.count("session_inputs"),
        101,
        "one per survivor; the pruned rows' children moved with them"
    );
    assert_eq!(f2.count("dispatch_inputs"), 1);
    assert_eq!(f2.count("task_inputs"), 1);
    assert_eq!(f2.count("task_intents"), 1);
    assert_eq!(f2.count("matrix_attachments"), 1);
    // The receipt table: the messages phase, trimmed to the limit.
    let (phases, rows): (String, u64) = f2
        .sql()
        .query_row(
            "SELECT (SELECT DISTINCT phase FROM retention_prune_receipts), COUNT(*) FROM retention_prune_receipts",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(phases, "messages");
    assert!(rows > 0 && rows <= 100);
}
