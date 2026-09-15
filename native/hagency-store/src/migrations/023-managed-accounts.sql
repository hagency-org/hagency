-- Private native credential namespace observations, never provider auth facts.
-- Idempotent: recovery tests rewind user_version and replay this migration.
CREATE TABLE IF NOT EXISTS account_identity_key (
  singleton INTEGER PRIMARY KEY CHECK(singleton=1),
  secret BLOB NOT NULL CHECK(length(secret)=32),
  deployment TEXT NOT NULL CHECK(length(deployment)=32),
  root_identity TEXT NOT NULL CHECK(json_valid(root_identity))
) STRICT;
CREATE TABLE IF NOT EXISTS managed_accounts (
  id TEXT PRIMARY KEY,
  ordinal INTEGER NOT NULL UNIQUE CHECK(ordinal BETWEEN 1 AND 16),
  generation INTEGER NOT NULL CHECK(generation=1),
  state TEXT NOT NULL CHECK(state IN ('preparing','active','uncertain','retired')),
  namespace_identity TEXT,
  identity_tuple TEXT,
  seat_id TEXT UNIQUE,
  CHECK(state NOT IN ('active','retired') OR (namespace_identity IS NOT NULL AND identity_tuple IS NOT NULL AND seat_id IS NOT NULL))
) STRICT;
CREATE TABLE IF NOT EXISTS resource_accounts (
  preset_id TEXT PRIMARY KEY,
  account_id TEXT NOT NULL REFERENCES managed_accounts(id),
  binding_generation INTEGER NOT NULL CHECK(binding_generation=1)
) STRICT;
CREATE TRIGGER IF NOT EXISTS resource_account_no_update BEFORE UPDATE ON resource_accounts
BEGIN SELECT RAISE(ABORT,'immutable account association'); END;
CREATE TRIGGER IF NOT EXISTS resource_account_no_delete BEFORE DELETE ON resource_accounts
BEGIN SELECT RAISE(ABORT,'immutable account association'); END;
