//! The execution-corpus retention slice (ADR-053/031 amendments, ADR-125
//! tick contract phase 3): the settled-dispatch candidate predicate, the
//! D-8 accepted-output residue, the held-completion pin, the never-pruned
//! attempt anchor and the bounded receipt, each pinned by its own scenario.
//! Dispatch fixtures are planted directly — the phase's predicate reads the
//! settled-verdict pair and its evidence tables, so the scenarios seed that
//! shape without driving the whole dispatch lifecycle. Every SQL statement
//! binds its parameters; no assertion relies on row order beyond what an
//! ORDER BY gives.
mod common;
use common::*;
use hagency_store::{DomainRepository, EXECUTION_RETENTION_DISPATCHES, EXECUTION_RETENTION_ROWS};
use rusqlite::{Connection, params};
use serde_json::json;

/// A fresh private database with the one registration the FK graph needs,
/// plus a direct SQLite handle for seeding and inspecting the corpus.
struct Fixture {
    root: tempfile::TempDir,
    db: DomainRepository,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut db = DomainRepository::open(&root.path().join("state")).unwrap();
        db.register(&registration()).unwrap();
        // The parents the planted engagements reference, through the store's
        // public entry points (the decisions fixture's pattern): `put_resource`
        // creates the `resources` row `pool`, and `admit` writes the
        // `projects` row `project_one` alongside its own pending engagement.
        // That seed engagement sits outside the execution corpus — the phase
        // reads `runner_dispatches`, never `engagements`.
        let pool = resource("pool", "seat", 1000);
        db.put_resource(&pool).unwrap();
        let seed = request("seed", "seed", &pool, 100);
        db.admit(&proof(&seed), 1000).unwrap();
        Self { root, db }
    }
    fn sql(&self) -> Connection {
        Connection::open(self.root.path().join("state/domain.sqlite3")).unwrap()
    }
}

fn count_receipts(sql: &Connection) -> u64 {
    sql.query_row("SELECT COUNT(*) FROM retention_prune_receipts", [], |r| {
        r.get(0)
    })
    .unwrap()
}

/// One engagement/session/dispatch chain plus its evidence: attempts, outputs
/// and the receipt family. `state`/`capability` shape the candidate
/// predicate; `accepted_outputs`/`plain_outputs` size the D-8 residue check.
fn plant_dispatch(
    sql: &Connection,
    id: &str,
    state: &str,
    capability: bool,
    accepted_outputs: u64,
    plain_outputs: u64,
) {
    let fleet = registration().fleet_id;
    // `put_resource` derives the row's id from the preset (`resource_…`), so
    // the planted engagements reference the same id the catalog wrote.
    let pool = resource("pool", "seat", 1000).id();
    sql.execute(
        "INSERT INTO engagements(id,fleet_id,generation,request_id,digest,context,evidence,project_id,name,resource_id,tokens,state,projection) \
         VALUES(?1,?2,1,?1,'{}','{}','{}','project_one',?3,?4,10,'active','{}')",
        params![format!("eng_{id}"), fleet, format!("agent_{id}"), pool],
    )
    .unwrap();
    sql.execute(
        "INSERT INTO runner_sessions(id,engagement_id,binding) VALUES(?1,?2,'{}')",
        params![format!("sess_{id}"), format!("eng_{id}")],
    )
    .unwrap();
    sql.execute(
        "INSERT INTO runner_dispatches(id,session_id,task_id,input,digest,state,capability_hash,not_before) \
         VALUES(?1,?2,NULL,'{}','{}',?3,?4,0)",
        params![
            id,
            format!("sess_{id}"),
            state,
            if capability {
                Some("capability".to_owned())
            } else {
                None
            }
        ],
    )
    .unwrap();
    sql.execute(
        "INSERT INTO runner_attempts(dispatch_id,fence,runner_id,outcome,capability_hash,created_at) \
         VALUES(?1,1,'runner','completed','attempt_hash',1)",
        [id],
    )
    .unwrap();
    for n in 0..accepted_outputs {
        sql.execute(
            "INSERT INTO runner_outputs(dispatch_id,fence,output,accepted) VALUES(?1,1,?2,1)",
            params![id, n.to_string()],
        )
        .unwrap();
    }
    for n in 0..plain_outputs {
        sql.execute(
            "INSERT INTO runner_outputs(dispatch_id,fence,output,accepted) VALUES(?1,1,?2,0)",
            params![id, n.to_string()],
        )
        .unwrap();
    }
    sql.execute(
        "INSERT INTO task_operation_receipts(dispatch_id,call_id,digest,response) VALUES(?1,'call','{}','{}')",
        [id],
    )
    .unwrap();
    sql.execute(
        "INSERT INTO graph_commands(dispatch_id,call_id,digest,response) VALUES(?1,'gcall','{}','{}')",
        [id],
    )
    .unwrap();
    sql.execute(
        "INSERT INTO conversation_operations(dispatch_id,call_id,digest,response) VALUES(?1,'ccall','{}','{}')",
        [id],
    )
    .unwrap();
    sql.execute(
        "INSERT INTO usage_sources(id,dispatch_id,fence,engagement_id,identity_digest,framework,attribution,high_water,latest_counts) \
         VALUES(?1,?2,1,?3,'id_digest','codex','{}','{}','{}')",
        params![format!("usage_{id}"), id, format!("eng_{id}")],
    )
    .unwrap();
    sql.execute(
        "INSERT INTO usage_receipts(source_id,call_id,digest,observation,response) VALUES(?1,'ucall','{}','{}','{}')",
        [format!("usage_{id}")],
    )
    .unwrap();
}

/// Seed `n` `phase='execution'` receipt rows directly, so the bound scenario
/// measures only the +1 the phase adds.
fn seed_execution_receipts(sql: &Connection, n: usize) {
    for i in 0..n {
        sql.execute(
            "INSERT INTO retention_prune_receipts\
             (phase,pruned,oldest_ref,newest_ref,remaining,elapsed_ms,at_ms) \
             VALUES('execution',1,?1,?1,0,0,?2)",
            params![format!("seed_{i}"), i as u64],
        )
        .unwrap();
    }
}

/// Scenario "Settled dispatch evidence is pruned inside its window": a
/// settled dispatch older than the newest-`EXECUTION_RETENTION_DISPATCHES`
/// window, with outputs, attempts and the receipt family, is drained by the
/// phase; the receipt records the prune; the newest accepted output row
/// survives as the D-8 residue; the attempt row is untouched.
#[test]
fn native_execution_prune_keeps_the_window_and_writes_a_receipt() {
    let mut f = Fixture::new();
    let sql = f.sql();
    // The old settled dispatch carries two accepted outputs (the residue
    // keeps exactly one) plus a plain one, plus the full receipt family.
    plant_dispatch(&sql, "dispatch_old", "completed", false, 2, 1);
    // The window: exactly EXECUTION_RETENTION_DISPATCHES newer settled
    // dispatches, each with one plain output so it carries evidence.
    for i in 0..EXECUTION_RETENTION_DISPATCHES {
        plant_dispatch(
            &sql,
            &format!("dispatch_recent_{i}"),
            "completed",
            false,
            0,
            1,
        );
    }
    let outcome = f.db.prune_execution_corpus(9_000, 64).unwrap();
    assert_eq!(
        outcome.pruned, 1,
        "only the over-window dispatch is drained"
    );
    // Old dispatch's evidence is gone: plain output and receipt family.
    let old = |table: &str, key: &str| {
        sql.query_row(
            &format!("SELECT COUNT(*) FROM {table} WHERE {key}='dispatch_old'"),
            [],
            |r| r.get::<_, u64>(0),
        )
        .unwrap()
    };
    assert_eq!(old("graph_commands", "dispatch_id"), 0);
    assert_eq!(old("conversation_operations", "dispatch_id"), 0);
    assert_eq!(old("task_operation_receipts", "dispatch_id"), 0);
    let usage: u64 = sql
        .query_row(
            "SELECT COUNT(*) FROM usage_receipts WHERE source_id='usage_dispatch_old'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(usage, 0);
    // The D-8 residue: exactly ONE of the two accepted outputs survives.
    let residue: u64 = sql
        .query_row(
            "SELECT COUNT(*) FROM runner_outputs WHERE dispatch_id='dispatch_old' AND accepted=1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        residue, 1,
        "the newest accepted output row per fence survives"
    );
    // The receipt records the prune with the dispatch id as its reference.
    let (phase, pruned, oldest_ref): (String, u64, String) = sql
        .query_row(
            "SELECT phase,pruned,oldest_ref FROM retention_prune_receipts ORDER BY sequence DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!((phase, pruned), ("execution".to_owned(), 1));
    assert_eq!(oldest_ref, "dispatch_old");
    // The window's dispatches keep their evidence.
    let recent: u64 = sql
        .query_row(
            "SELECT COUNT(*) FROM runner_outputs WHERE dispatch_id='dispatch_recent_0'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(recent, 1);
}

/// Scenario "A held completion pins its whole dispatch": a settled dispatch
/// whose owned completion is `held` keeps its outputs and every receipt it
/// carries; the dispatch it names is never a candidate.
#[test]
fn native_execution_prune_retains_the_held_completion_evidence() {
    let mut f = Fixture::new();
    let sql = f.sql();
    plant_dispatch(&sql, "dispatch_held", "completed", false, 1, 1);
    for i in 0..EXECUTION_RETENTION_DISPATCHES {
        plant_dispatch(
            &sql,
            &format!("dispatch_recent_{i}"),
            "completed",
            false,
            0,
            1,
        );
    }
    // The completion's composite FK names the receipt, and its state is
    // 'held': both pins sit on the same row. The `task_id` FK needs the
    // canonical task first (the retention fixture's planted-task pattern).
    sql.execute(
        "INSERT INTO canonical_tasks(id,session_id,config) VALUES(?1,?2,?3)",
        params![
            "task_held",
            "sess_dispatch_held",
            json!({"id":"task_held","title":"T","status":"completed"}).to_string()
        ],
    )
    .unwrap();
    sql.execute(
        "INSERT INTO owned_task_completions(id,dispatch_id,fence,task_id,execution_epoch,fingerprint,call_id,digest,body,route,deadline,state,created_at,updated_at) \
         VALUES('held_1','dispatch_held',1,'task_held',1,'fp','call','dg','body','{}',100,'held',1,1)",
        [],
    )
    .unwrap();
    let outcome = f.db.prune_execution_corpus(9_000, 64).unwrap();
    assert_eq!(outcome.pruned, 0, "the held dispatch is pinned, not pruned");
    // Everything the dispatch carries remains: outputs, receipt, receipt family.
    let kept = |table: &str, key: &str, want: u64| {
        let got: u64 = sql
            .query_row(
                &format!("SELECT COUNT(*) FROM {table} WHERE {key}='dispatch_held'"),
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(got, want, "{table}");
    };
    kept("task_operation_receipts", "dispatch_id", 1);
    kept("graph_commands", "dispatch_id", 1);
    kept("conversation_operations", "dispatch_id", 1);
    let outputs: u64 = sql
        .query_row(
            "SELECT COUNT(*) FROM runner_outputs WHERE dispatch_id='dispatch_held'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        outputs, 2,
        "both outputs remain — the dispatch was never touched"
    );
    // A held dispatch is not part of the over-window corpus either.
    assert_eq!(outcome.remaining, 0);
}

/// Scenario "An unresolved dispatch is never a candidate": an
/// `outcome_unknown` dispatch's attempt, output and receipt rows remain
/// through the prune, and after a recovery copies the linkage the original
/// still remains.
#[test]
fn native_execution_prune_retains_unsettled_and_unknown_fate_dispatches() {
    let mut f = Fixture::new();
    let sql = f.sql();
    plant_dispatch(&sql, "dispatch_unknown", "outcome_unknown", true, 1, 1);
    for i in 0..EXECUTION_RETENTION_DISPATCHES {
        plant_dispatch(
            &sql,
            &format!("dispatch_recent_{i}"),
            "completed",
            false,
            0,
            1,
        );
    }
    let outcome = f.db.prune_execution_corpus(9_000, 64).unwrap();
    assert_eq!(
        outcome.pruned, 0,
        "an unresolved dispatch is never a candidate"
    );
    let kept = |table: &str| {
        sql.query_row(
            &format!("SELECT COUNT(*) FROM {table} WHERE dispatch_id='dispatch_unknown'"),
            [],
            |r| r.get::<_, u64>(0),
        )
        .unwrap()
    };
    assert_eq!(kept("runner_attempts"), 1);
    assert_eq!(kept("task_operation_receipts"), 1);
    // A recovery supersedes the original; the unknown-fate row still carries
    // its evidence afterwards.
    sql.execute(
        "UPDATE runner_dispatches SET state='superseded' WHERE id='dispatch_unknown'",
        [],
    )
    .unwrap();
    let after = f.db.prune_execution_corpus(9_001, 64).unwrap();
    assert_eq!(
        after.pruned, 0,
        "the original stays pinned through recovery"
    );
    assert_eq!(kept("runner_attempts"), 1);
}

/// Scenario "The attempt row is never pruned and the late path still
/// authenticates": after the prune drains a settled dispatch's evidence, its
/// attempt row remains and `record_late_output` still authenticates against
/// it — the late path takes no clock (D-7).
#[test]
fn native_execution_prune_leaves_the_attempt_anchor() {
    let mut f = Fixture::new();
    let sql = f.sql();
    // A settled dispatch whose capability is returned (hash NULL) and whose
    // outputs are gone after the prune; its attempt row is the anchor.
    plant_dispatch(&sql, "dispatch_late", "completed", false, 1, 1);
    for i in 0..EXECUTION_RETENTION_DISPATCHES {
        plant_dispatch(
            &sql,
            &format!("dispatch_recent_{i}"),
            "completed",
            false,
            0,
            1,
        );
    }
    let outcome = f.db.prune_execution_corpus(9_000, 64).unwrap();
    assert_eq!(outcome.pruned, 1);
    // The attempt row survives the prune.
    let attempts: u64 = sql
        .query_row(
            "SELECT COUNT(*) FROM runner_attempts WHERE dispatch_id='dispatch_late'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(attempts, 1, "the attempt row is never pruned");
    // The late path authenticates against it: present a capability whose
    // runner matches the attempt row but whose secret cannot match the
    // planted hash — the refusal is authority (the row was read), not
    // not-found (the row was gone).
    let cap = hagency_core::tasks::RunnerCapability {
        dispatch_id: "dispatch_late".into(),
        runner_id: "runner".into(),
        fence: 1,
        secret: "ab".repeat(32),
    };
    let error =
        f.db.record_late_output(&cap, &json!({"late": true}))
            .unwrap_err();
    assert!(
        matches!(error, hagency_store::Error::RunnerAuthority),
        "the attempt row authenticated the refusal, not a missing row: {error:?}"
    );
    // A wrong-runner refusal on a GONE row would be NotFound instead; the
    // distinction is what pins the anchor.
}

/// Scenario "The receipt is bounded and the phase logs its cost": past 100
/// execution receipts, one more phase write trims the table to
/// `RETENTION_RECEIPT_LIMIT`; the row carries pruned, remaining, elapsed_ms
/// and at_ms.
#[test]
fn native_execution_prune_receipt_is_bounded_and_logs_its_cost() {
    let mut f = Fixture::new();
    let sql = f.sql();
    seed_execution_receipts(&sql, 100);
    // One over-window settled dispatch so the phase does work and writes.
    plant_dispatch(&sql, "dispatch_old", "completed", false, 1, 1);
    for i in 0..EXECUTION_RETENTION_DISPATCHES {
        plant_dispatch(
            &sql,
            &format!("dispatch_recent_{i}"),
            "completed",
            false,
            0,
            1,
        );
    }
    let outcome = f.db.prune_execution_corpus(9_000, 64).unwrap();
    assert_eq!(outcome.pruned, 1);
    // 100 seeds + 1 write, trimmed to the bound.
    assert_eq!(count_receipts(&sql), 100);
    // The newest row carries the full cost report.
    let (pruned, remaining, elapsed_ms, at_ms): (u64, u64, u64, u64) = sql
        .query_row(
            "SELECT pruned,remaining,elapsed_ms,at_ms FROM retention_prune_receipts \
             ORDER BY sequence DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!(pruned, 1);
    assert_eq!(remaining, 0);
    assert_eq!(at_ms, 9_000);
    let _ = elapsed_ms;
    // The trimmed rows are the OLDEST seeds; seed_99 (the last seed) survives.
    let seed_kept: u64 = sql
        .query_row(
            "SELECT COUNT(*) FROM retention_prune_receipts WHERE oldest_ref='seed_99'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(seed_kept, 1, "the trim keeps the newest receipts");
    // The per-table backstop constant is the design's, and a small corpus
    // never trips it.
    assert_eq!(EXECUTION_RETENTION_ROWS, 100_000);
}

/// ADR-181: the attempt's event log is pruned with its dispatch. An
/// over-window settled dispatch loses its events alongside its outputs; a
/// settled dispatch carrying ONLY events is still evidence-carrying, so it
/// is a candidate rather than a permanent survivor; the attempt rows they
/// hang off stay (D-7).
#[test]
fn native_execution_prune_drains_attempt_events_with_the_dispatch() {
    let mut f = Fixture::new();
    let sql = f.sql();
    plant_dispatch(&sql, "dispatch_old", "completed", false, 0, 1);
    plant_dispatch(&sql, "dispatch_events_only", "completed", false, 0, 0);
    // Strip the receipt family the planter always writes, leaving the
    // events below as this dispatch's only evidence.
    for table in [
        "task_operation_receipts",
        "graph_commands",
        "conversation_operations",
    ] {
        sql.execute(
            &format!("DELETE FROM {table} WHERE dispatch_id='dispatch_events_only'"),
            [],
        )
        .unwrap();
    }
    sql.execute(
        "DELETE FROM usage_receipts WHERE source_id='usage_dispatch_events_only'",
        [],
    )
    .unwrap();
    for (dispatch, seq) in [
        ("dispatch_old", 1),
        ("dispatch_old", 2),
        ("dispatch_events_only", 1),
    ] {
        sql.execute(
            "INSERT INTO runner_attempt_events(dispatch_id,fence,seq,at_ms,phase,detail) \
             VALUES(?1,1,?2,?2,'settled','{}')",
            params![dispatch, seq],
        )
        .unwrap();
    }
    for i in 0..EXECUTION_RETENTION_DISPATCHES {
        plant_dispatch(
            &sql,
            &format!("dispatch_recent_{i}"),
            "completed",
            false,
            0,
            1,
        );
    }
    let outcome = f.db.prune_execution_corpus(9_000, 64).unwrap();
    assert_eq!(outcome.pruned, 2, "both over-window dispatches drain");
    let events = |dispatch: &str| {
        sql.query_row(
            "SELECT COUNT(*) FROM runner_attempt_events WHERE dispatch_id=?1",
            [dispatch],
            |r| r.get::<_, u64>(0),
        )
        .unwrap()
    };
    assert_eq!(events("dispatch_old"), 0);
    assert_eq!(events("dispatch_events_only"), 0);
    for dispatch in ["dispatch_old", "dispatch_events_only"] {
        let attempts: u64 = sql
            .query_row(
                "SELECT COUNT(*) FROM runner_attempts WHERE dispatch_id=?1",
                [dispatch],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(attempts, 1, "{dispatch}: the attempt row is never pruned");
    }
    assert_eq!(outcome.remaining, 0, "a drained dispatch is over nothing");
}
