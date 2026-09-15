-- Existing positive observations remain until explicitly invalidated; a new
-- negative observation cannot be undone by same-generation positive replay.
ALTER TABLE matrix_transports ADD COLUMN available INTEGER NOT NULL DEFAULT 1 CHECK(available IN (0,1));
ALTER TABLE matrix_transports ADD COLUMN invalidation TEXT;
DROP VIEW current_matrix_routes;
CREATE VIEW current_matrix_routes AS
SELECT route.session_id FROM matrix_session_routes route
JOIN runner_sessions s ON s.id=route.session_id AND s.matrix_generation>0
JOIN engagements e ON e.id=s.engagement_id AND e.state='active'
JOIN registrations reg ON reg.fleet_id=e.fleet_id AND reg.generation=e.generation
JOIN projects p ON p.fleet_id=e.fleet_id AND p.id=e.project_id AND p.generation=e.generation
JOIN matrix_transports transport ON transport.engagement_id=e.id AND transport.available=1
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

CREATE TRIGGER matrix_transport_retire_approvals AFTER UPDATE ON matrix_transports
 WHEN NEW.available=0 OR OLD.generation<>NEW.generation OR OLD.device_id<>NEW.device_id OR OLD.sender_mxid<>NEW.sender_mxid
 BEGIN
 UPDATE approval_grants SET revoked=1 WHERE engagement_id=OLD.engagement_id;
 UPDATE owner_approvals SET state=CASE WHEN state='applying' THEN 'uncertain' ELSE 'invalidated' END
 WHERE context_id IN (SELECT id FROM approval_contexts WHERE engagement_id=OLD.engagement_id)
 AND state IN ('pending','decided','applying');
 END;
