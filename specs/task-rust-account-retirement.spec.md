spec: task
name: "Retire an account by logging out and auditing the transition"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, accounts, retirement, logout, audit]
---

## Intent

Bind MA-S4 (ADR-114's amendment) under D-ADR114 observe: retiring an account
performs a **host-observed logout** — the operator runs the provider's own
logout inside the namespace, native records the derived outcome — and audits
the transition in migration 029's logout receipt row. A logout failure
leaves the account readiness `unknown`: never `ready`, never
retired-as-clean. The store owns the fact; nothing here drives the logout
or reads a credential.

## Constraints

### Must
- Add migration 029's logout receipt row (created `IF NOT EXISTS`): the transition's audit record — account id, the transition clock, and the derived readiness state the namespace reached.
- Record the logout's observed outcome on `retire_account`'s `active → retired` transition: `unknown` when the logout could not be observed (failure, refusal, unclassifiable), the observed word otherwise — the store's own verdict, never an unobserved success.
- Leave a failed logout's readiness `unknown`: the MA-S1 `usable` predicate returns unknown for an absent/expired fact, the MA-S2 gate parks any dispatch with `account_readiness_unknown`, and the MA-S3b DTO serves `unknown` — the account is never `ready` and never recorded as a clean retirement.
- Keep the retire wrapper's `active → retired` state transition and its fence-and-unpublish order unchanged.

### Must Not
- Do not drive the logout from native code, capture the provider's stdout, or read a token/auth file — the logout is the operator's own host act, exactly as the login is.
- Do not store any credential, token or session material in the audit row — it records the transition and the derived readiness, nothing else.
- Do not `ALTER` `managed_accounts`'s columns or its four-state CHECK; migration 029 adds a receipt row only.
- Do not change the one-attempt materialise discipline, the `account_identity_key` singleton, the `resource_accounts` immutability triggers, or the close path.
- Do not gate any scenario by OS or feature in the binding set.

## Boundaries

### Allowed Changes
- native/hagency-store/src/domain/accounts.rs
- native/hagency-store/src/migrations/029-account-logout-receipt.sql
- native/hagency-store/src/domain.rs
- native/hagency-store/src/lib.rs
- native/hagency-store/tests/accounts.rs
- native/hagency-store/tests/
- native/hagency/src/bootstrap/accounts.rs (the offline CLI retire path: the stores retire_account now takes the host-observed logout outcome, so this arm threads it)
- specs/task-rust-account-retirement.spec.md
- knowledge/decisions/adr-114-native-managed-account-binding.md
- docs/progress.md

### Forbidden
- Live providers, live credentials, deployed state.
- mockup/**; native/hagency/src/console/** (MA-S3a's retire route is unchanged); the migration chain beyond 029; any login-driving code path (MA-S1's).

## Acceptance Criteria

Scenario: Retiring an account logs out and audits the transition
  Test: native_account_retire_logs_out_and_audits
  Level: integration
  Test Double: actual isolated files and SQLite; an active account with a recorded login observation
  Given an active account whose provider login was observed
  When retire_account runs the host-observed logout and transitions the row
  Then the row reads retired and a logout receipt row exists carrying the account id, the clock and the observed readiness
  And the resources the account published are unpublished in the same transaction

Scenario: A logout failure leaves readiness unknown, never ready and never retired-as-clean
  Test: native_account_retire_logout_failure_is_unknown
  Level: integration
  Test Double: actual isolated files and SQLite; a logout that cannot be observed
  Given an active account whose logout fails or is unclassifiable
  When retire_account runs and the readiness fact is read
  Then the account reads retired with readiness unknown and the audit row records the unknown outcome
  And no surface reads the account as ready and no clean-retirement claim is recorded
  And no credential or token byte reaches the audit row or any log

## Decisions

**Depends on MA-S1 (migration 028) and MA-S3a's retire wrapper.** The logout
receipt row is migration 029 (the ledger's MA-S4 number, after 028); the
readiness fact it audits is MA-S1's, and the retire entry point it augments
is MA-S3a's `retire_account` (`accounts.rs:665-686`). Neither exists on this
branch yet — this spec binds against their landed shapes and is
unimplementable before them, by design.

**The logout is host-observed, never native-driven** — the same D-ADR114
posture the login took: native records the derived outcome and never runs,
captures or stores the provider session.

## Out of Scope

The login and its fact (MA-S1), the dispatch gate (MA-S2), the DTO field
(MA-S3b), the retire route's own `active → retired` mechanics (MA-S3a,
unchanged), and any credential reading of any kind.
