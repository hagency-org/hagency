spec: task
name: "Separate elapsed lease evidence from concurrent worker admission"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, windows, fixtures]
---

## Intent

Exercise concurrency inside a valid lease and preserve diagnostic evidence when
an unchanged shutdown deadline returns an unknown result.

## Constraints

### Must
- Keep the exact one-claim concurrency assertion within an explicitly requested valid lease.
- Demonstrate actual writer elapsed-time expiry without overlapping Start authority.
- Retain both shutdown timeouts and original failure result with fixed phase diagnostics only on failure.
- Record original CI failures separately from subsequent qualification.

### Must Not
- Do not change production deadlines lease policy claim fencing or shutdown behavior.
- Do not infer the historical shutdown cause or treat a diagnostic rerun as a passing original suite.

## Boundaries

### Allowed Changes
- native/hagency-store/src/outbound/tests.rs
- native/hagency-store/src/domain_worker.rs
- knowledge/decisions/adr-075-native-windows-worker-evidence.md
- specs/task-rust-worker-fixture-evidence.spec.md
- docs/plan.md
- docs/progress.md
- docs/agent-knowledge.md

## Acceptance Criteria

Scenario: Concurrent commands admit exactly one unexpired claim
  Test: native_outbound_custody_worker
  Given concurrent actual bounded worker commands inside one explicit lease
  When activation receipt and claim transactions serialize
  Then receipts remain identical and exactly one claim is admitted

Scenario: Elapsed short lease permits replacement but never duplicate Start
  Test: native_outbound_custody_worker_elapsed_lease
  Given an unstarted100ms claim and200ms of host submission age
  When the real writer processes a new attempt
  Then the expired ticket fails and the replacement can Start only once

Scenario: Cancelled queued publication remains refused
  Test: native_owned_completion_queued_cancellation
  Given actual queued completion cancelled before writer admission
  When the writer resumes and the original shutdown executes
  Then no final reply is admitted and shutdown errors retain their original fixed snapshot

Scenario: Expired queued publication remains refused
  Test: native_owned_completion_queued_deadline
  Given actual queued completion beyond its deadline
  When the writer resumes and the original shutdown executes
  Then no final reply is admitted and resources remain owned until inspected cleanup

## Out of Scope

Historical Windows root-cause proof live deployment weaker deadlines and retrying
unknown writes or treating eventual cleanup as an earlier acknowledged shutdown.
