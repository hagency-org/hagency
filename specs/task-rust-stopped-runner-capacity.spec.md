spec: task
name: "Separate proven stopped runner occupancy from unresolved task custody"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-LOCAL-THREE-RUNNER-QUALIFICATION, REQ-DASHBOARD-RUNNER-PROJECTION, REQ-THREAD-SCOPED-SESSIONS]
tags: [active, rust, codex, recovery, capacity]
---

## Intent

The working TS runner retires its live entry after cleanup; an unresolved task
still quarantines its workspace. Native claim currently charges every unresolved
dispatch against the global live-process limit, including owners whose full stop
was durably observed. Use ADR162's exact-attempt observation for that distinction.
Do not change the limit or clear any task, stop, workspace or input state.

## Constraints

- Count leased/started/parked dispatches, and unresolved attempts without an
  original matching stopped-owner receipt. Unknown physical owners still count.
- A receipt from a different dispatch/fence never frees occupancy.
- The original workspace lease and dirty flag, session quarantine, canonical
  task and stop remain unchanged. All existing claim eligibility checks remain.
- Only a distinct otherwise eligible session/workspace may use the vacant slot.
- Original runtime failure classification and explicit operator recovery remain
  unchanged. Do not manufacture legacy stop evidence or deploy this to bypass a
  live attempt whose receipt is absent.
- Tests are offline. Include an actual failed owned process followed by an
  actual independently owned process, not only SQL fixtures.

## Allowed changes

- native/hagency-store/src/domain/execution.rs
- native/hagency-store/tests/owned_claim.rs
- native/hagency-execution/tests/owned/inspection.rs
- knowledge/decisions/adr-148-operator-recovery-resume.md
- knowledge/decisions/adr-162-native-stopped-owner-inspection.md
- knowledge/decisions/adr-163-native-stopped-runner-capacity.md
- this spec
- docs/**

## Scenarios

Scenario: Stopped physical occupancy differs from unresolved workspace custody
  Test: native_owned_claim_stopped_capacity
  Given one started dispatch fenced after failure and a disjoint queued task
  When its exact original stopped-owner inspection is retained
  Then the disjoint task can claim at the unchanged one-runner limit
  And the failed task and its workspace remain quarantined and leased
  And missing or mismatched receipt evidence cannot free a slot

Scenario: Real original owners establish occupancy release
  Test: native_owned_stopped_capacity_real_process
  Given an actual owned process that fails and stops fully
  When its original worker records inspection and an independent task runs
  Then the second actual process completes without changing the first outcome

## Remaining work

Explicit operator outcome resolution, old attempts lacking stop proof, live
file/approval qualification, restart routing and the complete Codex soak.
