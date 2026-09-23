-- ADR-181: every owned attempt leaves evidence a lost agent can be diagnosed
-- from. The per-attempt event log, one row per phase visit in the order the
-- host observed it; `detail` is a bounded JSON object of fixed keys (the
-- writer bounds it, the schema only insists it is JSON). Best-effort
-- observations: a row here authorizes nothing and never changes a verdict.
-- Retention prunes them with the dispatch (ADR-053 amendment §3 window).
CREATE TABLE runner_attempt_events(dispatch_id TEXT NOT NULL REFERENCES runner_dispatches(id), fence INTEGER NOT NULL, seq INTEGER NOT NULL, at_ms INTEGER NOT NULL, phase TEXT NOT NULL CHECK(phase IN ('claimed','spawn_started','spawn_done','initialized','turn_started','approval_requested','approval_decided','parked','resumed','stop_requested','stop_reported','settled','failed','lost')), detail TEXT NOT NULL CHECK(json_valid(detail)), PRIMARY KEY(dispatch_id,fence,seq)) STRICT;
-- The attempt row carries its clock (ADR-181 point 2): the phase timestamps
-- the retained product keeps on the dispatch row, and `terminal_reason`,
-- the `<failure>:<exit identity>:<stderr tail>` shape written once at
-- settlement or failure into the private store only (ADR-040 stands for
-- every other projection). `last_renew_at` is the lease renewal's own mark,
-- so a lease loss can say what it was judged from (point 6).
--
-- NOT idempotent by design (the 033 rule): no ADD COLUMN in this store
-- replays over an already-upgraded table; recovery fixtures rewind
-- user_version below 37 only alongside dropping these columns and the
-- event table.
ALTER TABLE runner_attempts ADD COLUMN started_at INTEGER;
ALTER TABLE runner_attempts ADD COLUMN parked_at INTEGER;
ALTER TABLE runner_attempts ADD COLUMN last_renew_at INTEGER;
ALTER TABLE runner_attempts ADD COLUMN settled_at INTEGER;
ALTER TABLE runner_attempts ADD COLUMN terminal_reason TEXT;
