-- ADR-182 decision 3: an unproven cleanup fences the agent, durably. One row
-- per fence, written by the driver before it drops the in-memory owner and
-- naming the attempt (dispatch, fence) whose custody was not proven stopped;
-- `reason` is the 038 CHECK's two words. While a row of an engagement has no
-- `cleared_at`, the host claim and both selectors return no work for it. Only
-- the operator's resolution of that dispatch (`recover_dispatch`,
-- `resolve_stopped_dispatch`, `continue_stopped_dispatch`) sets `cleared_at`
-- and `cleared_by`; nothing automatic does. The partial index serves the
-- one hot read, "is this engagement fenced". Recovery fixtures rewind
-- user_version below 38 only alongside dropping this table.
CREATE TABLE agent_fences(id INTEGER PRIMARY KEY AUTOINCREMENT, engagement_id TEXT NOT NULL REFERENCES engagements(id), dispatch_id TEXT NOT NULL REFERENCES runner_dispatches(id), fence INTEGER NOT NULL, reason TEXT NOT NULL CHECK(reason IN ('cleanup_unproven','cleanup_unknown')), created_at INTEGER NOT NULL, cleared_at INTEGER, cleared_by TEXT) STRICT;
CREATE INDEX agent_fences_open ON agent_fences(engagement_id) WHERE cleared_at IS NULL;
