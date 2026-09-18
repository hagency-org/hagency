spec: task
name: "Preserve per-agent native fleet failure diagnostics"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-LOCAL-THREE-RUNNER-QUALIFICATION]
tags: [active, rust, fleet, diagnostics]
---

## Intent

The first live two-agent file qualification retained one Codex protocol failure
and one uncertain file publication. The factory capabilities rollup discarded
each original worker's bounded diagnostic status. Preserve those observations
on the authenticated operator surface and log fixed failure categories while
the original owners are still alive.

## Constraints

- Use the existing bounded Status projection and exact registered engagement.
- Keep per-agent details behind the existing operator authentication; public
  health/readiness retain only component state words.
- Never expose raw protocol payloads, Matrix snapshot reasons or credentials.
- Preserve unknown outcomes, original SDK custody, leases and explicit recovery.
- No additional send, retry, process restart or authority follows diagnostics.
- Regression tests use synthetic local peers only.

## Allowed changes

- native/hagency/src/bootstrap.rs
- native/hagency/src/bootstrap/driver.rs
- native/hagency/src/bootstrap/fleet.rs
- native/hagency/src/file_service/pipeline.rs
- native/hagency/tests/configured_fleet/mod.rs
- native/hagency/tests/configured_fleet.rs
- specs/task-rust-factory-failure-diagnostics.spec.md
- knowledge/decisions/adr-175-factory-failure-diagnostics.md
- docs/**

## Scenarios

Scenario: A failed factory worker remains distinguishable from its healthy peer
  Test: native_factory_failure_diagnostics
  Given two original registered agent status handles
  When one records a failure and the other remains ready
  Then the private snapshot identifies each original engagement and status
  And observing diagnostics changes neither status nor fleet custody

Scenario: Actual executable diagnostics remain operator authenticated
  Test: native_configured_local_codex_fleet
  Given two agents created by the real configured native executable
  When the operator reads capabilities after their completed tasks
  Then each registered engagement has a bounded status
  And an unauthenticated request cannot read that snapshot

Scenario: Diagnostic labels cannot carry private Matrix content
  Test: native_bootstrap_matrix_failure_projection
  Given an arbitrary private unsafe snapshot reason
  When the error is projected into diagnostics
  Then only the fixed category is returned

Scenario: A handoff refusal preserves its original category before an operation exists
  Test: native_configured_fleet_handoff_diagnostics
  Given two actual initialized factory owners using disposable local provider directories
  When those original directories lose their admitted permissions before task handoff
  Then the protected status retains lost_authority for each original engagement
  And neither task starts or receives fabricated protocol or cleanup observations
  And the original worker refusal remains sticky without a replacement launch
