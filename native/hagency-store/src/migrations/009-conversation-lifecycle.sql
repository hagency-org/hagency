ALTER TABLE internal_conversations ADD COLUMN revision INTEGER NOT NULL DEFAULT 0 CHECK(revision>=0);
UPDATE internal_conversations SET config=json_set(config,'$.revision',0);
-- Matrix uniqueness is unchanged. Internal membership uniqueness is enforced by
-- internal_participants; retired bindings must coexist with a fresh incarnation.
DROP INDEX canonical_runner_session;
CREATE UNIQUE INDEX canonical_runner_session ON runner_sessions(
 engagement_id,
 CASE WHEN json_extract(binding,'$.kind')='internal' THEN 'internal' ELSE 'matrix' END,
 CASE WHEN json_extract(binding,'$.kind')='internal' THEN id ELSE json_extract(binding,'$.room_id') END,
 COALESCE(json_extract(binding,'$.thread_root'),'')
);
CREATE TABLE conversation_operations (
 dispatch_id TEXT NOT NULL REFERENCES runner_dispatches(id),call_id TEXT NOT NULL,
 digest TEXT NOT NULL,response TEXT NOT NULL CHECK(json_valid(response)),
 PRIMARY KEY(dispatch_id,call_id)
) STRICT;
CREATE TABLE dispatch_stops (
 dispatch_id TEXT PRIMARY KEY REFERENCES runner_dispatches(id),fence INTEGER NOT NULL,
 reason TEXT NOT NULL,created_at INTEGER NOT NULL,evidence TEXT,settled_at INTEGER,
 CHECK((evidence IS NULL)=(settled_at IS NULL))
) STRICT;
CREATE INDEX pending_dispatch_stops ON dispatch_stops(dispatch_id) WHERE settled_at IS NULL;
CREATE VIEW unresolved_dispatches AS
SELECT d.id,d.session_id FROM runner_dispatches d WHERE d.state='outcome_unknown'
 AND NOT EXISTS(SELECT 1 FROM dispatch_recoveries r WHERE r.original_id=d.id)
 AND NOT EXISTS(SELECT 1 FROM dispatch_stops s WHERE s.dispatch_id=d.id AND s.settled_at IS NOT NULL);
