spec: task
name: "Record the observed provider-login readiness fact without storing credentials"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, accounts, readiness, migration-028]
---

## Intent

Bind MA-S1 (ADR-114's amendment) under the decided D-ADR114: the provider
login is **observed at the host** — the operator runs the provider's own
login inside the retained namespace — and native records only the **derived
facts**: a mode (`subscription`/`api_key`/`unknown`) and an expiry. Readiness
is a fact with provenance and a bounded lifetime, never an inference from
files, and no credential byte is stored, logged, or served anywhere. The
store half only: the console DTO field is MA-S3b's and stays out.

## Constraints

### Must
- Record the observation in migration 028's new tables, created `IF NOT EXISTS` — never `ALTER TABLE managed_accounts` — carrying the derived mode, the always-set expiry (the provider's own when reported, otherwise the bounded default TTL), the bounded provider-state word, and the outcome (`observed`/`refused`/`uncertain`).
- Order the attempt row before the spawn and the receipt after the exit, in the SQLite-before-effect ordering materialise uses, so an interrupted or signalled login settles as `uncertain` on reopen — and `uncertain` is never promoted by a later read.
- Evaluate `usable` at read time: `outcome='observed'`, matching generation, unexpired; a read never writes, and an expired fact stays on disk as history while the answer degrades to `unknown`.
- Refuse ambient provider keys exactly as `apply_codex_environment` does (`accounts.rs:145-152`); the login child inherits the operator's terminal and its stdout is never captured.
- Keep the naming rule: no key matching `/credential/` (ADR-014's guard), pinned by a test.

### Must Not
- Do not store, log, serve or fixture any credential, token, refresh material, `auth.json` body or path to one — the derived mode and expiry are the whole record.
- Do not infer readiness from directory existence, file listing, or model selection — the only writer of a readiness fact is the login receipt.
- Do not add the readiness field to any console DTO (`AccountRow` stays five keys; MA-S3b owns that).
- Do not `ALTER` `managed_accounts`'s columns or its four-state CHECK; do not change the one-attempt materialise discipline, the `account_identity_key` singleton, the `resource_accounts` immutability triggers, or the close path.
- Do not gate any scenario by OS or feature in the binding set.

## Boundaries

### Allowed Changes
- native/hagency-store/src/domain/accounts.rs
- native/hagency-store/src/domain.rs
- native/hagency-store/src/domain_worker.rs
- native/hagency-store/src/lib.rs
- native/hagency-store/src/migrations/028-account-login-readiness.sql
- native/hagency-store/tests/accounts.rs
- native/hagency/src/bootstrap/accounts.rs
- native/hagency/src/main.rs
- native/scripts/account-vectors.mjs
- native/hagency-store/tests/fixtures/account-identity.json
- specs/task-rust-console-readiness-fact.spec.md
- knowledge/decisions/adr-114-native-managed-account-binding.md
- docs/progress.md

### Forbidden
- Live providers, real credentials, deployed state.
- mockup/** (the DTO field is MA-S3b's); native/hagency/src/console/**; any retained product file.

## Acceptance Criteria

Scenario: Readiness is observed, never inferred
  Test: native_account_login_readiness
  Level: integration
  Test Double: actual isolated files and SQLite, a recorded provider fixture
  Given an original private managed namespace with no observation
  When readiness is read, and then a login observation is recorded
  Then the first read is unknown and the second reports the observed mode, with no filesystem-derived answer anywhere in between

Scenario: An interrupted login is uncertain, never ready
  Test: native_account_login_interrupted_is_uncertain
  Level: integration
  Test Double: actual isolated files and SQLite
  Given an allocated attempt whose process does not settle
  When the account is reopened
  Then the receipt is uncertain, the attempt is settled, and no dispatch may consume it

Scenario: A stale readiness fact expires to unknown
  Test: native_account_readiness_expires_to_unknown
  Level: integration
  Test Double: actual isolated files and SQLite
  Given an observed fact past its expiry
  When readiness is read and a dispatch is selected
  Then the answer is unknown, the row is unchanged, and the dispatch parks as account_readiness_unknown

Scenario: No credential byte reaches any wire or fixture
  Test: native_account_login_records_no_credential_byte
  Level: integration
  Test Double: actual isolated files, SQLite, the CLI's own JSON output
  Given a login whose provider output carries a token-shaped string
  When the receipt, the store and the CLI output are inspected
  Then no token byte and no /credential/-matching key appears in any of them

Scenario: Dispatch requires an observed, unexpired account
  Level: integration
  Test Double: actual isolated files and SQLite
  Given an owned scope bound to an account whose readiness is unknown or expired
  When the queue is drained and the Host is asked to admit
  Then nothing is selected and no child starts

Scenario: Native readiness agrees with the retained verdict
  Test: native_account_readiness_matches_retained_detect
  Level: integration
  Test Double: actual isolated files, SQLite, and the retained probe's own check
  Given a fixture tree whose recorded readiness vectors are keyed by mode
  When native readiness and the retained probeFramework verdict are both evaluated
  Then they agree for every vector, so the vocabulary is pinned against retained rather than only against itself

Scenario: The CLI login route drives the production path
  Test: native_account_login_route_records_ready
  Level: integration
  Test Double: actual isolated files and SQLite, the fake login binary (hagency-login-probe)
  Given a prepared managed account and the fake provider login binary
  When the CLI account login --id route runs
  Then begin_account_login allocates the attempt, prepare_login/apply_codex_environment set the retained namespace, and settle_account_login records observed — account_readiness reads ready
  Production caller: hagency::bootstrap::accounts::run

Scenario: A refused login through the production route never reads ready
  Test: native_account_login_route_refused_never_ready
  Level: integration
  Test Double: actual isolated files and SQLite, the fake login binary refusing
  Given a prepared managed account whose provider login the binary refuses
  When the CLI account login --id route runs
  Then settle_account_login records refused — account_readiness reads unknown, never ready
  Production caller: hagency::bootstrap::accounts::run

## Decisions

**Ordering of the sibling slices.** **MA-S2** (the dispatch gate that parks on
`account_readiness_unknown`) follows this slice and consumes the `usable`
predicate exactly as fixed in ADR-114's amendment — matching generation,
`outcome='observed'`, unexpired, read-time evaluation. **MA-S3b** (the
readiness field on the console `AccountRow` and its validator conjunct)
follows it and lands in one commit when a fact exists to serve. This spec
binds neither: its five dispatch-side assertions state the interface MA-S2
implements, and its Forbidden keeps the DTO out.

The retained-oracle scenario
(`native_account_readiness_matches_retained_detect`) runs the retained
probe's own check through the CI lane's existing node step — the
ceiling-vectors shape — so the readiness vocabulary is pinned against
retained, not only against itself.

## Out of Scope

MA-S2's gate implementation, MA-S3b's DTO field, MA-S4 (subscription
materialisation), the login command's console surface (there is none and
none is planned — the login is the operator's own host act), and any
credential reading of any kind.
