-- Legacy routes get no fabricated visibility boundary. Renew room/device
-- observations and create a fresh session before verified ingress is available.
ALTER TABLE matrix_transports ADD COLUMN observed_at INTEGER;
ALTER TABLE matrix_room_scopes ADD COLUMN visibility_since INTEGER;
ALTER TABLE matrix_session_routes ADD COLUMN ingress_since INTEGER;
ALTER TABLE matrix_session_routes ADD COLUMN parent_session_id TEXT REFERENCES runner_sessions(id);
ALTER TABLE session_inputs ADD COLUMN config TEXT CHECK(config IS NULL OR json_valid(config));
ALTER TABLE task_inputs ADD COLUMN config TEXT CHECK(config IS NULL OR json_valid(config));
ALTER TABLE task_inputs ADD COLUMN wake INTEGER CHECK(wake IS NULL OR wake IN (0,1));
ALTER TABLE task_notices ADD COLUMN verified_route TEXT CHECK(verified_route IS NULL OR json_valid(verified_route));
ALTER TABLE task_notices ADD COLUMN content_digest TEXT;
CREATE TABLE matrix_ingress_events (
 engagement_id TEXT NOT NULL REFERENCES engagements(id),source_key TEXT NOT NULL,
 message_sequence INTEGER NOT NULL REFERENCES admitted_messages(sequence),
 source_session_id TEXT NOT NULL REFERENCES runner_sessions(id),
 scope_digest TEXT NOT NULL,digest TEXT NOT NULL,config TEXT NOT NULL CHECK(json_valid(config)),
 PRIMARY KEY(engagement_id,source_key)
) STRICT;
CREATE INDEX matrix_ingress_source ON matrix_ingress_events(source_key);
CREATE TABLE verified_task_requests (
 source_session_id TEXT NOT NULL REFERENCES runner_sessions(id),request_key TEXT NOT NULL,
 digest TEXT NOT NULL,task_id TEXT NOT NULL REFERENCES canonical_tasks(id),
 PRIMARY KEY(source_session_id,request_key)
) STRICT;
DROP VIEW current_matrix_routes;
CREATE VIEW current_matrix_routes AS
SELECT route.session_id FROM matrix_session_routes route
JOIN runner_sessions s ON s.id=route.session_id AND s.matrix_generation>0
JOIN engagements e ON e.id=s.engagement_id AND e.state='active'
JOIN registrations reg ON reg.fleet_id=e.fleet_id AND reg.generation=e.generation
JOIN projects p ON p.fleet_id=e.fleet_id AND p.id=e.project_id AND p.generation=e.generation
JOIN matrix_transports transport ON transport.engagement_id=e.id
JOIN matrix_room_scopes room ON room.server_name=route.server_name AND room.room_id=route.room_id
JOIN matrix_room_memberships membership ON membership.engagement_id=e.id AND membership.server_name=room.server_name AND membership.room_id=room.room_id
WHERE route.registration_generation=reg.generation
 AND (route.parent_session_id IS NULL OR EXISTS(SELECT 1 FROM matrix_session_routes parent WHERE parent.session_id=route.parent_session_id AND parent.retired=0 AND parent.registration_generation=route.registration_generation AND parent.room_generation=route.room_generation AND parent.transport_generation=route.transport_generation AND parent.server_name=route.server_name AND parent.room_id=route.room_id AND json_extract(parent.config,'$.engagement_id')=e.id))
 AND route.retired=0 AND s.matrix_generation=json_extract(route.config,'$.session_generation')
 AND transport.registration_generation=reg.generation AND transport.generation=route.transport_generation
 AND transport.server_name=route.server_name AND route.server_name=json_extract(reg.config,'$.serverName')
 AND room.available=1
 AND EXISTS(SELECT 1 FROM json_each(room.joined) WHERE value=transport.sender_mxid)
 AND EXISTS(SELECT 1 FROM json_each(room.joined) WHERE value=p.owner_mxid)
 AND room.generation=route.room_generation AND room.registration_generation=reg.generation
 AND membership.room_generation=room.generation AND membership.transport_generation=transport.generation
 AND (json_extract(room.privacy,'$.kind')='group' OR room.direct_sender=transport.sender_mxid)
 AND room.fleet_id=e.fleet_id AND room.project_id=e.project_id AND room.owner_mxid=p.owner_mxid
 AND json_extract(s.binding,'$.kind') IS NULL
 AND json_extract(s.binding,'$.room_id')=route.room_id
 AND json_extract(route.config,'$.session_id')=s.id
 AND json_extract(route.config,'$.server_name')=route.server_name
 AND json_extract(route.config,'$.room_id')=route.room_id
 AND json_extract(route.config,'$.room_generation')=route.room_generation
 AND json_extract(route.config,'$.transport_generation')=route.transport_generation
 AND json_extract(route.config,'$.registration_generation')=route.registration_generation
 AND json_extract(route.config,'$.engagement_id')=e.id
 AND json_extract(route.config,'$.fleet_id')=e.fleet_id
 AND json_extract(route.config,'$.project_id')=e.project_id
 AND json_extract(route.config,'$.owner_mxid')=p.owner_mxid
 AND json_extract(route.config,'$.sender_mxid')=transport.sender_mxid
 AND json_extract(route.config,'$.device_id')=transport.device_id
 AND json_extract(route.config,'$.privacy')=room.privacy
 AND json_extract(route.config,'$.encrypted')=room.encrypted
 AND json_extract(route.config,'$.thread_root') IS json_extract(s.binding,'$.thread_root');
DROP VIEW task_followup_ready;
CREATE VIEW task_followup_ready AS
SELECT DISTINCT d.id AS dispatch_id,t.id AS task_id
FROM runner_dispatches d
JOIN canonical_tasks t ON t.id=d.task_id AND t.session_id=d.session_id
JOIN task_intents i ON i.task_id=t.id AND i.state='active'
JOIN admitted_messages root ON root.sequence=i.root_sequence
JOIN task_inputs root_input ON root_input.task_id=t.id AND root_input.message_sequence=i.root_sequence
JOIN runner_sessions session ON session.id=d.session_id
LEFT JOIN matrix_session_routes route ON route.session_id=d.session_id
JOIN dispatch_inputs di ON di.dispatch_id=d.id
JOIN task_inputs ti ON ti.task_id=t.id AND ti.message_sequence=di.message_sequence
JOIN session_inputs si ON si.session_id=d.session_id AND si.message_sequence=di.message_sequence
JOIN admitted_messages m ON m.sequence=di.message_sequence
WHERE json_extract(t.config,'$.status')='done'
 AND si.dispatch_id=d.id AND si.processed_at IS NULL AND si.wake=1
 AND m.sequence>root.sequence
 AND json_extract(COALESCE(si.config,m.config),'$.sender_mxid')=json_extract(COALESCE(root_input.config,root.config),'$.sender_mxid')
 AND json_extract(COALESCE(si.config,m.config),'$.room_id')=json_extract(COALESCE(root_input.config,root.config),'$.room_id')
 AND json_extract(COALESCE(si.config,m.config),'$.server_name')=json_extract(COALESCE(root_input.config,root.config),'$.server_name')
 AND (json_extract(COALESCE(si.config,m.config),'$.thread_root')=json_extract(COALESCE(root_input.config,root.config),'$.event_id') OR (session.matrix_generation>0 AND json_extract(route.config,'$.privacy.kind')='direct' AND json_extract(session.binding,'$.thread_root') IS NULL AND json_extract(COALESCE(si.config,m.config),'$.thread_root') IS NULL))
 AND json_extract(COALESCE(si.config,m.config),'$.kind') IN ('m.text','m.file','m.image','m.audio','m.video')
 AND json_extract(COALESCE(root_input.config,root.config),'$.kind') IN ('m.text','m.file','m.image','m.audio','m.video')
 AND json_extract(COALESCE(si.config,m.config),'$.received_at')>json_extract(t.config,'$.completed_at')
 AND json_extract(COALESCE(si.config,m.config),'$.origin_ts')>json_extract(t.config,'$.completed_at');
