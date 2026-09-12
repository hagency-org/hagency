CREATE TABLE peer_messages (
 sequence INTEGER PRIMARY KEY AUTOINCREMENT,
 source_key TEXT NOT NULL UNIQUE,digest TEXT NOT NULL,
 conversation_id TEXT NOT NULL REFERENCES internal_conversations(id),
 config TEXT NOT NULL CHECK(json_valid(config))
) STRICT;
CREATE TABLE peer_session_inputs (
 session_id TEXT NOT NULL REFERENCES runner_sessions(id),
 message_sequence INTEGER NOT NULL REFERENCES peer_messages(sequence),
 wake INTEGER NOT NULL CHECK(wake IN (0,1)),
 dispatch_id TEXT REFERENCES runner_dispatches(id),processed_at INTEGER,
 PRIMARY KEY(session_id,message_sequence)
) STRICT;
CREATE INDEX peer_pending_inbox ON peer_session_inputs(session_id,message_sequence)
 WHERE processed_at IS NULL AND dispatch_id IS NULL;
CREATE TABLE peer_dispatch_inputs (
 dispatch_id TEXT NOT NULL REFERENCES runner_dispatches(id),
 message_sequence INTEGER NOT NULL REFERENCES peer_messages(sequence),
 PRIMARY KEY(dispatch_id,message_sequence)
) STRICT;
CREATE VIEW live_peer_inputs AS
SELECT i.session_id,i.message_sequence
FROM peer_session_inputs i
JOIN peer_messages m ON m.sequence=i.message_sequence
JOIN internal_conversations c ON c.id=m.conversation_id
JOIN runner_sessions s ON s.id=i.session_id
JOIN engagements e ON e.id=s.engagement_id
JOIN registrations r ON r.fleet_id=e.fleet_id
WHERE c.state='active' AND e.state='active' AND e.generation=r.generation
 AND c.fleet_id=e.fleet_id AND c.project_id=e.project_id AND c.generation=e.generation
 AND (c.creator_session_id=s.id OR EXISTS(
   SELECT 1 FROM internal_participants p WHERE p.conversation_id=c.id AND p.session_id=s.id
 ));
-- A peer response can resume already-started work, but cannot replace the
-- initial Matrix input or the original human's post-completion follow-up.
CREATE VIEW task_dispatch_input_ready AS
SELECT d.id AS dispatch_id,t.id AS task_id
FROM runner_dispatches d JOIN canonical_tasks t ON t.id=d.task_id
WHERE EXISTS(SELECT 1 FROM dispatch_inputs di JOIN task_inputs ti
 ON ti.message_sequence=di.message_sequence
 WHERE di.dispatch_id=d.id AND ti.task_id=t.id)
 OR (json_extract(t.config,'$.status') IN ('in_progress','blocked') AND EXISTS(
 SELECT 1 FROM peer_dispatch_inputs pi
 JOIN peer_session_inputs si ON si.message_sequence=pi.message_sequence
 JOIN live_peer_inputs li ON li.session_id=si.session_id AND li.message_sequence=si.message_sequence
 WHERE pi.dispatch_id=d.id AND si.session_id=d.session_id AND si.dispatch_id=d.id
 AND si.processed_at IS NULL AND si.wake=1));
