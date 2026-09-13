//! The decision-receipt bound (ADR-095 amendment, retention Slice 3): the
//! `decisions` trim runs in-write, inside the deciding command's own
//! transaction, bounded oldest-first by `rowid` with `retry_cleanup` excluded.
//! Every scenario is pinned by its own selector; no assertion relies on row
//! order beyond what an ORDER BY gives.
mod common;
use common::*;
use hagency_store::{DomainRepository, EffectOutcome};
use rusqlite::Connection;

/// A fresh private database with one resource, plus a direct SQLite handle on
/// the same file for seeding and inspecting `decisions` / receipts.
struct Fixture {
    root: tempfile::TempDir,
    db: DomainRepository,
    pool: hagency_core::project::Resource,
}

fn count(sql: &Connection, table: &str) -> i64 {
    sql.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

fn count_decisions(sql: &Connection) -> i64 {
    count(sql, "decisions")
}

fn count_receipts(sql: &Connection) -> i64 {
    count(sql, "retention_prune_receipts")
}

/// Seed `n` plain decisions (digest/result that never match a retry_cleanup
/// recomputation) directly, bypassing `record_decision` so no trim fires.
fn seed_decisions(sql: &Connection, n: usize) {
    for i in 0..n {
        sql.execute(
            "INSERT INTO decisions(id,digest,result) VALUES(?1,'ordinary','{}')",
            [format!("cmd_{i}")],
        )
        .unwrap();
    }
}

fn seed_receipts(sql: &Connection, n: usize) {
    for i in 0..n {
        sql.execute(
            "INSERT INTO retention_prune_receipts\
             (phase,pruned,oldest_ref,newest_ref,remaining,elapsed_ms,at_ms) \
             VALUES('messages',0,'0','0',0,0,0)",
            [i],
        )
        .unwrap();
    }
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut db = DomainRepository::open(&root.path().join("state")).unwrap();
        db.register(&registration()).unwrap();
        let pool = resource("pool", "seat", 1000);
        db.put_resource(&pool).unwrap();
        Self { root, db, pool }
    }
    fn sql(&self) -> Connection {
        Connection::open(self.root.path().join("state/domain.sqlite3")).unwrap()
    }
    /// Admit then reject one engagement, recording one decision and thus
    /// triggering the in-write trim. Returns the engagement id.
    fn decide(&mut self, id: &str, name: &str) -> String {
        let request = request(id, name, &self.pool, 100);
        let proof = proof(&request);
        let engagement = self.db.admit(&proof, 1000).unwrap();
        self.db.reject(id, &engagement.id).unwrap();
        engagement.id
    }
    /// A revoked engagement whose retire effect is `failed` — the shape
    /// `retry_cleanup` resets.
    fn revoked_with_failed_retire(&mut self, id: &str, name: &str) -> String {
        let request = request(id, name, &self.pool, 100);
        let proof = proof(&request);
        let engagement = self.db.admit(&proof, 1000).unwrap();
        self.db.approve("approve", &proof, 1000).unwrap();
        let provision = self.db.claim_effect().unwrap().unwrap();
        self.db
            .observe_effect(
                &provision.id,
                provision.fence,
                &EffectOutcome::Applied {
                    receipt: "provisioned".into(),
                },
            )
            .unwrap();
        self.db.revoke("revoke", &engagement.id).unwrap();
        let retire = self.db.claim_effect().unwrap().unwrap();
        self.db
            .observe_effect(
                &retire.id,
                retire.fence,
                &EffectOutcome::NotApplied {
                    receipt: "not_applied".into(),
                },
            )
            .unwrap();
        engagement.id
    }
}

/// Scenario "The decision window is bounded oldest-first by rowid": more than
/// 500 decisions, one more records, the oldest beyond the window are gone, the
/// newest 500 (and the maximum-rowid row) survive, and a `phase='decisions'`
/// receipt records the prune.
#[test]
fn native_decision_prune_keeps_the_newest_and_never_the_live_effect() {
    let mut f = Fixture::new();
    let sql = f.sql();
    seed_decisions(&sql, 501);
    f.decide("trim_trigger", "trigger agent");
    assert_eq!(count_decisions(&sql), 500);
    // The two oldest rows (rowid 1,2) are pruned; the high-water mark is kept.
    let kept: i64 = sql
        .query_row(
            "SELECT COUNT(*) FROM decisions WHERE id='trim_trigger'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(kept, 1, "the newest verdict is never deleted");
    let pruned_away: i64 = sql
        .query_row(
            "SELECT COUNT(*) FROM decisions WHERE id IN ('cmd_0','cmd_1')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        pruned_away, 0,
        "the oldest verdicts beyond the window are gone"
    );
    let receipt: (i64, String, String) = sql
        .query_row(
            "SELECT pruned,oldest_ref,newest_ref FROM retention_prune_receipts \
             WHERE phase='decisions' ORDER BY sequence DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(receipt, (2, "1".into(), "2".into()));
}

/// Scenario "A legitimate first retry_cleanup is never refused because of the
/// prune": a `retry_cleanup` decision is excluded from the candidate set by
/// recomputing its digest, so an old retry decision survives while an equally
/// old ordinary decision is pruned.
#[test]
fn native_decision_prune_never_refuses_a_legitimate_first_retry() {
    let mut f = Fixture::new();
    let sql = f.sql();
    let id = f.revoked_with_failed_retire("retry_src", "retry agent");
    // First retry (fresh command id) resets the failed retire to pending and
    // records the retry_cleanup decision as the lowest rowid.
    let retried = f.db.retry_cleanup("first_retry", &id).unwrap();
    assert_eq!(retried.id, id);
    // Fill the window with 500 ordinary decisions (rowid 2..501).
    seed_decisions(&sql, 500);
    // One more decide drives the trim: the retry_cleanup row (rowid 1) is the
    // oldest candidate but must be excluded; cmd_0 (rowid 2) is pruned.
    f.decide("trim_trigger", "trigger agent");
    let retry_kept: i64 = sql
        .query_row(
            "SELECT COUNT(*) FROM decisions WHERE id='first_retry'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(retry_kept, 1, "retry_cleanup decisions are never pruned");
    let ordinary_pruned: i64 = sql
        .query_row("SELECT COUNT(*) FROM decisions WHERE id='cmd_0'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(ordinary_pruned, 0, "the oldest ordinary decision is pruned");
}

/// Scenario "A retried retry_cleanup leaves the failed retire effect
/// unchanged": a retry with the SAME command id replays the stored result and
/// never re-runs the mutation, so the failed retire effect is untouched.
#[test]
fn native_decision_replay_window_is_bounded() {
    let mut f = Fixture::new();
    let id = f.revoked_with_failed_retire("replay_src", "replay agent");
    // First retry resets the retire effect to pending and records its decision.
    f.db.retry_cleanup("retry_cmd", &id).unwrap();
    // Drive the retire effect back to `failed`: claim it (started) and observe
    // a definitive NotApplied.
    let retire = f.db.claim_effect().unwrap().unwrap();
    f.db.observe_effect(
        &retire.id,
        retire.fence,
        &EffectOutcome::NotApplied {
            receipt: "still_not_applied".into(),
        },
    )
    .unwrap();
    // Retrying with the SAME command id replays the stored verdict: the effect
    // reset does not fire again, so the effect stays `failed`.
    f.db.retry_cleanup("retry_cmd", &id).unwrap();
    let state: String =
        f.db.effect(&retire.id)
            .map(|e| format!("{:?}", e.state))
            .unwrap();
    assert_eq!(
        state, "Failed",
        "a replayed retry leaves the failed effect unchanged"
    );
}

/// Scenario "The prune rolls back with the failing command": a receipt write
/// that fails aborts the deciding command's transaction, so the decision is
/// not recorded and no receipt row survives.
#[test]
fn native_decision_prune_rolls_back_with_the_failing_command() {
    let mut f = Fixture::new();
    let sql = f.sql();
    seed_decisions(&sql, 501);
    let before = count_decisions(&sql);
    // Fail the receipt write AFTER the prune: the transaction rolls back whole.
    sql.execute_batch(
        "CREATE TRIGGER refuse_receipt BEFORE INSERT ON retention_prune_receipts \
         BEGIN SELECT RAISE(ABORT,'fixture rollback'); END;",
    )
    .unwrap();
    let engagement =
        f.db.admit(
            &proof(&request("rollback", "rollback agent", &f.pool, 100)),
            1000,
        )
        .unwrap();
    assert!(f.db.reject("rollback_cmd", &engagement.id).is_err());
    assert_eq!(
        count_decisions(&sql),
        before,
        "the decision count is unchanged"
    );
    assert_eq!(
        count_receipts(&sql),
        0,
        "no receipt records an uncommitted prune"
    );
    let oldest: i64 = sql
        .query_row("SELECT COUNT(*) FROM decisions WHERE id='cmd_0'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(
        oldest, 1,
        "the previously-oldest surviving command id is present"
    );
}

/// Scenario "The receipt is itself bounded": the receipt writer trims the
/// shared table to 100 rows in the same step it inserts.
#[test]
fn native_decision_prune_receipt_records_what_left() {
    let mut f = Fixture::new();
    let sql = f.sql();
    seed_receipts(&sql, 101);
    seed_decisions(&sql, 501);
    f.decide("trim_trigger", "trigger agent");
    assert!(
        count_receipts(&sql) <= 100,
        "the receipt table holds at most 100 rows"
    );
    let decisions_receipt: i64 = sql
        .query_row(
            "SELECT COUNT(*) FROM retention_prune_receipts WHERE phase='decisions'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        decisions_receipt, 1,
        "one decisions-phase receipt records the prune"
    );
}
