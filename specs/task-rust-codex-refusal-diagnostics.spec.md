spec: task
name: "Carry bounded Codex notification refusal diagnostics to the native owner"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-LOCAL-THREE-RUNNER-QUALIFICATION]
tags: [active, rust, codex, diagnostics]
---

## Intent

The isolated local Codex soak stopped on an unsupported event. Preserve the
runtime's existing fixed notification category through the original owned
report so the operator can diagnose the protocol gap without exposing payloads.

## Constraints

- Project only the runtime's fixed category, never arbitrary method names,
  identifiers, arguments, messages or credentials.
- Do not change notification acceptance, process cleanup, custody or retries.
- An absent category remains null; unsupported events remain unknown outcomes.
- Tests use local synthetic peers only; live qualification is a separate operation.

## Allowed changes

- native/hagency-execution/src/operation.rs
- native/hagency/src/bootstrap.rs
- specs/task-rust-codex-refusal-diagnostics.spec.md
- knowledge/decisions/adr-168-codex-refusal-diagnostics.md
- docs/**

## Scenarios

Scenario: The original refusal category survives the operator projection
  Test: native_bootstrap_runtime_observation_projection
  Given a fixed runtime refusal category or no observed refusal
  When the owned report is projected into native diagnostics
  Then only the bounded category or null is returned
  And the existing unknown outcome and diagnostics remain unchanged

Scenario: Raw notification names cannot become diagnostic values
  Test: native_codex_session_hook_notices
  Given a malformed known notification or an arbitrary private method name
  When the session refuses it
  Then the category is the fixed known label or unknown
