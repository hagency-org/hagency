spec: task
name: "Resume the retained continuous worker after explicit stopped-outcome resolution"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-LOCAL-THREE-RUNNER-QUALIFICATION]
tags: [active, rust, recovery, custody]
---

## Intent

Live qualification found that ADR165 commits an explicit continuation but the
continuous worker has already exited its dispatch loop. Retain that worker while
its original stopped report awaits an operator decision, then resume ordinary
claiming after observing exact durable resolution.

## Constraints

- Require the original report to prove physical custody ended and its original
  stop inspection was recorded. Missing proof retains the existing refusal.
- The writer read binds dispatch, fence, runner and original capability hash.
  Require the original inspection, settled stop, durable resolution/recovery and
  absence of original leases. Unsettled or merely stopped work cannot resume.
- No automatic retry, synthetic settlement, replay of failed input, new token,
  route generation change or restart is authorized by this observation.
- After resolution release only the exact registered workspace entry. Ordinary
  claims revalidate current routes, accounts, tasks and remaining custody.
- Cancellation ends the wait retaining the original report for normal close.
- Busy or unavailable resolution reads may be repeated; they perform no mutation
  and never release custody without an observed committed decision.
- One-attempt mode is unchanged. All automated tests remain offline.

## Allowed changes

- native/hagency-store/src/domain/stopped_inspection.rs
- native/hagency-store/src/domain_worker.rs
- native/hagency-store/tests/outcome_resolution/**
- native/hagency/src/bootstrap/driver.rs
- native/hagency/tests/bootstrap*
- native/hagency/tests/fixtures/owned_mcp_peer.rs
- specs/task-rust-continuous-outcome-resolution.spec.md
- knowledge/decisions/adr-169-continuous-outcome-resolution.md
- docs/**

## Scenarios

Scenario: Only the original resolved owner may release its local binding
  Test: native_owned_stop_resolution_observation
  Given an original stopped dispatch and recorded inventory
  When the host observes resolution before and after explicit operator action
  Then unresolved or foreign capabilities cannot report resolution
  And a committed decision is observed without rewriting the failed outcome

Scenario: The original continuous process runs a reviewed continuation
  Test: native_continuous_driver_operator_resolution
  Given a real offline subprocess failed with an original stopped-owner receipt
  When the operator resolves it with a distinct continuation
  Then the same service starts the new dispatch through its ordinary claim path
  And it never reruns the failed dispatch or requires a service restart
