spec: task
name: "Carry the retained long-task budget through owned native execution"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-LOCAL-THREE-RUNNER-QUALIFICATION]
tags: [active, rust, execution, parity, deadlines]
---

## Intent

Port the retained TS runner's finite twenty-minute execution allowance into the
explicit native host configuration. The live file request arrived roughly twenty
two seconds into the native thirty-second operation; an owner wait cannot fit.
Execution, capability and approval-context bounds must agree without extending
an already running operation, its lease, or a callback deadline.

## Constraints

### Must
- Permit explicit operation budgets from 100 ms through twenty minutes; retain the 10 ms through two-second RPC/write bounds and absolute original deadline.
- Allocate a host-only capability covering that configured budget plus thirty seconds for admission/settlement, with a sixty-second minimum matching existing short runs.
- Preserve the initial sixty-second lease, five-second renewals, current-scope checks, parked maintenance and cancellation cadence. No renewal may exceed the original capability expiry.
- Keep generic runner claims capped at five minutes; only the existing compatible host claim may request the longer bounded capability.
- Permit explicit owner waits plus response reserve up to the existing ten-minute durable approval ceiling, still within the original operation. Keep startup's existing one-second owner-wait default.
- Reject overflow and over-limit inputs and retain all unknown/negative cleanup and task outcomes.
- Run deterministic offline tests and an actual owned subprocess beyond the old thirty-second ceiling.
- Preserve the original fixed startup error in the private Report so early
  failures distinguish guardian startup from runtime deadline failures. This is
  diagnostic only and grants no cleanup, retry or custody-release authority.

### Must Not
- No retries, automatic recovery, new launcher, new credentials, changed sandbox or direct database repair.
- No live network from ordinary tests, and no longer budget inferred from runtime input.

## Boundaries

### Allowed Changes
- native/hagency-core/src/tasks.rs
- native/hagency-execution/src/host.rs
- native/hagency-execution/src/operation.rs
- native/hagency-execution/src/warm.rs
- native/hagency-execution/src/approval/capacity.rs
- native/hagency-execution/tests/owned/**
- native/hagency-runtime/src/bin/hagency-runtime-probe.rs
- native/hagency-store/src/domain/execution.rs
- native/hagency-store/src/domain/approvals/owned.rs
- native/hagency-store/tests/owned_claim.rs
- native/hagency-store/tests/approvals/owned.rs
- native/hagency/src/bootstrap/config.rs
- native/hagency/src/bootstrap/driver.rs
- specs/task-rust-owned-execution-budget.spec.md
- knowledge/decisions/adr-161-native-execution-budget-parity.md
- knowledge/decisions/adr-053-native-owned-dispatch.md
- docs/**

## Acceptance Criteria

Scenario: Host limits and capability cover the configured execution interval
  Test: native_owned_long_operation_limits
  Given short and twenty-minute explicit operation budgets
  When the host validates and derives the capability interval
  Then existing short budgets retain their sixty-second capability and longer budgets stay finite
  And zero overflow and over-limit values are refused

Scenario: Only a compatible host may claim a long-lived capability
  Test: native_owned_claim_long_budget
  Given a current verified session and exclusive workspace
  When the host claims and renews through the configured interval
  Then the lease never exceeds the original capability expiry
  And generic claims and over-limit host claims remain refused

Scenario: Actual owned execution survives the old ceiling
  Test: native_owned_turn_long_lifetime
  Given a real owned native subprocess and a forty-five-second operation
  When its acknowledged turn remains quiet for more than thirty seconds
  Then the original owner completes without early timeout and settles only after full stop

Scenario: A long budget does not delay explicit cancellation
  Test: native_owned_turn_long_cancel
  Given an acknowledged turn with a twenty-minute operation budget
  When the host cancels the original operation
  Then the original process stops promptly and the attempt remains negative

Scenario: Approval custody fits the same original operation
  Test: native_owned_approval_long_budget
  Given a longer original operation and a private owner decision
  When the same callback is parked and answered
  Then its durable context and actual native response work without extending its original deadlines

Scenario: Startup validates the complete budget
  Test: native_bootstrap_approval_wait_bound
  Given explicit operation and owner-wait configuration
  When the host is prepared
  Then owner waits fit both the original operation and durable approval ceiling

Scenario: Long approval contexts cannot be rebound or bypass lease loss
  Test: native_owned_approval_context_long_limit
  Given an original context within the twenty-minute maximum
  When the host tries to rebind it or maintain it after its short lease expired
  Then the same authority checks refuse it without extending custody

## Out of Scope

Automatic task replay, restart route handoff, raising RPC/SQLite wait limits,
pre-activation warm/factory startup budget changes, twenty-minute real-model soak
acceptance, Claude/Octos integration and full parity.
