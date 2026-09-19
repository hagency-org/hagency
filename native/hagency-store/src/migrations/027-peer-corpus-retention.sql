-- Slice 7 (ADR-125): the agent-to-agent peer corpus gains its bound. Pruned
-- rows are deleted with their child projections, children first (RESTRICT; no
-- ON DELETE anywhere); a bounded identity store keeps the idempotency answer
-- for the pruned key (the send's Conflict/replayed verdict survives prune).
--
-- Replay-safe: every fixture that rewinds below this head and re-opens
-- replays this migration on a live database, so every CREATE below is
-- IF NOT EXISTS (the 024 pattern). Nothing is altered.
CREATE TABLE IF NOT EXISTS retained_peer_index (
  source_key   TEXT PRIMARY KEY,          -- the send's idempotency key
  digest       TEXT NOT NULL,             -- kept so a pruned key can still answer Conflict
  sequence     INTEGER NOT NULL,          -- the pruned row's sequence, returned as `replayed`
  pruned_at_ms INTEGER NOT NULL
) STRICT;
CREATE INDEX IF NOT EXISTS peer_index_pruned_at ON retained_peer_index(pruned_at_ms);

-- Pin-probe indexes: every EXISTS in the peer pin predicate must be an index
-- seek, not a per-candidate scan. Both child PKs lead with the other column
-- (007), so neither serves a message-first probe; `peer_pending_inbox` is
-- partial and serves only the inbox page. `graph_nodes.message_sequence` is
-- UNIQUE and already carries its implicit index.
CREATE INDEX IF NOT EXISTS peer_session_inputs_message  ON peer_session_inputs(message_sequence);
CREATE INDEX IF NOT EXISTS peer_dispatch_inputs_message ON peer_dispatch_inputs(message_sequence);

-- The ONE retention receipt table (tick contract §3.1) is created by
-- migration 026, which landed first (Slice 1) and whose CHECK already
-- admits 'peer' in the phase enum — so this migration adds ONLY the peer
-- identity store and the two pin-probe indexes above. Re-creating the
-- receipt table here (even IF NOT EXISTS) would be a redundant no-op in
-- every head order where 026 precedes 027, and the docs boundary licenses
-- "indexes only" for 027. The receipt DDL stays in exactly one place.

