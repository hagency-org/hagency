//! Phase 4, `engagements`, of the retention sweep tick (ADR-095 Slice 6):
//! the ended-engagement record bound. One `Immediate` transaction per batch;
//! the candidate predicate (terminal + no custody pin P1–P7) is the safety
//! mechanism, the FK refusal is only the no-orphan net.
use super::DomainRepository;
use crate::Error;
use hagency_core::JSON_SAFE_MAX;
use rusqlite::{TransactionBehavior, params};
use serde::Serialize;
use serde_json::json;

/// The retained record cap (`lib/engagement-store.js:56`): the newest
/// `ENDED_LIMIT` terminal engagements survive; older ones are pruned
/// oldest-first by `rowid` (store-assigned, monotonic, re-admission-correct).
pub const ENDED_LIMIT: u64 = 500;
/// Receipt trim bound (tick contract §3.1), applied by the same writer.
const RETENTION_RECEIPT_LIMIT: u64 = 100;

/// Counters one engagements-phase pass produced — a sibling of
/// `CorpusSweepOutcome`. `elapsed_ms` follows the round-4 rule: the outcome
/// sample is taken after the commit so the batch-reduction rule sees the
/// commit's own cost too.
#[derive(Debug, Default, Clone, Serialize, PartialEq, Eq)]
pub struct EngagementPruneOutcome {
    pub pruned: u64,
    pub remaining: u64,
    pub elapsed_ms: u64,
}

/// The one read for the bound: terminal rows against the ceiling (D-5).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct EngagementRetentionStatus {
    pub terminal_rows: u64,
    pub ceiling: u64,
    pub over_by: u64,
}

fn bounded(value: u64) -> Result<u64, Error> {
    if value > JSON_SAFE_MAX {
        return Err(Error::Capacity);
    }
    Ok(value)
}

/// One `(table, scoping SQL)` step of the child-first tier walk. The scoping
/// SQL yields the rows of `table` that belong to the batch's engagements.
/// A single ordered pass is the fixed point: the FK graph between these
/// tables is acyclic by tier, so child-first order removes every removable
/// row in one pass and anything left pins its parent to the next tick.
/// One step of the child-first tier walk: (table, the child's FK column,
/// the parent-key scope). The scope yields the PARENT key values reachable
/// from this batch's engagements — every SELECT names its table alias (no
/// `ambiguous column name: id`) — and the delete matches the child's own
/// FK column, never its rowid (an INTEGER rowid never equals a TEXT parent
/// id, so `rowid IN (SELECT id ...)` silently deleted nothing).
const CASCADE: &[(&str, &str, &str)] = &[
    (
        "runner_outputs",
        "dispatch_id",
        "SELECT d.id FROM runner_dispatches d JOIN runner_sessions s ON s.id=d.session_id JOIN engagement_prune_batch b ON b.engagement_id=s.engagement_id",
    ),
    (
        "runner_attempts",
        "dispatch_id",
        "SELECT d.id FROM runner_dispatches d JOIN runner_sessions s ON s.id=d.session_id JOIN engagement_prune_batch b ON b.engagement_id=s.engagement_id",
    ),
    (
        "task_comments",
        "task_id",
        "SELECT t.id FROM canonical_tasks t JOIN runner_sessions s ON s.id=t.session_id JOIN engagement_prune_batch b ON b.engagement_id=s.engagement_id",
    ),
    (
        "graph_dependencies",
        "graph_id",
        "SELECT g.id FROM task_graphs g JOIN runner_sessions s ON s.id=g.creator_session_id JOIN engagement_prune_batch b ON b.engagement_id=s.engagement_id",
    ),
    (
        "task_input_receipts",
        "task_id",
        "SELECT t.id FROM canonical_tasks t JOIN runner_sessions s ON s.id=t.session_id JOIN engagement_prune_batch b ON b.engagement_id=s.engagement_id",
    ),
    (
        "conversation_operations",
        "dispatch_id",
        "SELECT d.id FROM runner_dispatches d JOIN runner_sessions s ON s.id=d.session_id JOIN engagement_prune_batch b ON b.engagement_id=s.engagement_id",
    ),
    (
        "dispatch_inputs",
        "dispatch_id",
        "SELECT d.id FROM runner_dispatches d JOIN runner_sessions s ON s.id=d.session_id JOIN engagement_prune_batch b ON b.engagement_id=s.engagement_id",
    ),
    (
        "peer_dispatch_inputs",
        "dispatch_id",
        "SELECT d.id FROM runner_dispatches d JOIN runner_sessions s ON s.id=d.session_id JOIN engagement_prune_batch b ON b.engagement_id=s.engagement_id",
    ),
    (
        "peer_session_inputs",
        "session_id",
        "SELECT s.id FROM runner_sessions s JOIN engagement_prune_batch b ON b.engagement_id=s.engagement_id",
    ),
    (
        "session_inputs",
        "session_id",
        "SELECT s.id FROM runner_sessions s JOIN engagement_prune_batch b ON b.engagement_id=s.engagement_id",
    ),
    (
        "task_inputs",
        "task_id",
        "SELECT t.id FROM canonical_tasks t JOIN runner_sessions s ON s.id=t.session_id JOIN engagement_prune_batch b ON b.engagement_id=s.engagement_id",
    ),
    (
        "dispatch_stops",
        "dispatch_id",
        "SELECT d.id FROM runner_dispatches d JOIN runner_sessions s ON s.id=d.session_id JOIN engagement_prune_batch b ON b.engagement_id=s.engagement_id",
    ),
    (
        "dispatch_resources",
        "dispatch_id",
        "SELECT d.id FROM runner_dispatches d JOIN runner_sessions s ON s.id=d.session_id JOIN engagement_prune_batch b ON b.engagement_id=s.engagement_id",
    ),
    (
        "dispatch_attachment_windows",
        "dispatch_id",
        "SELECT d.id FROM runner_dispatches d JOIN runner_sessions s ON s.id=d.session_id JOIN engagement_prune_batch b ON b.engagement_id=s.engagement_id",
    ),
    (
        "resource_leases",
        "dispatch_id",
        "SELECT d.id FROM runner_dispatches d JOIN runner_sessions s ON s.id=d.session_id JOIN engagement_prune_batch b ON b.engagement_id=s.engagement_id",
    ),
    (
        "dispatch_recovery_reports",
        "dispatch_id",
        "SELECT d.id FROM runner_dispatches d JOIN runner_sessions s ON s.id=d.session_id JOIN engagement_prune_batch b ON b.engagement_id=s.engagement_id",
    ),
    (
        "dispatch_recoveries",
        "original_id",
        "SELECT d.id FROM runner_dispatches d JOIN runner_sessions s ON s.id=d.session_id JOIN engagement_prune_batch b ON b.engagement_id=s.engagement_id",
    ),
    (
        "dispatch_recoveries",
        "replacement_id",
        "SELECT d.id FROM runner_dispatches d JOIN runner_sessions s ON s.id=d.session_id JOIN engagement_prune_batch b ON b.engagement_id=s.engagement_id",
    ),
    (
        "graph_commands",
        "dispatch_id",
        "SELECT d.id FROM runner_dispatches d JOIN runner_sessions s ON s.id=d.session_id JOIN engagement_prune_batch b ON b.engagement_id=s.engagement_id",
    ),
    (
        "file_deliveries",
        "dispatch_id",
        "SELECT d.id FROM runner_dispatches d JOIN runner_sessions s ON s.id=d.session_id JOIN engagement_prune_batch b ON b.engagement_id=s.engagement_id",
    ),
    (
        "file_uploads",
        "dispatch_id",
        "SELECT d.id FROM runner_dispatches d JOIN runner_sessions s ON s.id=d.session_id JOIN engagement_prune_batch b ON b.engagement_id=s.engagement_id",
    ),
    (
        "usage_receipts",
        "source_id",
        "SELECT u.id FROM usage_sources u JOIN engagement_prune_batch b ON b.engagement_id=u.engagement_id",
    ),
    (
        "approval_responses",
        "context_id",
        "SELECT c.id FROM approval_contexts c JOIN engagement_prune_batch b ON b.engagement_id=c.engagement_id",
    ),
    (
        "approval_verdict_receipts",
        "request_id",
        "SELECT o.id FROM owner_approvals o JOIN approval_contexts c ON c.id=o.context_id JOIN engagement_prune_batch b ON b.engagement_id=c.engagement_id",
    ),
    (
        "notice_send_inspections",
        "notice_id",
        "SELECT n.id FROM task_notices n JOIN canonical_tasks t ON t.id=n.task_id JOIN runner_sessions s ON s.id=t.session_id JOIN engagement_prune_batch b ON b.engagement_id=s.engagement_id",
    ),
    (
        "session_attachment_visibility",
        "session_id",
        "SELECT s.id FROM runner_sessions s JOIN engagement_prune_batch b ON b.engagement_id=s.engagement_id",
    ),
    (
        "owned_task_completions",
        "dispatch_id",
        "SELECT d.id FROM runner_dispatches d JOIN runner_sessions s ON s.id=d.session_id JOIN engagement_prune_batch b ON b.engagement_id=s.engagement_id",
    ),
    (
        "task_operation_receipts",
        "dispatch_id",
        "SELECT d.id FROM runner_dispatches d JOIN runner_sessions s ON s.id=d.session_id JOIN engagement_prune_batch b ON b.engagement_id=s.engagement_id",
    ),
    (
        "final_reply_inspections",
        "reply_id",
        "SELECT f.id FROM final_replies f JOIN runner_sessions s ON s.id=f.session_id JOIN engagement_prune_batch b ON b.engagement_id=s.engagement_id",
    ),
    (
        "final_reply_calls",
        "reply_id",
        "SELECT f.id FROM final_replies f JOIN runner_sessions s ON s.id=f.session_id JOIN engagement_prune_batch b ON b.engagement_id=s.engagement_id",
    ),
    (
        "final_replies",
        "session_id",
        "SELECT s.id FROM runner_sessions s JOIN engagement_prune_batch b ON b.engagement_id=s.engagement_id",
    ),
    (
        "task_outbox",
        "task_id",
        "SELECT t.id FROM canonical_tasks t JOIN runner_sessions s ON s.id=t.session_id JOIN engagement_prune_batch b ON b.engagement_id=s.engagement_id",
    ),
    (
        "task_notices",
        "task_id",
        "SELECT t.id FROM canonical_tasks t JOIN runner_sessions s ON s.id=t.session_id JOIN engagement_prune_batch b ON b.engagement_id=s.engagement_id",
    ),
    (
        "verified_task_requests",
        "source_session_id",
        "SELECT s.id FROM runner_sessions s JOIN engagement_prune_batch b ON b.engagement_id=s.engagement_id",
    ),
    (
        "task_intents",
        "session_id",
        "SELECT s.id FROM runner_sessions s JOIN engagement_prune_batch b ON b.engagement_id=s.engagement_id",
    ),
    (
        "matrix_attachments",
        "engagement_id",
        "SELECT engagement_id FROM engagement_prune_batch",
    ),
    (
        "matrix_ingress_events",
        "engagement_id",
        "SELECT engagement_id FROM engagement_prune_batch",
    ),
    (
        "owner_approvals",
        "context_id",
        "SELECT c.id FROM approval_contexts c JOIN engagement_prune_batch b ON b.engagement_id=c.engagement_id",
    ),
    (
        "retained_message_archive",
        "engagement_id",
        "SELECT engagement_id FROM engagement_prune_batch",
    ),
    (
        "internal_participants",
        "engagement_id",
        "SELECT engagement_id FROM engagement_prune_batch",
    ),
    (
        "graph_nodes",
        "graph_id",
        "SELECT g.id FROM task_graphs g JOIN runner_sessions s ON s.id=g.creator_session_id JOIN engagement_prune_batch b ON b.engagement_id=s.engagement_id",
    ),
    (
        "peer_messages",
        "conversation_id",
        "SELECT c.id FROM internal_conversations c JOIN runner_sessions s ON s.id=c.creator_session_id JOIN engagement_prune_batch b ON b.engagement_id=s.engagement_id",
    ),
    (
        "approval_contexts",
        "engagement_id",
        "SELECT engagement_id FROM engagement_prune_batch",
    ),
    (
        "usage_sources",
        "engagement_id",
        "SELECT engagement_id FROM engagement_prune_batch",
    ),
    (
        "runner_dispatches",
        "session_id",
        "SELECT s.id FROM runner_sessions s JOIN engagement_prune_batch b ON b.engagement_id=s.engagement_id",
    ),
    (
        "task_graphs",
        "creator_session_id",
        "SELECT s.id FROM runner_sessions s JOIN engagement_prune_batch b ON b.engagement_id=s.engagement_id",
    ),
    (
        "internal_conversations",
        "creator_session_id",
        "SELECT s.id FROM runner_sessions s JOIN engagement_prune_batch b ON b.engagement_id=s.engagement_id",
    ),
    (
        "usage_periods",
        "engagement_id",
        "SELECT engagement_id FROM engagement_prune_batch",
    ),
    (
        "approval_grants",
        "engagement_id",
        "SELECT engagement_id FROM engagement_prune_batch",
    ),
    (
        "canonical_tasks",
        "session_id",
        "SELECT s.id FROM runner_sessions s JOIN engagement_prune_batch b ON b.engagement_id=s.engagement_id",
    ),
    (
        "matrix_session_routes",
        "session_id",
        "SELECT s.id FROM runner_sessions s JOIN engagement_prune_batch b ON b.engagement_id=s.engagement_id",
    ),
    (
        "matrix_transports",
        "engagement_id",
        "SELECT engagement_id FROM engagement_prune_batch",
    ),
    (
        "matrix_room_memberships",
        "engagement_id",
        "SELECT engagement_id FROM engagement_prune_batch",
    ),
    (
        "approval_bindings",
        "engagement_id",
        "SELECT engagement_id FROM engagement_prune_batch",
    ),
    (
        "effects",
        "engagement_id",
        "SELECT engagement_id FROM engagement_prune_batch",
    ),
    (
        "runner_sessions",
        "engagement_id",
        "SELECT engagement_id FROM engagement_prune_batch",
    ),
    (
        "engagement_ends",
        "engagement_id",
        "SELECT engagement_id FROM engagement_prune_batch",
    ),
];

impl DomainRepository {
    /// Phase 4 of the retention tick: the ended-engagement bound. One
    /// `Immediate` transaction; candidates are terminal engagements past the
    /// `rowid` window with no custody pin (P1–P7); the whole reachable set
    /// goes child-first in the same transaction. A deferred engagement is
    /// `remaining > 0`, never a work refusal.
    pub fn sweep_engagements(
        &mut self,
        now: u64,
        ceiling: u64,
        batch: u64,
    ) -> Result<EngagementPruneOutcome, Error> {
        let ceiling = bounded(ceiling)?;
        let batch = bounded(batch)?;
        let started = std::time::Instant::now();
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let terminal: u64 = tx.query_row(
            "SELECT COUNT(*) FROM engagements WHERE state IN ('rejected','revoked','failed')",
            [],
            |r| r.get(0),
        )?;
        let remaining = terminal.saturating_sub(ceiling);
        let mut pruned: u64 = 0;
        let mut payload_ids = Vec::new();
        let mut child_counts = serde_json::Map::new();
        if remaining > 0 {
            // Count-only, cap `ceiling`, ordered by rowid ASC: a candidate is
            // a terminal engagement NOT among the newest `ceiling` terminal
            // rows by rowid. The rank form (NOT IN newest-N) — not
            // `rowid <= max-ceiling` arithmetic — is what "count-only"
            // requires: rowid gaps from SQLite's rowid reuse cannot shrink
            // or widen the kept window.
            let candidates: Vec<(String, String, Option<u64>)> = {
                let mut statement = tx.prepare(
                    "SELECT e.id,e.state,(SELECT ended_at FROM engagement_ends k \
                     WHERE k.engagement_id=e.id) FROM engagements e \
                     WHERE e.state IN ('rejected','revoked','failed') \
                     AND e.rowid NOT IN (SELECT rowid FROM engagements \
                        WHERE state IN ('rejected','revoked','failed') \
                        ORDER BY rowid DESC LIMIT ?1) \
                     AND NOT EXISTS (SELECT 1 FROM effects f WHERE f.engagement_id=e.id \
                        AND (f.state IN ('pending','started','uncertain') \
                             OR (f.kind='retire' AND f.state='failed'))) \
                     AND NOT EXISTS (SELECT 1 FROM runner_dispatches d \
                        JOIN runner_sessions s ON s.id=d.session_id \
                        WHERE s.engagement_id=e.id \
                        AND (d.state IN ('leased','started','parked','outcome_unknown') \
                             OR EXISTS (SELECT 1 FROM dispatch_stops st \
                                WHERE st.dispatch_id=d.id AND st.settled_at IS NULL))) \
                     AND NOT EXISTS (SELECT 1 FROM owner_approvals o \
                        JOIN approval_contexts c ON c.id=o.context_id \
                        WHERE c.engagement_id=e.id \
                        AND o.state IN ('pending','decided','applying','uncertain')) \
                     ORDER BY e.rowid ASC LIMIT ?2",
                )?;
                statement
                    .query_map(params![ceiling, batch], |r| {
                        Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                    })?
                    .collect::<Result<_, _>>()?
            };
            if !candidates.is_empty() {
                tx.execute_batch(
                    "CREATE TEMP TABLE IF NOT EXISTS engagement_prune_batch\
                     (engagement_id TEXT PRIMARY KEY) WITHOUT ROWID;",
                )?;
                tx.execute("DELETE FROM engagement_prune_batch", [])?;
                for (id, _, _) in &candidates {
                    tx.execute(
                        "INSERT INTO engagement_prune_batch(engagement_id) VALUES(?1)",
                        [id],
                    )?;
                }
                for (id, state, ended_at) in &candidates {
                    payload_ids.push(json!({
                        "id": id,
                        "state": state,
                        "ended_at": ended_at,
                    }));
                }
                // The child-first tier walk: one ordered pass is the fixed
                // point. The FK refusal (foreign_keys=ON) is the net that
                // catches a mis-ordered delete and guarantees no orphan; a
                // refused statement (a cross-engagement child still holds
                // the row — e.g. another engagement's task created by this
                // session) defers that parent to the next tick instead of
                // failing the phase: statement-level failure never aborts
                // the transaction, so the walk continues.
                let mut deferred = false;
                for (table, column, scope) in CASCADE {
                    match tx.execute(
                        &format!("DELETE FROM {table} WHERE {column} IN ({scope})"),
                        [],
                    ) {
                        Ok(removed) => {
                            if removed > 0 {
                                child_counts.insert(
                                    (*table).to_owned(),
                                    serde_json::Value::from(
                                        u64::try_from(removed).unwrap_or(u64::MAX),
                                    ),
                                );
                            }
                        }
                        Err(rusqlite::Error::SqliteFailure(error, _))
                            if error.code == rusqlite::ErrorCode::ConstraintViolation =>
                        {
                            deferred = true;
                        }
                        Err(error) => return Err(error.into()),
                    }
                }
                let mut pruned_now: u64 = 0;
                for (id, _, _) in &candidates {
                    match tx.execute("DELETE FROM engagements WHERE id=?1", [id]) {
                        Ok(_) => pruned_now += 1,
                        Err(rusqlite::Error::SqliteFailure(error, _))
                            if error.code == rusqlite::ErrorCode::ConstraintViolation =>
                        {
                            // A child this phase's own walk could not clear
                            // pins the engagement: it defers, never a loss.
                            deferred = true;
                        }
                        Err(error) => return Err(error.into()),
                    }
                }
                pruned = pruned_now;
                let _ = (
                    &deferred,
                    tx.execute("DELETE FROM engagement_prune_batch", []),
                );
            }
        }
        // One receipt row per tick when the phase did work, trimmed by the
        // same writer to the contract's limit. The receipt stays INSIDE the
        // phase's transaction: a receipt exists iff the phase committed.
        let receipt_elapsed = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX / 2);
        if pruned > 0 || remaining > 0 {
            tx.execute(
                "INSERT INTO retention_prune_receipts\
                 (phase,pruned,oldest_ref,newest_ref,remaining,elapsed_ms,at_ms,payload) \
                 VALUES('engagements',?1,?2,?3,?4,?5,?6,?7)",
                params![
                    pruned,
                    payload_ids
                        .first()
                        .map(|v| v["id"].as_str().unwrap_or_default().to_owned())
                        .unwrap_or_default(),
                    payload_ids
                        .last()
                        .map(|v| v["id"].as_str().unwrap_or_default().to_owned())
                        .unwrap_or_default(),
                    remaining,
                    receipt_elapsed,
                    bounded(now)?,
                    serde_json::to_string(&json!({
                        "engagements": payload_ids,
                        "children": child_counts,
                    }))?,
                ],
            )?;
            tx.execute(
                "DELETE FROM retention_prune_receipts WHERE sequence NOT IN (\
                 SELECT sequence FROM retention_prune_receipts \
                 ORDER BY sequence DESC LIMIT ?1)",
                [RETENTION_RECEIPT_LIMIT],
            )?;
        }
        tx.commit()?;
        Ok(EngagementPruneOutcome {
            pruned,
            remaining,
            elapsed_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX / 2),
        })
    }

    /// The one read for the engagements bound (D-5): terminal rows against
    /// the ceiling.
    pub fn engagement_retention_status(
        &self,
        ceiling: u64,
    ) -> Result<EngagementRetentionStatus, Error> {
        let ceiling = bounded(ceiling)?;
        let terminal_rows: u64 = self.db.query_row(
            "SELECT COUNT(*) FROM engagements WHERE state IN ('rejected','revoked','failed')",
            [],
            |r| r.get(0),
        )?;
        Ok(EngagementRetentionStatus {
            terminal_rows,
            ceiling,
            over_by: terminal_rows.saturating_sub(ceiling),
        })
    }
}
