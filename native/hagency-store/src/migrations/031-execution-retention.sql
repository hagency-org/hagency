-- Slice 2 (ADR-053/ADR-031 amendments, ADR-125 tick contract): the
-- per-dispatch execution corpus gains the indexes its retention phase prunes
-- by. Phase 3, `execution`, of the retention sweep tick deletes settled
-- dispatches' output and receipt-family rows — runner_outputs, the receipt
-- family (task_operation_receipts, conversation_operations, usage_receipts,
-- graph_commands, final_reply_calls) — keyed off the candidate dispatch
-- predicate `state IN ('completed','superseded') AND capability_hash IS NULL`,
-- and keeps the newest 1 accepted runner_outputs row per (dispatch_id, fence)
-- (the D-8 residue). runner_attempts is never pruned (D-7) and this migration
-- adds no delete; the delete statements live in the store's execution phase.
--
-- Indexes only. The receipt table itself is the tick contract's §3.1 DDL,
-- created by migration 026 — no second CREATE here.
--
-- Every statement is CREATE INDEX IF NOT EXISTS: an upgrade fixture rewinds a
-- live database to the previous head and reopens, so 029 replays over a
-- database that already carries its objects. 029 only creates; nothing is
-- altered, so replay is a no-op (the 024 idempotent-replay pattern).

-- The candidate predicate: runner_dispatches has dispatch_queue
-- (state,not_before,id) and a partial dispatch_expiry, but no index leading
-- with state AND capability_hash — the predicate's two conjuncts.
CREATE INDEX IF NOT EXISTS dispatch_settled
  ON runner_dispatches(state, capability_hash, id);

-- The per-dispatch drain: one delete per candidate dispatch over each of its
-- evidence tables, and the D-8 residue lookup (newest accepted row per fence).
CREATE INDEX IF NOT EXISTS runner_outputs_dispatch
  ON runner_outputs(dispatch_id, fence, sequence);
CREATE INDEX IF NOT EXISTS runner_attempts_dispatch
  ON runner_attempts(dispatch_id, fence);
CREATE INDEX IF NOT EXISTS task_receipts_dispatch
  ON task_operation_receipts(dispatch_id);
CREATE INDEX IF NOT EXISTS graph_commands_dispatch
  ON graph_commands(dispatch_id);
CREATE INDEX IF NOT EXISTS conv_ops_dispatch
  ON conversation_operations(dispatch_id);
-- final_reply_calls drains by its dispatch_id PK prefix; usage_receipts drains
-- through usage_sources, whose UNIQUE(dispatch_id,fence) already serves the
-- lookup — neither table gains an index here.
