-- Duplicate legacy native bindings fail migration rather than enabling two runners
-- for one conversation. Production imports are not enabled by this migration.
CREATE UNIQUE INDEX canonical_runner_session ON runner_sessions(
 engagement_id,json_extract(binding,'$.room_id'),COALESCE(json_extract(binding,'$.thread_root'),'')
);
CREATE TABLE admitted_messages (
 sequence INTEGER PRIMARY KEY AUTOINCREMENT, source_key TEXT NOT NULL UNIQUE,
 digest TEXT NOT NULL, config TEXT NOT NULL CHECK(json_valid(config))
) STRICT;
CREATE TABLE session_inputs (
 session_id TEXT NOT NULL REFERENCES runner_sessions(id),
 message_sequence INTEGER NOT NULL REFERENCES admitted_messages(sequence),
 wake INTEGER NOT NULL CHECK(wake IN (0,1)),
 dispatch_id TEXT REFERENCES runner_dispatches(id), processed_at INTEGER,
 PRIMARY KEY(session_id,message_sequence)
) STRICT;
CREATE INDEX session_inbox ON session_inputs(session_id,message_sequence) WHERE processed_at IS NULL AND dispatch_id IS NULL;
CREATE TABLE dispatch_inputs (
 dispatch_id TEXT NOT NULL REFERENCES runner_dispatches(id),
 message_sequence INTEGER NOT NULL REFERENCES admitted_messages(sequence),
 PRIMARY KEY(dispatch_id,message_sequence)
) STRICT;
