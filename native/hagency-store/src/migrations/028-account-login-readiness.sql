-- MA-S1 (ADR-114 amendment): a provider-login readiness FACT, observed by running
-- the provider's own login under an operator-held terminal inside the retained
-- namespace. Never inferred from files (D1). No credential byte is stored here:
-- mode/expiry are DERIVED facts with provenance; provider_state carries the
-- PARENT's closed-vocabulary classification of the child's exit, never provider
-- words — the child inherits the operator's terminal and the parent captures no
-- bytes. Idempotent: recovery fixtures rewind user_version and replay this
-- migration (the 023/026 discipline), so these are CREATE TABLE IF NOT EXISTS and
-- managed_accounts is never ALTERed (the 025 replay hazard).
CREATE TABLE IF NOT EXISTS account_login_observations (
  id                 TEXT PRIMARY KEY,          -- observation_<32 hex>, store-generated
  account_id         TEXT NOT NULL REFERENCES managed_accounts(id),
  account_generation INTEGER NOT NULL CHECK(account_generation = 1),
  attempt            INTEGER NOT NULL CHECK(attempt >= 1),  -- 1-based; no silent retry
  observed_at_ms     INTEGER NOT NULL,
  expires_at_ms      INTEGER NOT NULL,          -- ALWAYS set: the provider's expiry or observed_at + default TTL
  mode               TEXT NOT NULL CHECK(mode IN ('subscription','api_key','unknown')),
  provider_state     TEXT NOT NULL CHECK(length(provider_state) <= 512
                           AND provider_state NOT LIKE '%credential%'),  -- the PARENT's classification (closed vocabulary)
  outcome            TEXT NOT NULL CHECK(outcome IN ('observed','refused','uncertain')),
  -- Derived in place: an `observed` row must carry something that discriminates
  -- it. Every observed arm supplies a non-empty classification, so the
  -- constraint is satisfiable and never vacuous.
  CHECK(outcome <> 'observed' OR mode <> 'unknown' OR provider_state <> '')
) STRICT;
CREATE INDEX IF NOT EXISTS account_login_latest
  ON account_login_observations(account_id, observed_at_ms DESC);

-- Allocation of one attempt BEFORE the effect: mirrors materialise's
-- durable-state-before-mkdir ordering, so a crash between the attempt row and
-- the receipt leaves `attempting`, which reconciliation settles to `uncertain`
-- — never `observed`. A login is not an account state transition: an active
-- account stays active while only its readiness fact is pending (P4).
CREATE TABLE IF NOT EXISTS account_login_attempts (
  account_id      TEXT PRIMARY KEY REFERENCES managed_accounts(id),
  attempt         INTEGER NOT NULL CHECK(attempt >= 1),
  started_at_ms   INTEGER NOT NULL,
  deadline_ms     INTEGER NOT NULL,
  state           TEXT NOT NULL CHECK(state IN ('attempting','settled')),
  receipt_id      TEXT                           -- set when settled
) STRICT;
