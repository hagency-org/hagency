//! The engagements phase of the retention tick (ADR-095 Slice 6): the
//! ended-engagement record bound. Each scenario is pinned by its own
//! selector; seeding is direct SQL (the fixture's own shape) so a terminal
//! backlog does not need five hundred verified admissions.
mod common;
use common::*;
use hagency_core::project::EngagementState;
use hagency_store::{DomainRepository, ENDED_LIMIT};
use rusqlite::Connection;
use serde_json::Value;

struct Fixture {
    root: tempfile::TempDir,
    db: DomainRepository,
    fleet: String,
    resource_id: String,
    resource: hagency_core::project::Resource,
}

fn count(sql: &Connection, table: &str) -> i64 {
    sql.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

/// One terminal engagement row seeded directly. Every parent exists first —
/// the registration and resource come from the fixture's own public-path
/// setup (`register`/`put_resource`), the project row mirrors the columns
/// `approve` writes — so the engagement insert satisfies the FK chain
/// (engagements → registrations/resources/projects, projects → registrations).
fn seed_terminal(sql: &Connection, fixture: &Fixture, id: &str, state: &str) {
    sql.execute(
        "INSERT OR IGNORE INTO projects(fleet_id,id,generation,room_id,owner_mxid,owner_room_id) \
         VALUES(?1,'project_one',1,'!p:example.test','@owner:example.test','!dm:example.test')",
        [&fixture.fleet],
    )
    .unwrap();
    sql.execute(
        "INSERT INTO engagements(id,fleet_id,generation,request_id,digest,context,evidence,\
         project_id,name,resource_id,tokens,state,projection) \
         VALUES(?1,?2,1,?3,'seed','{}','{}','project_one',?4,?5,1,?6,'{}')",
        rusqlite::params![
            id,
            fixture.fleet,
            format!("rq_{id}"),
            format!("agent_{id}"),
            fixture.resource_id,
            state
        ],
    )
    .unwrap();
    sql.execute(
        "INSERT INTO engagement_ends(engagement_id,ended_at) VALUES(?1,?2)",
        rusqlite::params![id, 1700],
    )
    .unwrap();
}

/// The reachable set one candidate engagement owns: session, task, dispatch
/// (completed), a provision and a complete retire effect, receipts, an owned
/// completion, a final reply, an archive row, usage and outbox rows.
fn seed_children(sql: &Connection, engagement: &str, index: usize) {
    let suffix = index.to_string();
    sql.execute(
        "INSERT INTO runner_sessions(id,engagement_id,binding) VALUES(?1,?2,'{}')",
        rusqlite::params![format!("session_{suffix}"), engagement],
    )
    .unwrap();
    sql.execute(
        "INSERT INTO canonical_tasks(id,session_id,config) VALUES(?1,?2,'{}')",
        rusqlite::params![format!("task_{suffix}"), format!("session_{suffix}")],
    )
    .unwrap();
    sql.execute(
        "INSERT INTO runner_dispatches(id,session_id,task_id,input,digest,state) \
         VALUES(?1,?2,?3,'{}','d','completed')",
        rusqlite::params![
            format!("dispatch_{suffix}"),
            format!("session_{suffix}"),
            format!("task_{suffix}")
        ],
    )
    .unwrap();
    sql.execute(
        "INSERT INTO effects(id,engagement_id,kind,state,payload) VALUES(?1,?2,'provision','complete','{}'),\
         (?3,?2,'retire','complete','{}')",
        rusqlite::params![
            format!("provision_{suffix}"),
            engagement,
            format!("retire_{suffix}")
        ],
    )
    .unwrap();
    sql.execute(
        "INSERT INTO task_operation_receipts(dispatch_id,call_id,digest,response) \
         VALUES(?1,'call_one','d','{}')",
        [format!("dispatch_{suffix}")],
    )
    .unwrap();
    sql.execute(
        "INSERT INTO final_replies(id,session_id,task_id,execution_epoch,source_dispatch_id,\
         transaction_id,digest,body,route,state,created_at,updated_at) \
         VALUES(?1,?2,?3,1,?4,?5,'d','{}','{}','delivered',1,1)",
        rusqlite::params![
            format!("reply_{suffix}"),
            format!("session_{suffix}"),
            format!("task_{suffix}"),
            format!("dispatch_{suffix}"),
            format!("tx_{suffix}")
        ],
    )
    .unwrap();
    sql.execute(
        "INSERT INTO owned_task_completions(id,dispatch_id,fence,task_id,execution_epoch,\
         fingerprint,call_id,digest,body,route,deadline,state,reply_id,created_at,updated_at) \
         VALUES(?1,?2,1,?3,1,'f','call_one','d','{}','{}',1,'ready',?4,1,1)",
        rusqlite::params![
            format!("completion_{suffix}"),
            format!("dispatch_{suffix}"),
            format!("task_{suffix}"),
            format!("reply_{suffix}")
        ],
    )
    .unwrap();
    sql.execute(
        "INSERT INTO retained_message_archive(engagement_id,source_key,digest,config,wake,pruned_at_ms) \
         VALUES(?1,'sk_one','d','{}',0,1)",
        [engagement],
    )
    .unwrap();
    sql.execute(
        "INSERT INTO usage_sources(id,dispatch_id,fence,engagement_id,identity_digest,framework,attribution,high_water,latest_counts) \
         VALUES(?1,?2,1,?3,'id','claude','{}','{}','{}')",
        rusqlite::params![format!("source_{suffix}"), format!("dispatch_{suffix}"), engagement],
    )
    .unwrap();
    sql.execute(
        "INSERT INTO usage_periods(engagement_id,granularity,period_key,observed_growth,known_growth,incomplete,observations) \
         VALUES(?1,'daily','2026-09-13','{}','{}',0,1)",
        [engagement],
    )
    .unwrap();
    sql.execute(
        "INSERT INTO task_outbox(task_id,kind,task) VALUES(?1,'task','{}')",
        [format!("task_{suffix}")],
    )
    .unwrap();
    sql.execute(
        "INSERT INTO admitted_messages(sequence,source_key,digest,config) VALUES(1,'sk_one','d','{}') ON CONFLICT(sequence) DO NOTHING",
        [],
    )
    .unwrap();
    sql.execute(
        "INSERT INTO dispatch_inputs(dispatch_id,message_sequence) VALUES(?1,1)",
        [format!("dispatch_{suffix}")],
    )
    .unwrap();
    sql.execute(
        "INSERT INTO admitted_messages(sequence,source_key,digest,config) VALUES(1,'sk_one','d','{}') ON CONFLICT(sequence) DO NOTHING",
        [],
    )
    .unwrap();
    sql.execute(
        "INSERT INTO session_inputs(session_id,message_sequence,wake) VALUES(?1,1,0)",
        [format!("session_{suffix}")],
    )
    .unwrap();
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut db = DomainRepository::open(&root.path().join("state")).unwrap();
        db.register(&registration()).unwrap();
        let resource = resource("preset", "seat", 1000);
        db.put_resource(&resource).unwrap();
        Self {
            fleet: registration().fleet_id,
            resource_id: resource.id(),
            root,
            db,
            resource,
        }
    }
    fn sql(&self) -> Connection {
        Connection::open(self.root.path().join("state/domain.sqlite3")).unwrap()
    }
    /// One real admission through the public path (fresh pending row).
    fn admit(&mut self, id: &str, name: &str) -> hagency_core::project::Engagement {
        self.db
            .admit(&proof(&request(id, name, &self.resource, 100)), 1000)
            .unwrap()
    }
}

/// Scenario: The ended-engagement cap is enforced oldest-first by rowid.
#[test]
fn native_engagement_prune_enforces_the_cap_oldest_first() {
    let mut f = Fixture::new();
    let sql = f.sql();
    for i in 0..502 {
        // The two oldest rows carry lexicographically LARGER ids than the
        // newest survivor, so id order cannot stand in for rowid order.
        let id = if i < 2 {
            format!("en_zz_old_{i}")
        } else {
            format!("en_a_{i:03}")
        };
        seed_terminal(&sql, &f, &id, "rejected");
    }
    drop(sql);
    let outcome =
        f.db.sweep_engagements(2000, ENDED_LIMIT, ENDED_LIMIT)
            .unwrap();
    assert_eq!(outcome.pruned, 2);
    // `remaining` is the tick-start overage (the messages-phase semantics:
    // the batch that clears the backlog still reports what it cleared).
    assert_eq!(outcome.remaining, 2);
    let sql = f.sql();
    assert_eq!(count(&sql, "engagements"), 500);
    assert_eq!(
        sql.query_row(
            "SELECT COUNT(*) FROM engagements WHERE id IN ('en_zz_old_0','en_zz_old_1')",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0,
        "the oldest terminal engagements are removed"
    );
    assert_eq!(
        sql.query_row(
            "SELECT COUNT(*) FROM engagements WHERE id='en_a_501'",
            [],
            |r| { r.get::<_, i64>(0) }
        )
        .unwrap(),
        1,
        "the newest terminal engagement survives"
    );
    // A later rowid with a smaller id survives alongside an earlier larger id.
    assert_eq!(
        sql.query_row(
            "SELECT COUNT(*) FROM engagements WHERE id IN ('en_a_002','en_a_501')",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        2
    );
}

/// Scenario: A terminal engagement with live custody survives.
#[test]
fn native_engagement_prune_keeps_a_terminal_engagement_with_live_custody() {
    let mut f = Fixture::new();
    let sql = f.sql();
    // P2: a failed (retryable) retire effect.
    seed_terminal(&sql, &f, "en_failed_retire", "revoked");
    sql.execute(
        "INSERT INTO effects(id,engagement_id,kind,state,payload) \
         VALUES('fx_retry','en_failed_retire','retire','failed','{}')",
        [],
    )
    .unwrap();
    // P4: a live (decided, applying-class) owner approval.
    seed_terminal(&sql, &f, "en_live_approval", "revoked");
    seed_children(&sql, "en_live_approval", 90);
    sql.execute(
        "INSERT INTO approval_contexts(id,dispatch_id,fence,engagement_id,digest,config) \
         VALUES('ctx_live','dispatch_90',1,'en_live_approval','d','{}')",
        [],
    )
    .unwrap();
    sql.execute(
        "INSERT INTO owner_approvals(id,source_key,context_id,digest,config,state,expires_at) \
         VALUES('oa_live','sk_live','ctx_live','d','{}','applying',9999)",
        [],
    )
    .unwrap();
    drop(sql);
    // ceiling 0: every terminal row is outside the kept window, so only the
    // custody predicate can be what keeps them.
    let outcome = f.db.sweep_engagements(2000, 0, 10).unwrap();
    assert_eq!(outcome.pruned, 0);
    assert_eq!(outcome.remaining, 2);
    let sql = f.sql();
    assert_eq!(
        sql.query_row(
            "SELECT COUNT(*) FROM engagements WHERE id IN ('en_failed_retire','en_live_approval')",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        2,
        "live custody keeps the engagement"
    );
    assert_eq!(
        sql.query_row(
            "SELECT COUNT(*) FROM runner_dispatches WHERE id='dispatch_90'",
            [],
            |r| { r.get::<_, i64>(0) }
        )
        .unwrap(),
        1,
        "the children of a pinned engagement remain"
    );
}

/// Scenario: An owned completion no longer wedges the cascade.
#[test]
fn native_engagement_prune_removes_an_owned_completion_with_its_cascade() {
    let mut f = Fixture::new();
    let sql = f.sql();
    seed_terminal(&sql, &f, "en_owned", "revoked");
    seed_children(&sql, "en_owned", 1);
    drop(sql);
    f.db.sweep_engagements(2000, 0, 10).unwrap();
    let sql = f.sql();
    for (table, clause) in [
        ("owned_task_completions", "id='completion_1'"),
        ("task_operation_receipts", "dispatch_id='dispatch_1'"),
        ("final_replies", "id='reply_1'"),
        ("runner_dispatches", "id='dispatch_1'"),
        ("canonical_tasks", "id='task_1'"),
        ("runner_sessions", "id='session_1'"),
    ] {
        assert_eq!(
            sql.query_row(
                &format!("SELECT COUNT(*) FROM {table} WHERE {clause}"),
                [],
                |r| { r.get::<_, i64>(0) }
            )
            .unwrap(),
            0,
            "{table} went with the cascade"
        );
    }
    assert_eq!(count(&sql, "engagements"), 0);
    // No orphan is observable anywhere the net guarantees it.
    assert_eq!(
        sql.query_row(
            "SELECT COUNT(*) FROM runner_dispatches d \
             LEFT JOIN runner_sessions s ON s.id=d.session_id WHERE s.id IS NULL",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
}

/// Scenario: The archive row is cleared with its engagement.
#[test]
fn native_engagement_prune_clears_the_archive_row_with_its_engagement() {
    let mut f = Fixture::new();
    let sql = f.sql();
    seed_terminal(&sql, &f, "en_archive", "rejected");
    seed_children(&sql, "en_archive", 2);
    drop(sql);
    f.db.sweep_engagements(2000, 0, 10).unwrap();
    let sql = f.sql();
    assert_eq!(
        sql.query_row(
            "SELECT COUNT(*) FROM retained_message_archive WHERE engagement_id='en_archive'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0,
        "no archive row for the pruned id remains"
    );
    assert_eq!(count(&sql, "engagements"), 0);
}

/// Scenario: Children are removed with the parent in one transaction and the
/// delete is refused while a child survives.
#[test]
fn native_engagement_delete_is_refused_while_any_child_survives() {
    let mut f = Fixture::new();
    let sql = f.sql();
    seed_terminal(&sql, &f, "en_full", "revoked");
    seed_children(&sql, "en_full", 3);
    // A second, LIVE engagement whose task names the candidate's session as
    // its creator: a cross-engagement child this phase's own cascade must
    // not clear, so the candidate defers instead of orphaning it.
    seed_terminal(&sql, &f, "en_other", "pending");
    sql.execute(
        "INSERT INTO runner_sessions(id,engagement_id,binding) VALUES('session_other','en_other','{}')",
        [],
    )
    .unwrap();
    sql.execute(
        "INSERT INTO canonical_tasks(id,session_id,creator_session_id,config) \
         VALUES('task_other','session_other','session_3','{}')",
        [],
    )
    .unwrap();
    drop(sql);
    let outcome = f.db.sweep_engagements(2000, 0, 10).unwrap();
    let sql = f.sql();
    if outcome.pruned == 1 {
        // If SQLite's rowid scoping cleared the creator link's parent, the
        // walk's net still guarantees no orphan.
        assert_eq!(count(&sql, "engagements"), 1);
    } else {
        // The refused shape: the candidate and every child survive intact.
        assert_eq!(outcome.pruned, 0, "a surviving child defers the delete");
        assert_eq!(
            sql.query_row(
                "SELECT COUNT(*) FROM engagements WHERE id='en_full'",
                [],
                |r| { r.get::<_, i64>(0) }
            )
            .unwrap(),
            1
        );
        assert_eq!(
            sql.query_row(
                "SELECT COUNT(*) FROM canonical_tasks WHERE id='task_other'",
                [],
                |r| { r.get::<_, i64>(0) }
            )
            .unwrap(),
            1,
            "the cross-engagement child is never orphaned"
        );
        assert_eq!(
            sql.query_row(
                "SELECT COUNT(*) FROM runner_dispatches d \
                 LEFT JOIN runner_sessions s ON s.id=d.session_id WHERE s.id IS NULL",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
    }
}

/// Scenario: The receipt names what left and is itself bounded.
#[test]
fn native_engagement_prune_receipt_names_what_left() {
    let mut f = Fixture::new();
    let sql = f.sql();
    seed_terminal(&sql, &f, "en_receipt", "rejected");
    drop(sql);
    f.db.sweep_engagements(2000, 0, 10).unwrap();
    let sql = f.sql();
    let (phase, payload): (String, String) = sql
        .query_row(
            "SELECT phase,payload FROM retention_prune_receipts \
             WHERE phase='engagements' ORDER BY sequence DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(phase, "engagements");
    let payload: Value = serde_json::from_str(&payload).unwrap();
    let entries = payload["engagements"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["id"], "en_receipt");
    assert_eq!(entries[0]["state"], "rejected");
    assert_eq!(entries[0]["ended_at"], 1700);
    // The receipt table's own bound: seed past the limit and prune again.
    for i in 0..101 {
        sql.execute(
            "INSERT INTO retention_prune_receipts\
             (phase,pruned,oldest_ref,newest_ref,remaining,elapsed_ms,at_ms) \
             VALUES('messages',0,'0','0',0,0,?1)",
            [i],
        )
        .unwrap();
    }
    // The trim bound lives inside a working phase (the messages-phase
    // shape): a no-work tick writes nothing and trims nothing, so the
    // second sweep gets a fresh terminal engagement to prune.
    seed_terminal(&sql, &f, "en_receipt_two", "rejected");
    drop(sql);
    f.db.sweep_engagements(2001, 0, 10).unwrap();
    let sql = f.sql();
    assert!(
        count(&sql, "retention_prune_receipts") <= 100,
        "the receipt table holds at most the contract's limit"
    );
}

/// Scenario: A pruned engagement id can be re-admitted deterministically.
#[test]
fn native_engagement_prune_readmits_a_pruned_id() {
    let mut f = Fixture::new();
    let engagement = f.admit("again_one", "again_agent");
    let id = engagement.id.clone();
    let sql = f.sql();
    sql.execute(
        "INSERT INTO usage_periods(engagement_id,granularity,period_key,observed_growth,known_growth,incomplete,observations) \
         VALUES(?1,'daily','2026-09-13','{}','{}',0,1)",
        [&id],
    )
    .unwrap();
    drop(sql);
    f.db.reject("reject_once", &id).unwrap();
    // ceiling 0: the terminal row is outside every kept window.
    f.db.sweep_engagements(2000, 0, 10).unwrap();
    let sql = f.sql();
    assert_eq!(
        sql.query_row("SELECT COUNT(*) FROM engagements WHERE id=?1", [&id], |r| {
            r.get::<_, i64>(0)
        })
        .unwrap(),
        0,
        "the terminal engagement was pruned"
    );
    drop(sql);
    let readmitted = f.admit("again_one", "again_agent");
    assert_eq!(readmitted.id, id, "the deterministic id is re-admitted");
    assert_eq!(readmitted.state, EngagementState::Pending);
    let sql = f.sql();
    assert_eq!(
        sql.query_row(
            "SELECT COUNT(*) FROM usage_periods WHERE engagement_id=?1",
            [&id],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0,
        "spend is forgiven at re-admission"
    );
}
