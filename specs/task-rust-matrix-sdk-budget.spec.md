spec: task
name: "Configure the original Matrix SDK budget before native startup"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-MATRIX-DM-PRIVACY]
tags: [active, rust, matrix]
---

## Intent

The authorized live ADR178 fleet used1s shared request pacing. Fresh approval
enrollment exhausted the default20s SDK budget and cancelled before any task.
Expose the existing bounded SDK budget as explicit host startup configuration.

## Constraints

- Optional matrix_sdk_timeout_ms sets the original Limits.sdk before owners
  exist. Preserve the20s default; admit only the existing10..60000ms range.
- The same frozen limits reach the coordinator, approval collector and factory.
- Never extend a running deadline, alter model/task/approval-response budgets,
  replay a possible write, rearm incomplete enrollment or reuse unknown custody.
- Preserve failed root7 state and its observed process exit. Live tests remain
  explicit private operator actions; ordinary tests use local synthetic TLS.
- No UI/i18n strings, formatter, commit, PR or production cutover.

## Allowed changes

- native/hagency/src/bootstrap/config.rs
- native/hagency/tests/configured_fleet.rs
- native/hagency/tests/configured_fleet/**
- specs/task-rust-matrix-sdk-budget.spec.md
- knowledge/decisions/adr-179-matrix-sdk-budget.md
- docs/**

## Scenarios

Scenario: Host SDK budget is bounded and independent of task execution
  Test: native_matrix_sdk_budget_configuration
  Given an omitted, valid or invalid host SDK timeout
  When configuration creates the original Matrix limits
  Then omission retains20s and invalid values refuse before startup
  And HTTP and model operation budgets remain independent

Scenario: Paced enrollment completes only within its original selected budget
  Test: native_configured_paced_startup
  Given a real configured service and fresh synthetic approval account at1s pacing
  When the original SDK budget is default20s or explicit60s
  Then the short original attempt refuses without task execution
  And the separate longer attempt completes original enrollment and closes
  And neither attempt retries enrollment writes or creates an agent task
