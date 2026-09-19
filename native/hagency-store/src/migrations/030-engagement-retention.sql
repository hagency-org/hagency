-- Slice 6 (ADR-095 amendment): the ended-engagement record bound.
-- Registry slot 27 on this base; the file keeps the backlog ledger's number
-- 030, its number assigned by landing order (right after MA-S4, which kept 029), never the
-- ledger's pre-allocation: the strict one-by-one upgrade loop cannot skip a
-- version.
--
-- ended_at is advisory metadata on a side table (never a column on
-- engagements: an ADD COLUMN there would be replayed over an already-
-- upgraded table by every fixture that rewinds user_version — the 025
-- rule). Ordering is rowid alone, store-assigned, monotonic, re-admission-
-- correct. IF NOT EXISTS keeps replay idempotent (the 024 pattern).
-- IF NOT EXISTS keeps replay idempotent (the 024 pattern). The candidate
-- index is on state alone: SQLite cannot index the implicit rowid pseudo-
-- column, and the ordering key is used in the sweep's queries, not here.
CREATE TABLE IF NOT EXISTS engagement_ends (
  engagement_id TEXT PRIMARY KEY REFERENCES engagements(id),
  ended_at      INTEGER NOT NULL
) STRICT;
CREATE INDEX IF NOT EXISTS ended_engagement_candidates ON engagements(state);

-- The receipt payload (tick contract §3.2): the per-id terminal state and
-- child counts. The live 026 table predates the contract's payload column,
-- and an ALTER ADD COLUMN cannot replay over an upgraded table (the 025
-- rule), so the receipt table is rebuilt copy-swap (the 014 precedent).
-- Replay-safe on both shapes: the copy selects only the eight original
-- columns explicitly, so it compiles whether or not the source already
-- carries payload; rows are preserved, payloads reset to NULL on a
-- synthetic rewind replay only.
DROP TABLE IF EXISTS retention_prune_receipts_v30;
CREATE TABLE retention_prune_receipts_v30 (
  sequence    INTEGER PRIMARY KEY AUTOINCREMENT,
  phase       TEXT    NOT NULL CHECK(phase IN ('messages','peer','execution','engagements','decisions')),
  pruned      INTEGER NOT NULL CHECK(pruned >= 0),
  oldest_ref  TEXT    NOT NULL CHECK(length(oldest_ref) <= 256),
  newest_ref  TEXT    NOT NULL CHECK(length(newest_ref) <= 256),
  remaining   INTEGER NOT NULL CHECK(remaining >= 0),
  elapsed_ms  INTEGER NOT NULL CHECK(elapsed_ms >= 0),
  at_ms       INTEGER NOT NULL,
  payload     TEXT    CHECK(payload IS NULL OR json_valid(payload))
) STRICT;
INSERT INTO retention_prune_receipts_v30(sequence,phase,pruned,oldest_ref,newest_ref,remaining,elapsed_ms,at_ms)
  SELECT sequence,phase,pruned,oldest_ref,newest_ref,remaining,elapsed_ms,at_ms FROM retention_prune_receipts;
DROP TABLE retention_prune_receipts;
ALTER TABLE retention_prune_receipts_v30 RENAME TO retention_prune_receipts;
CREATE INDEX IF NOT EXISTS preceipt_at ON retention_prune_receipts(at_ms);
