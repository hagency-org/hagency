-- Operator display-state transitions on ceiling alerts (ADR-124 amendment):
-- ONE legal-transition map, owned by the store, served to every consumer.
-- Four states (the brief-24 §2 subset): open, acknowledged, resolved
-- (terminal), suppressed — `assigned` is not a state the ceiling alert can
-- honestly carry (it would need an assignee column and the retained
-- agent-token authority, which the native boundary does not have). A
-- transition is display state only — it never enforces, never touches
-- engagements, leases, admission or retries. Idempotent: recovery tests
-- rewind user_version and replay this migration.
ALTER TABLE ceiling_alerts ADD COLUMN status TEXT NOT NULL DEFAULT 'open'
  CHECK(status IN ('open','acknowledged','resolved','suppressed'));
-- Operator note: free text, bounded like the retained `addNote`
-- (normalizeText 2048, alert-store.js:446-447). Never private values.
ALTER TABLE ceiling_alerts ADD COLUMN note TEXT CHECK(note IS NULL OR length(note) <= 2048);
-- Who transitioned last and when — display provenance; `resolved_by` stays
-- the sweep's own column exactly as migration 024 defined it.
ALTER TABLE ceiling_alerts ADD COLUMN transitioned_at_ms INTEGER;
ALTER TABLE ceiling_alerts ADD COLUMN transitioned_by TEXT CHECK(transitioned_by IS NULL OR length(transitioned_by) <= 128);
-- Backfill: a row with resolved_at_ms set is resolved; every other row is
-- open by construction (the sweep was the only writer before this migration
-- and wrote exactly those two shapes).
UPDATE ceiling_alerts SET status='resolved' WHERE resolved_at_ms IS NOT NULL;
