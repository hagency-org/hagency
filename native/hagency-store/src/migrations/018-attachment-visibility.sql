-- No backfilled authority for pre-migration messages or dispatches.
CREATE TABLE IF NOT EXISTS matrix_attachments (
 engagement_id TEXT NOT NULL REFERENCES engagements(id),
 source_key TEXT NOT NULL,
 message_sequence INTEGER NOT NULL REFERENCES admitted_messages(sequence),
 source_session_id TEXT NOT NULL REFERENCES runner_sessions(id),
 digest TEXT NOT NULL, content_digest TEXT NOT NULL,
 metadata TEXT NOT NULL CHECK(json_valid(metadata)),
 sdk_identity TEXT NOT NULL, manifest_id TEXT NOT NULL,
 PRIMARY KEY(engagement_id,source_key),
 UNIQUE(engagement_id,message_sequence),
 FOREIGN KEY(engagement_id,source_key) REFERENCES matrix_ingress_events(engagement_id,source_key)
) STRICT;
CREATE INDEX IF NOT EXISTS matrix_attachment_source ON matrix_attachments(source_key);
CREATE TABLE IF NOT EXISTS session_attachment_visibility (
 projection_sequence INTEGER PRIMARY KEY AUTOINCREMENT,
 session_id TEXT NOT NULL REFERENCES runner_sessions(id),
 engagement_id TEXT NOT NULL,
 source_key TEXT NOT NULL,
 message_sequence INTEGER NOT NULL REFERENCES admitted_messages(sequence),
 UNIQUE(session_id,message_sequence),
 FOREIGN KEY(engagement_id,source_key) REFERENCES matrix_attachments(engagement_id,source_key)
) STRICT;
CREATE INDEX IF NOT EXISTS attachment_session_page ON session_attachment_visibility(session_id,projection_sequence);
CREATE TABLE IF NOT EXISTS dispatch_attachment_windows (
 dispatch_id TEXT PRIMARY KEY REFERENCES runner_dispatches(id),
 session_id TEXT NOT NULL REFERENCES runner_sessions(id),
 source_cutoff INTEGER NOT NULL CHECK(source_cutoff>=0),
 projection_cutoff INTEGER NOT NULL CHECK(projection_cutoff>=0)
) STRICT;
