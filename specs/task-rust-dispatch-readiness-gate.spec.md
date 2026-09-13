spec: task
name: "Park dispatch consumption on unknown account readiness"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, dispatch, accounts, readiness, gate]
---

## Intent

Bind MA-S2: a dispatch over a managed scope is consumed only when the bound
account's readiness fact (MA-S1, migration 028) is **observed and unexpired**;
otherwise it **parks** with the named reason `account_readiness_unknown` and
is re-evaluated at read time when a new fact settles — never retried in a
loop. The gate lives in the two places ADR-053's amendment fixes: the
queued-dispatch selector (`domain/execution.rs:743`) and the Host admission
(`hagency-execution/src/host.rs:251-262`), which re-checks the same predicate
at consumption rather than trusting the selector.

## Constraints

### Must
- Add one conjunct to the selector and the same re-check to the Host admission: the account's latest usable fact — matching generation, `outcome='observed'`, unexpired — evaluated at read time; a read never writes and never caches.
- Park with the named reason `account_readiness_unknown` using the existing parked-update shape (`approvals.rs:152-161`); the row, its inputs and its custody survive untouched.
- Re-evaluate a parked dispatch on the next selector pass after a new readiness fact settles — event-driven, never a timer or retry loop.
- Make the park visible as a state with its reason in the audit trail.

### Must Not
- Do not fail the dispatch opaquely, drop it, or rewrite its custody — a park is a refusal of now, not of the dispatch.
- Do not infer readiness at the gate: directory existence, file listings and model selection prove nothing; only MA-S1's receipt does.
- Do not add a launcher, runner capability, workspace access, or any console surface — the gate is store-and-host only.
- Do not change `one_live_session`, the `dispatch_stops` DDL, the fence kernel, the settlement rule, or the close path.
- Do not gate any scenario by OS or feature in the binding set.

## Boundaries

### Allowed Changes
- native/hagency-store/src/domain/execution.rs
- native/hagency-store/tests/
- native/hagency-execution/src/host.rs
- native/hagency-execution/tests/
- specs/task-rust-dispatch-readiness-gate.spec.md
- knowledge/decisions/adr-053-native-owned-dispatch.md
- knowledge/decisions/adr-114-native-managed-account-binding.md
- docs/progress.md

### Forbidden
- Live providers, live agents, credentials, deployed state.
- native/hagency/src/console/**; mockup/**; native/hagency/src/bootstrap/**; the migration (028 is MA-S1's, landed before this slice).

## Acceptance Criteria

Scenario: Dispatch requires an observed, unexpired account
  Test: native_dispatch_requires_ready_account
  Level: integration
  Test Double: actual isolated files and SQLite; an owned scope bound to accounts in every readiness shape
  Given owned scopes bound to accounts whose readiness is unknown, expired, uncertain and observed-unexpired
  When the queue is drained and the Host is asked to admit
  Then only the observed-unexpired scope is selected and starts no child for any other
  And the Host admission refuses the same rows the selector refuses — neither trusts the other's cache

Scenario: A dispatch with unknown readiness parks with the named reason
  Test: native_dispatch_parks_with_named_reason_on_unknown_readiness
  Level: integration
  Test Double: actual isolated files and SQLite; one dispatch over an unobserved account
  Given a queued dispatch whose bound account has no usable fact
  When the selector runs
  Then the dispatch parks with the reason account_readiness_unknown and its inputs and custody are unchanged
  And the park is not a failure — the row is not dropped, errored, or rewritten

Scenario: A parked dispatch resumes when readiness is observed
  Test: native_dispatch_resumes_when_readiness_is_observed
  Level: integration
  Test Double: actual isolated files and SQLite; the parked dispatch and a later login receipt
  Given the parked dispatch and an account whose login is then observed unexpired
  When the next selector pass runs
  Then the dispatch becomes dispatchable and completes without rewrite
  And an expired or uncertain later fact parks it again with the same named reason

Scenario: The park is read-time evaluation, not a retry loop
  Test: native_dispatch_park_is_read_time_not_a_retry_loop
  Level: integration
  Test Double: actual isolated files and SQLite; a parked dispatch with no new fact
  Given a dispatch parked on unknown readiness whose account gains no new fact
  When many selector passes and arbitrary time intervals elapse
  Then the dispatch parks once and stays parked — no retry storm, no busy re-check, no state churn
  And the only re-evaluation trigger is a newly settled readiness fact

Scenario: The park is visible in the audit table
  Test: native_dispatch_park_is_visible_in_the_audit_table
  Level: integration
  Test Double: actual isolated files and SQLite; the parked dispatch's audit rows
  Given a dispatch that parked on unknown readiness and later resumed
  When the audit trail is read
  Then the park appears as a state with its reason account_readiness_unknown and its interval is derivable
  And the later dispatch leaves no orphaned park record

## Decisions

**This slice depends on MA-S1 landing first** — the gate consumes the fact
migration 028 records, and there is nothing to evaluate until it exists. The
selector `native_dispatch_requires_ready_account` is **moved here from MA-S1's
spec in this same commit** so one spec owns it (its scenario text is carried
over unchanged in substance); MA-S1's spec keeps the fact-recording five.

The two-file boundary is deliberate: the selector and the Host admission must
carry the identical predicate, and one spec owning both keeps them from
drifting — the exact failure mode a cached selector admits.

## Out of Scope

MA-S1's recording half (landed), MA-S3b's console DTO field, MA-S4
(subscription materialisation), any operator surface for the park (the audit
table is the visibility), and any readiness inference of any kind.
