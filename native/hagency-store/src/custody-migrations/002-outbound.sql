-- A host-authenticated transport is distinct from operator fixture intake.
CREATE TABLE outbound_transports (
 binding TEXT PRIMARY KEY NOT NULL REFERENCES bindings(id),
 identity TEXT NOT NULL, fleet_key TEXT NOT NULL UNIQUE, consumer TEXT NOT NULL UNIQUE,
 machine_generation INTEGER NOT NULL CHECK(machine_generation>0),
 fingerprint TEXT NOT NULL, scope TEXT NOT NULL,
 sequence INTEGER NOT NULL DEFAULT 0,
 accepted_sequence INTEGER NOT NULL DEFAULT 0, accepted_digest TEXT
) STRICT;
CREATE TABLE outbound_polls (
 binding TEXT NOT NULL REFERENCES outbound_transports(binding),
 lane TEXT NOT NULL CHECK(lane IN ('matrix','work')), ticket TEXT NOT NULL, response_digest TEXT,
 PRIMARY KEY(binding,lane)
) STRICT;
ALTER TABLE inbox ADD COLUMN kind TEXT;
ALTER TABLE inbox ADD COLUMN origin_machine_generation INTEGER;
ALTER TABLE inbox ADD COLUMN lease_generation INTEGER;
ALTER TABLE inbox ADD COLUMN lease_token TEXT;
ALTER TABLE inbox ADD COLUMN lease_expires INTEGER;
ALTER TABLE inbox ADD COLUMN lease_state TEXT CHECK(lease_state IN ('received','unknown','accepted','stale','retired'));
ALTER TABLE inbox ADD COLUMN processing_state TEXT CHECK(processing_state IN ('pending','claimed','started','unknown','done','retired'));
ALTER TABLE inbox ADD COLUMN retry_at INTEGER NOT NULL DEFAULT 0;
CREATE INDEX outbound_pending ON inbox(binding,lane,processing_state);
CREATE TABLE outbound_attempts (
 binding TEXT NOT NULL REFERENCES outbound_transports(binding),
 id TEXT NOT NULL, lane TEXT NOT NULL, delivery_id TEXT NOT NULL,
 command_digest TEXT NOT NULL, capability TEXT NOT NULL,
 state TEXT NOT NULL CHECK(state IN ('claimed','started','unknown','completed','retry','retired')),
 deadline INTEGER NOT NULL, result TEXT, result_digest TEXT,
 PRIMARY KEY(binding,id),
 FOREIGN KEY(binding,lane,delivery_id) REFERENCES inbox(binding,lane,id)
) STRICT;
CREATE UNIQUE INDEX outbound_one_active_attempt ON outbound_attempts(binding,lane,delivery_id)
 WHERE state IN ('claimed','started','unknown');
CREATE TABLE outbound_publications (
 binding TEXT PRIMARY KEY NOT NULL REFERENCES outbound_transports(binding),
 sequence INTEGER NOT NULL, digest TEXT NOT NULL, body TEXT NOT NULL,
 state TEXT NOT NULL CHECK(state IN ('ready','unknown','rejected'))
) STRICT;
