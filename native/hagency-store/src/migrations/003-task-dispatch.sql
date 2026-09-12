CREATE TABLE runner_sessions (
 id TEXT PRIMARY KEY, engagement_id TEXT NOT NULL REFERENCES engagements(id),
 binding TEXT NOT NULL CHECK(json_valid(binding)), quarantined INTEGER NOT NULL DEFAULT 0 CHECK(quarantined IN (0,1))
) STRICT;
CREATE TABLE canonical_tasks (
 id TEXT PRIMARY KEY, session_id TEXT NOT NULL REFERENCES runner_sessions(id),
 creator_session_id TEXT REFERENCES runner_sessions(id), config TEXT NOT NULL CHECK(json_valid(config))
) STRICT;
CREATE INDEX tasks_session ON canonical_tasks(session_id,id);
CREATE INDEX tasks_creator ON canonical_tasks(creator_session_id,id);
CREATE TABLE workspace_resources (id TEXT PRIMARY KEY, dirty INTEGER NOT NULL DEFAULT 0 CHECK(dirty IN (0,1))) STRICT;
CREATE TABLE runner_dispatches (
 id TEXT PRIMARY KEY, session_id TEXT NOT NULL REFERENCES runner_sessions(id),
 task_id TEXT REFERENCES canonical_tasks(id), input TEXT NOT NULL CHECK(json_valid(input)), digest TEXT NOT NULL,
 state TEXT NOT NULL CHECK(state IN ('queued','leased','started','parked','completed','outcome_unknown','superseded')),
 fence INTEGER NOT NULL DEFAULT 0, runner_id TEXT, capability_hash TEXT,
 lease_until INTEGER, capability_until INTEGER, not_before INTEGER NOT NULL DEFAULT 0
) STRICT;
CREATE UNIQUE INDEX one_live_session ON runner_dispatches(session_id) WHERE state IN ('leased','started','parked');
CREATE INDEX dispatch_queue ON runner_dispatches(state,not_before,id);
CREATE INDEX dispatch_expiry ON runner_dispatches(lease_until) WHERE state IN ('leased','started','parked');
CREATE TABLE dispatch_resources (
 dispatch_id TEXT NOT NULL REFERENCES runner_dispatches(id), resource_id TEXT NOT NULL REFERENCES workspace_resources(id),
 exclusive INTEGER NOT NULL CHECK(exclusive IN (0,1)), PRIMARY KEY(dispatch_id,resource_id)
) STRICT;
CREATE TABLE resource_leases (
 resource_id TEXT NOT NULL REFERENCES workspace_resources(id), dispatch_id TEXT NOT NULL REFERENCES runner_dispatches(id),
 exclusive INTEGER NOT NULL CHECK(exclusive IN (0,1)), PRIMARY KEY(resource_id,dispatch_id)
) STRICT;
CREATE TABLE runner_attempts (
 dispatch_id TEXT NOT NULL REFERENCES runner_dispatches(id), fence INTEGER NOT NULL, runner_id TEXT NOT NULL,
 outcome TEXT NOT NULL, capability_hash TEXT NOT NULL, created_at INTEGER NOT NULL, PRIMARY KEY(dispatch_id,fence)
) STRICT;
CREATE TABLE runner_outputs (
 sequence INTEGER PRIMARY KEY AUTOINCREMENT, dispatch_id TEXT NOT NULL REFERENCES runner_dispatches(id),
 fence INTEGER NOT NULL, output TEXT NOT NULL CHECK(json_valid(output)), accepted INTEGER NOT NULL CHECK(accepted IN (0,1))
) STRICT;
CREATE TABLE task_comments (
 sequence INTEGER PRIMARY KEY AUTOINCREMENT, task_id TEXT NOT NULL REFERENCES canonical_tasks(id),
 author TEXT NOT NULL, body TEXT NOT NULL, created_at INTEGER NOT NULL
) STRICT;
CREATE INDEX comments_task ON task_comments(task_id,sequence);
CREATE TABLE task_operation_receipts (
 dispatch_id TEXT NOT NULL REFERENCES runner_dispatches(id), call_id TEXT NOT NULL,
 digest TEXT NOT NULL, response TEXT NOT NULL CHECK(json_valid(response)), PRIMARY KEY(dispatch_id,call_id)
) STRICT;
CREATE TABLE task_outbox (
 sequence INTEGER PRIMARY KEY AUTOINCREMENT, task_id TEXT NOT NULL REFERENCES canonical_tasks(id),
 kind TEXT NOT NULL, task TEXT NOT NULL CHECK(json_valid(task)), delivered INTEGER NOT NULL DEFAULT 0 CHECK(delivered IN (0,1))
) STRICT;
CREATE INDEX task_outbox_pending ON task_outbox(delivered,sequence);
CREATE TABLE dispatch_recoveries (
 original_id TEXT PRIMARY KEY REFERENCES runner_dispatches(id), replacement_id TEXT NOT NULL UNIQUE REFERENCES runner_dispatches(id),
 evidence TEXT NOT NULL, created_at INTEGER NOT NULL
) STRICT;
