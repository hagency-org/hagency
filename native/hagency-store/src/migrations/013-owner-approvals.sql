CREATE TABLE approval_rooms (
 server_name TEXT NOT NULL,room_id TEXT NOT NULL,generation INTEGER NOT NULL,
 fleet_id TEXT NOT NULL,project_id TEXT NOT NULL,registration_generation INTEGER NOT NULL,
 owner_mxid TEXT NOT NULL,bot_mxid TEXT NOT NULL,device_id TEXT NOT NULL,
 available INTEGER NOT NULL CHECK(available IN (0,1)),digest TEXT NOT NULL,config TEXT NOT NULL CHECK(json_valid(config)),
 PRIMARY KEY(server_name,room_id)
) STRICT;
CREATE TABLE approval_bindings (
 engagement_id TEXT PRIMARY KEY REFERENCES engagements(id),server_name TEXT NOT NULL,room_id TEXT NOT NULL,
 room_generation INTEGER NOT NULL,incarnation INTEGER NOT NULL,
 FOREIGN KEY(server_name,room_id) REFERENCES approval_rooms(server_name,room_id)
) STRICT;
CREATE VIEW current_approval_bindings AS
 SELECT b.engagement_id FROM approval_bindings b
 JOIN approval_rooms room ON room.server_name=b.server_name AND room.room_id=b.room_id AND room.generation=b.room_generation AND room.available=1
 JOIN engagements e ON e.id=b.engagement_id AND e.state='active'
 JOIN registrations r ON r.fleet_id=e.fleet_id AND r.generation=e.generation
 JOIN projects p ON p.fleet_id=e.fleet_id AND p.id=e.project_id AND p.generation=e.generation
 WHERE room.fleet_id=e.fleet_id AND room.project_id=e.project_id AND room.registration_generation=e.generation
 AND json_extract(room.config,'$.encrypted')=1 AND json_extract(room.config,'$.invite_only')=1 AND json_extract(room.config,'$.available')=1
 AND json_array_length(json_extract(room.config,'$.joined'))=2
 AND EXISTS(SELECT 1 FROM json_each(room.config,'$.joined') WHERE value=room.owner_mxid)
 AND EXISTS(SELECT 1 FROM json_each(room.config,'$.joined') WHERE value=room.bot_mxid)
 AND room.owner_mxid=p.owner_mxid AND room.room_id=p.owner_room_id
 AND room.server_name=json_extract(r.config,'$.serverName') AND room.bot_mxid=json_extract(r.config,'$.approvalBotMxid');
CREATE TABLE approval_contexts (
 id TEXT PRIMARY KEY,dispatch_id TEXT NOT NULL REFERENCES runner_dispatches(id),fence INTEGER NOT NULL,
 engagement_id TEXT NOT NULL REFERENCES engagements(id),digest TEXT NOT NULL,config TEXT NOT NULL CHECK(json_valid(config))
) STRICT;
CREATE INDEX approval_context_dispatch ON approval_contexts(dispatch_id,fence);
CREATE TABLE owner_approvals (
 id TEXT PRIMARY KEY,source_key TEXT NOT NULL UNIQUE,context_id TEXT NOT NULL REFERENCES approval_contexts(id),
 digest TEXT NOT NULL,config TEXT NOT NULL CHECK(json_valid(config)),scope_key TEXT,scope_kind TEXT,description TEXT,
 state TEXT NOT NULL CHECK(state IN ('pending','decided','applying','uncertain','applied','invalidated','not_applied')),
 choice TEXT,grant_id TEXT,expires_at INTEGER NOT NULL,application TEXT CHECK(application IS NULL OR json_valid(application)),
 observation TEXT CHECK(observation IS NULL OR json_valid(observation))
) STRICT;
CREATE INDEX owner_approvals_context ON owner_approvals(context_id,state);
CREATE TABLE approval_grants (
 id TEXT PRIMARY KEY,engagement_id TEXT NOT NULL REFERENCES engagements(id),binding_generation INTEGER NOT NULL,
 scope_key TEXT NOT NULL,scope_kind TEXT NOT NULL,mode TEXT NOT NULL CHECK(mode IN ('task','always')),
 task_id TEXT REFERENCES canonical_tasks(id),task_epoch INTEGER,
 context_key TEXT NOT NULL,revoked INTEGER NOT NULL DEFAULT 0 CHECK(revoked IN (0,1))
) STRICT;
CREATE INDEX approval_grant_match ON approval_grants(engagement_id,scope_key,context_key,revoked);
CREATE TABLE approval_verdict_receipts (
 source_key TEXT PRIMARY KEY,digest TEXT NOT NULL,request_id TEXT NOT NULL REFERENCES owner_approvals(id)
) STRICT;
CREATE TRIGGER approval_room_retire_grants AFTER UPDATE ON approval_rooms
 WHEN OLD.generation<>NEW.generation OR NEW.available=0 OR OLD.owner_mxid<>NEW.owner_mxid OR OLD.device_id<>NEW.device_id
 BEGIN UPDATE approval_grants SET revoked=1 WHERE engagement_id IN (SELECT engagement_id FROM approval_bindings WHERE server_name=OLD.server_name AND room_id=OLD.room_id); END;
CREATE TRIGGER approval_project_retire AFTER UPDATE ON projects
 WHEN OLD.owner_mxid<>NEW.owner_mxid OR OLD.owner_room_id<>NEW.owner_room_id OR OLD.generation<>NEW.generation
 BEGIN UPDATE approval_rooms SET available=0 WHERE fleet_id=OLD.fleet_id AND project_id=OLD.id; END;
CREATE TRIGGER approval_registration_retire AFTER UPDATE ON registrations
 WHEN OLD.generation<>NEW.generation
 BEGIN UPDATE approval_rooms SET available=0 WHERE fleet_id=OLD.fleet_id; END;
CREATE TRIGGER approval_engagement_retire AFTER UPDATE ON engagements
 WHEN NEW.state<>'active' OR OLD.generation<>NEW.generation
 BEGIN UPDATE approval_grants SET revoked=1 WHERE engagement_id=OLD.id; END;
CREATE TRIGGER approval_task_retire AFTER UPDATE ON canonical_tasks
 WHEN json_extract(NEW.config,'$.status')='done' OR json_extract(OLD.config,'$.execution_epoch')<>json_extract(NEW.config,'$.execution_epoch')
 BEGIN UPDATE approval_grants SET revoked=1 WHERE task_id=OLD.id AND mode='task'; END;
