spec: task
name: "Observe original usage failures before releasing fixture locks"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-EXECUTION-AUTHORIZATION]
tags: [active, rust, approvals, testing, ownership]
---

## Intent

Correct two original 1baa80d macOS CI failures whose fixed sleeps did not prove
that native usage reached the SQLite writer while its lock was held. Preserve
the original failed results and the production refusal of an unacknowledged slot.

## Constraints

- Preserve production behavior, SQLite busy allowance, operation/owner/response
  deadlines, original child, pending writer future and unknown refusal assertions.
- A test-only observer may signal only after the actual original usage writer
  returns an error. It cannot inject an outcome or bypass the writer.
- Hold the actual lock until that observation. Keep the original maintenance or
  begin receipt pinned; release it only after verifying the failed usage slot.
- Move the integration unknown-slot selector into existing owned library support
  to use private test seams, preserving its actual-child and exact count checks.
- No live services, schema, dependencies, runtime protocol or production cutover.

## Boundaries

### Allowed Changes
- native/hagency-execution/src/approval.rs
- native/hagency-execution/src/approval/control.rs
- native/hagency-execution/src/approval/observations.rs
- native/hagency-execution/src/approval/state.rs
- native/hagency-execution/tests/support/approval_loss.rs
- native/hagency-execution/tests/owned/approvals.rs
- specs/task-rust-owned-approval-usage-receipts.spec.md
- docs/agent-knowledge.md
- docs/progress.md

### Forbidden
- All paths outside this exact partition and all live operations.

## Acceptance Criteria

Rule: retained-usage-receipt — No later callback passes an unacknowledged usage slot

Scenario: Successful begin cannot conceal a usage failure
  Test: native_owned_approval_usage_successful_control
  Given a real committed begin whose original receipt is held
  When an actual usage write fails under the retained SQLite lock
  Then exactly one usage observation and the original callback remain retained without response bytes or later callbacks

Scenario: Maintenance cannot conceal a usage failure
  Test: native_owned_approval_usage_unknown_slot
  Given a real maintenance receipt and a pending owner request
  When the actual first usage write fails before the lock is released
  Then the original slot remains pending with zero acknowledgements and no later usage or response bytes

Scenario: Existing successful usage remains usable
  Test: native_owned_approval_usage
  Given the real owned callback and usage stream
  When actual writer receipts succeed
  Then all observations remain ordered and acknowledged without synthetic Applied

## Out of Scope

Windows path admission is governed by a separate ADR116 contract. Local success
does not replace the original hosted failures or qualify actual Windows execution.
