ALTER TABLE runner_sessions ADD COLUMN matrix_generation INTEGER NOT NULL DEFAULT 0;
DROP INDEX canonical_runner_session;
CREATE UNIQUE INDEX canonical_runner_session ON runner_sessions(
 engagement_id,
 CASE WHEN json_extract(binding,'$.kind')='internal' THEN 'internal' ELSE 'matrix' END,
 CASE WHEN json_extract(binding,'$.kind')='internal' THEN id ELSE json_extract(binding,'$.room_id') END,
 COALESCE(json_extract(binding,'$.thread_root'),''),matrix_generation
);
CREATE TABLE matrix_transports (
 engagement_id TEXT PRIMARY KEY REFERENCES engagements(id),
 registration_generation INTEGER NOT NULL,generation INTEGER NOT NULL,
 server_name TEXT NOT NULL,sender_mxid TEXT NOT NULL,device_id TEXT NOT NULL,
 UNIQUE(server_name,sender_mxid)
) STRICT;
CREATE TABLE matrix_room_scopes (
 server_name TEXT NOT NULL,room_id TEXT NOT NULL,generation INTEGER NOT NULL,
 fleet_id TEXT NOT NULL,project_id TEXT NOT NULL,registration_generation INTEGER NOT NULL,
 owner_mxid TEXT NOT NULL,privacy TEXT NOT NULL CHECK(json_valid(privacy)),
 direct_sender TEXT,
 joined TEXT NOT NULL CHECK(json_valid(joined)),invite_only INTEGER NOT NULL CHECK(invite_only IN (0,1)),
 available INTEGER NOT NULL DEFAULT 1 CHECK(available IN (0,1)),invalidation TEXT,
 encrypted INTEGER NOT NULL CHECK(encrypted IN (0,1)),
 PRIMARY KEY(server_name,room_id),
 FOREIGN KEY(fleet_id,project_id) REFERENCES projects(fleet_id,id)
) STRICT;
CREATE TABLE matrix_room_memberships (
 engagement_id TEXT NOT NULL REFERENCES engagements(id),server_name TEXT NOT NULL,room_id TEXT NOT NULL,
 room_generation INTEGER NOT NULL,transport_generation INTEGER NOT NULL,
 PRIMARY KEY(engagement_id,server_name,room_id),
 FOREIGN KEY(server_name,room_id) REFERENCES matrix_room_scopes(server_name,room_id)
) STRICT;
CREATE TABLE matrix_session_routes (
 session_id TEXT PRIMARY KEY REFERENCES runner_sessions(id),
 server_name TEXT NOT NULL,room_id TEXT NOT NULL,room_generation INTEGER NOT NULL,
 transport_generation INTEGER NOT NULL,registration_generation INTEGER NOT NULL,
 retired INTEGER NOT NULL DEFAULT 0 CHECK(retired IN (0,1)),
 config TEXT NOT NULL CHECK(json_valid(config)),
 FOREIGN KEY(server_name,room_id) REFERENCES matrix_room_scopes(server_name,room_id)
) STRICT;
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

CREATE TABLE final_replies (
 id TEXT PRIMARY KEY,session_id TEXT NOT NULL REFERENCES runner_sessions(id),
 task_id TEXT NOT NULL REFERENCES canonical_tasks(id),execution_epoch INTEGER NOT NULL,
 source_dispatch_id TEXT NOT NULL REFERENCES runner_dispatches(id),
 transaction_id TEXT NOT NULL UNIQUE,digest TEXT NOT NULL,body TEXT NOT NULL,
 route TEXT NOT NULL CHECK(json_valid(route)),
 state TEXT NOT NULL CHECK(state IN ('pending','claimed','sending','uncertain','delivered','cancelled')),
 cancel_requested INTEGER NOT NULL DEFAULT 0 CHECK(cancel_requested IN (0,1)),
 fence INTEGER NOT NULL DEFAULT 0,claim_hash TEXT,claim_until INTEGER,
 event_id TEXT,observation TEXT,created_at INTEGER NOT NULL,updated_at INTEGER NOT NULL,
 UNIQUE(task_id,execution_epoch)
) STRICT;
CREATE TABLE final_reply_calls (
 dispatch_id TEXT NOT NULL REFERENCES runner_dispatches(id),call_id TEXT NOT NULL,
 reply_id TEXT NOT NULL REFERENCES final_replies(id),digest TEXT NOT NULL,
 PRIMARY KEY(dispatch_id,call_id)
) STRICT;
CREATE TABLE final_reply_inspections (
 reply_id TEXT NOT NULL REFERENCES final_replies(id),fence INTEGER NOT NULL,digest TEXT NOT NULL,
 PRIMARY KEY(reply_id,fence)
) STRICT;
CREATE VIEW current_final_replies AS
SELECT f.id FROM final_replies f
JOIN current_matrix_routes route ON route.session_id=f.session_id
JOIN canonical_tasks t ON t.id=f.task_id AND t.session_id=f.session_id
JOIN matrix_session_routes r ON r.session_id=f.session_id AND r.config=f.route
WHERE json_extract(t.config,'$.status')='done'
 AND json_extract(t.config,'$.execution_epoch')=f.execution_epoch;
