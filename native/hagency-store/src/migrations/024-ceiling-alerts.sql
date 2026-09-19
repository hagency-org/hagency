-- Ceiling overrun alarms (ADR-124): one row per resource dedupe key.
-- Diagnostic only: presence never blocks or permits admission, and the
-- sweep's resolve is a display transition, never enforcement (no revocation,
-- no retry, no lease authority). Idempotent: recovery tests rewind
-- user_version and replay this migration.
CREATE TABLE IF NOT EXISTS ceiling_alerts (
  dedupe_key TEXT PRIMARY KEY,
  resource_id TEXT NOT NULL,
  summary TEXT NOT NULL,
  detail TEXT NOT NULL CHECK(length(detail) <= 4096),
  runbook TEXT NOT NULL,
  impact TEXT NOT NULL,
  recovery_condition TEXT NOT NULL,
  occurrences INTEGER NOT NULL CHECK(occurrences >= 1),
  first_seen_ms INTEGER NOT NULL,
  last_seen_ms INTEGER NOT NULL,
  resolved_at_ms INTEGER,
  resolved_by TEXT
) STRICT;
