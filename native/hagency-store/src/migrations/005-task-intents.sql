CREATE TABLE task_intents (
 task_id TEXT PRIMARY KEY REFERENCES canonical_tasks(id),
 request_scope TEXT NOT NULL,request_key TEXT NOT NULL,digest TEXT NOT NULL,
 session_id TEXT NOT NULL UNIQUE REFERENCES runner_sessions(id),
 root_sequence INTEGER NOT NULL REFERENCES admitted_messages(sequence),
 state TEXT NOT NULL CHECK(state IN ('pending','active','closed')),
 anchor_event_id TEXT,
 UNIQUE(request_scope,request_key)
) STRICT;
CREATE TABLE task_inputs (
 task_id TEXT NOT NULL REFERENCES canonical_tasks(id),
 message_sequence INTEGER NOT NULL REFERENCES admitted_messages(sequence),
 PRIMARY KEY(task_id,message_sequence)
) STRICT;
CREATE TABLE task_input_receipts (
 scope TEXT NOT NULL,request_key TEXT NOT NULL,digest TEXT NOT NULL,
 task_id TEXT NOT NULL REFERENCES canonical_tasks(id),
 PRIMARY KEY(scope,request_key)
) STRICT;
CREATE TABLE task_notices (
 id TEXT PRIMARY KEY,task_id TEXT NOT NULL REFERENCES canonical_tasks(id),
 config TEXT NOT NULL CHECK(json_valid(config)),
 state TEXT NOT NULL CHECK(state IN ('pending','claimed','delivered','failed','cancelled')),
 claim_hash TEXT,claim_until INTEGER,delivery TEXT CHECK(delivery IS NULL OR json_valid(delivery)),
 error_code TEXT,not_before INTEGER NOT NULL DEFAULT 0
) STRICT;
CREATE INDEX task_notice_ready ON task_notices(state,not_before,id);
-- One predicate is used before leasing and again at the actual start. A queued
-- dispatch alone never reopens a task, nor does unrelated/previously read input.
CREATE VIEW task_followup_ready AS
SELECT DISTINCT d.id AS dispatch_id,t.id AS task_id
FROM runner_dispatches d
JOIN canonical_tasks t ON t.id=d.task_id AND t.session_id=d.session_id
JOIN task_intents i ON i.task_id=t.id AND i.state='active'
JOIN admitted_messages root ON root.sequence=i.root_sequence
JOIN dispatch_inputs di ON di.dispatch_id=d.id
JOIN task_inputs ti ON ti.task_id=t.id AND ti.message_sequence=di.message_sequence
JOIN session_inputs si ON si.session_id=d.session_id AND si.message_sequence=di.message_sequence
JOIN admitted_messages m ON m.sequence=di.message_sequence
WHERE json_extract(t.config,'$.status')='done'
 AND si.dispatch_id=d.id AND si.processed_at IS NULL AND si.wake=1
 AND m.sequence>root.sequence
 AND json_extract(m.config,'$.sender_mxid')=json_extract(root.config,'$.sender_mxid')
 AND json_extract(m.config,'$.room_id')=json_extract(root.config,'$.room_id')
 AND json_extract(m.config,'$.server_name')=json_extract(root.config,'$.server_name')
 AND json_extract(m.config,'$.thread_root')=json_extract(root.config,'$.event_id')
 AND json_extract(m.config,'$.kind') IN ('m.text','m.file','m.image','m.audio','m.video')
 AND json_extract(root.config,'$.kind') IN ('m.text','m.file','m.image','m.audio','m.video')
 AND json_extract(m.config,'$.received_at')>json_extract(t.config,'$.completed_at')
 AND json_extract(m.config,'$.origin_ts')>json_extract(t.config,'$.completed_at');
