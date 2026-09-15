CREATE TABLE registrations (
    fleet_id TEXT PRIMARY KEY,
    generation INTEGER NOT NULL CHECK(generation > 0),
    config TEXT NOT NULL CHECK(json_valid(config))
) STRICT;
CREATE TABLE projects (
    fleet_id TEXT NOT NULL REFERENCES registrations(fleet_id),
    id TEXT NOT NULL,
    generation INTEGER NOT NULL,
    room_id TEXT NOT NULL,
    owner_mxid TEXT NOT NULL,
    owner_room_id TEXT NOT NULL,
    PRIMARY KEY(fleet_id,id),
    UNIQUE(fleet_id,room_id)
) STRICT;
CREATE TABLE seats (id TEXT PRIMARY KEY, config TEXT NOT NULL CHECK(json_valid(config))) STRICT;
CREATE TABLE resources (
    id TEXT PRIMARY KEY,
    preset_id TEXT NOT NULL UNIQUE,
    config TEXT NOT NULL CHECK(json_valid(config))
) STRICT;
CREATE TABLE engagements (
    id TEXT PRIMARY KEY,
    fleet_id TEXT NOT NULL REFERENCES registrations(fleet_id),
    generation INTEGER NOT NULL,
    request_id TEXT NOT NULL,
    digest TEXT NOT NULL,
    context TEXT NOT NULL CHECK(json_valid(context)),
    evidence TEXT NOT NULL CHECK(json_valid(evidence)),
    project_id TEXT NOT NULL,
    name TEXT NOT NULL,
    resource_id TEXT NOT NULL REFERENCES resources(id),
    preset_id TEXT,
    seat_id TEXT,
    tokens INTEGER NOT NULL CHECK(tokens > 0),
    state TEXT NOT NULL CHECK(state IN ('pending','reserved','active','rejected','revoked','failed')),
    projection TEXT NOT NULL CHECK(json_valid(projection)),
    UNIQUE(fleet_id,request_id),
    FOREIGN KEY(fleet_id,project_id) REFERENCES projects(fleet_id,id)
) STRICT;
CREATE UNIQUE INDEX live_project_names ON engagements(fleet_id,project_id,name)
    WHERE state IN ('pending','reserved','active');
CREATE INDEX pool_commitments ON engagements(preset_id,state);
CREATE INDEX seat_commitments ON engagements(seat_id,state);
CREATE TABLE decisions (
    id TEXT PRIMARY KEY,
    digest TEXT NOT NULL,
    result TEXT NOT NULL CHECK(json_valid(result))
) STRICT;
CREATE TABLE effects (
    id TEXT PRIMARY KEY,
    engagement_id TEXT NOT NULL REFERENCES engagements(id),
    kind TEXT NOT NULL CHECK(kind IN ('provision','retire')),
    state TEXT NOT NULL CHECK(state IN ('pending','started','uncertain','complete','failed','cancelled')),
    fence INTEGER NOT NULL DEFAULT 0,
    payload TEXT NOT NULL CHECK(json_valid(payload)),
    outcome_digest TEXT,
    UNIQUE(engagement_id,kind)
) STRICT;
CREATE INDEX pending_effects ON effects(state,id);
