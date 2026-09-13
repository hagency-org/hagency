-- MA-S4 (ADR-114 amendment): the retirement audit row. Retiring an account performs
-- a host-observed logout (the operator runs the provider's own logout inside the
-- retained namespace); native records only the DERIVED outcome, never drives the
-- logout, never captures stdout, never reads a token or auth file. Idempotent:
-- recovery fixtures rewind user_version and replay this migration (the
-- 023/026/027/028 discipline); the single statement is CREATE ... IF NOT EXISTS and
-- nothing is altered — managed_accounts keeps its 023 columns and its four-state
-- CHECK untouched (the audit row is a separate table, not a column).
CREATE TABLE IF NOT EXISTS account_logout_receipts (
  id             TEXT PRIMARY KEY,   -- logout_<32 hex>, store-generated
  account_id     TEXT NOT NULL REFERENCES managed_accounts(id),
  retired_at_ms  INTEGER NOT NULL,   -- the transition clock
  readiness      TEXT NOT NULL CHECK(readiness IN ('observed','unknown')),
  -- the derived readiness word the namespace reached; 'unknown' when the logout
  -- could not be observed (failure, refusal, unclassifiable) — never a
  -- clean-retirement claim the logout did not observe.
  logout_detail  TEXT NOT NULL CHECK(length(logout_detail) <= 512)
  -- the parent's bounded classification of the logout child's exit, from a closed
  -- vocabulary — never the provider's verbatim words, never a credential.
) STRICT;
