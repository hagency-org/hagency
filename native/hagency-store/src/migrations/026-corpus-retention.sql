-- Slice 1 (ADR-125): the admitted-message corpus gains its bound. Pruned rows
-- are archived, never dropped (retained "archive, do not lose",
-- backend-v2.js:3388). The archive is itself bounded to the same ceiling and
-- pruned oldest-first by the same sweep tick (the archive window is a named
-- product decision in the ADR).
--
-- The archive carries the ingress identity (engagement_id, source_key)
-- because matrix_ingress_events is keyed that way (012) and the replay reads
-- look rows up by the pair; the UNIQUE is on the pair, and every archive read
-- is engagement-scoped. `wake` is carried because the receipt caller needs it
-- on an archived hit.
--
-- Every statement is CREATE ... IF NOT EXISTS: the ceiling-alerts upgrade
-- fixture rewinds a live database to 24 and reopens, so 026 replays over a
-- database that already carries its objects. 026 only creates; nothing is
-- altered, so replay is a no-op (the 024 idempotent-replay pattern).
CREATE TABLE IF NOT EXISTS retained_message_archive (
  sequence          INTEGER PRIMARY KEY,
  engagement_id     TEXT,
  source_key        TEXT NOT NULL,
  scope_digest      TEXT,
  digest            TEXT NOT NULL,
  config            TEXT NOT NULL CHECK(json_valid(config)),
  source_session_id TEXT,
  wake              INTEGER NOT NULL CHECK(wake IN (0,1)),
  pruned_at_ms      INTEGER NOT NULL,
  UNIQUE(engagement_id, source_key)
) STRICT;
CREATE INDEX IF NOT EXISTS archive_ingress_source ON retained_message_archive(engagement_id,source_key);
CREATE INDEX IF NOT EXISTS archive_pruned_at      ON retained_message_archive(pruned_at_ms);

-- Pin-probe indexes: every NOT EXISTS in the pin predicate must be an index
-- seek, not a per-candidate table scan. The seven names collide with none
-- (the PKs and existing partials do not serve the message-first probes).
CREATE INDEX IF NOT EXISTS session_inputs_message         ON session_inputs(message_sequence);
CREATE INDEX IF NOT EXISTS dispatch_inputs_message        ON dispatch_inputs(message_sequence);
CREATE INDEX IF NOT EXISTS task_intents_root              ON task_intents(root_sequence);
CREATE INDEX IF NOT EXISTS task_inputs_message            ON task_inputs(message_sequence);
CREATE INDEX IF NOT EXISTS ingress_event_message          ON matrix_ingress_events(message_sequence);
CREATE INDEX IF NOT EXISTS matrix_attachment_message      ON matrix_attachments(message_sequence);
CREATE INDEX IF NOT EXISTS attachment_visibility_message  ON session_attachment_visibility(message_sequence);

-- The ONE retention receipt table (tick contract §3.1): one row per phase
-- per tick, written inside that phase's own transaction when the phase did
-- work (pruned>0 or remaining>0), trimmed to RETENTION_RECEIPT_LIMIT=100 by
-- the same writer. Later retention migrations repeat this CREATE IF NOT EXISTS
-- shape verbatim.
CREATE TABLE IF NOT EXISTS retention_prune_receipts (
  sequence    INTEGER PRIMARY KEY AUTOINCREMENT,
  phase       TEXT    NOT NULL CHECK(phase IN ('messages','peer','execution','engagements','decisions')),
  pruned      INTEGER NOT NULL CHECK(pruned >= 0),
  oldest_ref  TEXT    NOT NULL CHECK(length(oldest_ref) <= 256),
  newest_ref  TEXT    NOT NULL CHECK(length(newest_ref) <= 256),
  remaining   INTEGER NOT NULL CHECK(remaining >= 0),
  elapsed_ms  INTEGER NOT NULL CHECK(elapsed_ms >= 0),
  at_ms       INTEGER NOT NULL
) STRICT;
CREATE INDEX IF NOT EXISTS preceipt_at ON retention_prune_receipts(at_ms);
