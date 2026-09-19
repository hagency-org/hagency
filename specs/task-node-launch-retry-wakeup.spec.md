spec: task
name: "Preserve the router claim cutoff when scheduling launch retries"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [node, scheduler, regression]
---

## Intent

Prevent a queued launch retry from losing its wake when its availability crosses
between the failed claim and the subsequent future-wake lookup.

## Constraints

### Must
- Use the exact eligibility cutoff sampled by the actual claim transaction for its next-wake lookup.
- Preserve existing claim eligibility, capability, lease, input and started-work fencing.
- Keep standalone claim and next-wake APIs compatible and generate checked-in router files through the normal TypeScript build.
- Keep source inventory hashes and offsets synchronized with the reviewed backend change without changing classifications, counts or parity gates.
- Exclude already-due blocked rows from immediate wake scheduling.
- Preserve the original recovery integration assertions, retry policy and thirty-second timeout; print only fixed-stage and bounded numeric/state evidence on failure.
- Keep the confirmed controlled clock race distinct from the unknown cause of original CI34556196694 at7cf0dc0.
- Run exact Vitest selectors separately; agent-spec's Cargo-only lifecycle cannot verify Node scenarios.

### Must Not
- Do not widen timeouts, add retries, weaken assertions, contact live external services or modify live state.
- Do not print arbitrary child errors, messages, paths, environment or capability values in failure diagnostics.

## Decisions

This contract retains the design boundaries in [ADR-011](../knowledge/decisions/adr-011-backend-owned-ephemeral-runner-sessions.md), [ADR-087](../knowledge/decisions/adr-087-node-launch-retry-wakeup.md).

- [JS-only] A combined claim-with-wake API carries the transaction's actual cutoff into the wake lookup without introducing a caller-controlled authority clock.
- [JS-only] Retain parse/lint and explicit parsed boundary evidence separately from direct Vitest scenario results.

## Boundaries

### Allowed Changes
- backend-v2.js
- router/src/store.ts
- router/dist/store.js
- router/dist/store.d.ts
- tests/router-core.test.js
- tests/router-launch-recovery.test.js
- native/fixtures/legacy-inventory.json
- tests/native-migration-inventory.test.js
- specs/task-node-launch-retry-wakeup.spec.md
- knowledge/decisions/adr-087-node-launch-retry-wakeup.md
- docs/agent-knowledge.md
- docs/progress.md

## Acceptance Criteria

Scenario: A retry that becomes due after the claim retains its wake
  Test: claim wake retains a retry that becomes due between claim and wake lookup
  Given a queued retry with an exact future availability and a clock that crosses that boundary after eligibility selection
  When the backend-facing combined claim and wake operation runs
  Then the original claim returns no work and its original cutoff still produces the due retry wake
  And a subsequent claim succeeds without losing input

Scenario: Already-due blocked work cannot create a timer spin
  Test: claim wake excludes already-due blocked work and retains future retries
  Given already-due blocked work and a separate future queued retry
  When the combined claim and wake operation finds no eligible work
  Then only the future retry supplies a wake and blocked work alone supplies no immediate wake

Scenario: Claim results and standalone wake behavior remain compatible
  Test: claim wake preserves successful and refused claim results
  Given successful and capacity-refused claims
  When callers use the combined operation and existing standalone APIs
  Then eligibility and refusal results remain unchanged and existing standalone future filtering remains intact

Scenario: Actual wrapper launch recovery preserves inputs and shutdown fencing
  Test: backend requeues a wrapper that dies before takePayload without losing its input
  Given an actual guardian launching an absent fixture executable
  When the backend records two unstarted failures and reconstructs a persisted retry wake
  Then both failures preserve the same input and stopping the pump prevents a third launch

Scenario: The reviewed backend change preserves source inventory truth
  Test: native_inventory_reproduces_current_sources
  Given the reviewed backend source hash and exact registration offsets
  When the source inventory is regenerated without executing runtime code
  Then the committed inventory matches all source hashes and offsets
  And classifications, counts and parity gates remain unchanged

## Out of Scope

Proving the historical CI timeout's exact stage or cause, native runtime changes,
general scheduler polling, wrapper lifecycle redesign and production activation.
