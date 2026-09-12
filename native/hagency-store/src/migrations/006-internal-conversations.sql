CREATE TABLE internal_conversations (
 id TEXT PRIMARY KEY,
 fleet_id TEXT NOT NULL,project_id TEXT NOT NULL,generation INTEGER NOT NULL,
 creator_session_id TEXT NOT NULL REFERENCES runner_sessions(id),
 request_scope TEXT NOT NULL,request_key TEXT NOT NULL,digest TEXT NOT NULL,
 config TEXT NOT NULL CHECK(json_valid(config)),
 state TEXT NOT NULL CHECK(state IN ('active','closed')),
 UNIQUE(request_scope,request_key),
 FOREIGN KEY(fleet_id,project_id) REFERENCES projects(fleet_id,id)
) STRICT;
CREATE TABLE internal_participants (
 conversation_id TEXT NOT NULL REFERENCES internal_conversations(id),
 engagement_id TEXT NOT NULL REFERENCES engagements(id),
 session_id TEXT NOT NULL UNIQUE REFERENCES runner_sessions(id),
 PRIMARY KEY(conversation_id,engagement_id)
) STRICT;
DROP INDEX canonical_runner_session;
CREATE UNIQUE INDEX canonical_runner_session ON runner_sessions(
 engagement_id,
 CASE WHEN json_extract(binding,'$.kind')='internal' THEN 'internal' ELSE 'matrix' END,
 COALESCE(json_extract(binding,'$.conversation_id'),json_extract(binding,'$.room_id')),
 COALESCE(json_extract(binding,'$.thread_root'),'')
);
