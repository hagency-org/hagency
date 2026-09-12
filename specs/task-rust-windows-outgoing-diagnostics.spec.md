spec: task
name: "Diagnose the original Windows Matrix outgoing failures"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, ci, diagnostics]
---

## Intent

Run the failing Matrix library selectors in a bounded failure-only diagnostic
without replacing the original Windows full-suite failure verdict.

## Constraints

### Must
- Preserve the normal full workspace suite and its fatal failure.
- Limit the additional step to five minutes after actual Windows test failure.
- Run the real outgoing selector with the same workspace all-target feature selection and unchanged assertions and production deadlines.
- Keep the existing separate Matrix and Palpo transport diagnostic.
- Treat any diagnostic success as separate evidence instead of qualification of the original run.

### Must Not
- Do not add automatic success retries continue-on-error weakened assertions or modified runtime timing.
- Do not infer disk contention or a production defect from a missing HTTP request alone.

## Boundaries

### Allowed Changes
- .github/workflows/rust.yml
- specs/task-rust-windows-outgoing-diagnostics.spec.md
- docs/progress.md
- docs/agent-knowledge.md

## Acceptance Criteria

Scenario: The exact original outgoing library path remains exercised
  Test: native_matrix_outgoing_plain_final_actual_https_formatted_and_idempotent_receipt
  Given an actual local authenticated Matrix sender fixture
  When its unchanged library selector runs
  Then the outgoing claim and actual final HTTP acceptance remain required

Scenario: Unknown outcomes still prevent unsafe replay
  Test: native_matrix_outgoing_recovery_lost_http_and_begin_do_not_replay
  Given an interrupted begin or lost HTTP acceptance response
  When the diagnostic runs the unchanged original recovery fixture
  Then a later diagnostic cannot create a new send or replace original uncertainty

## Out of Scope

Explaining the historical Windows cause before evidence exists, production fixes,
loosening deadlines, declaring original failures passing and release qualification.
