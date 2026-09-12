spec: task
name: "Persist native canonical task and dispatch authority"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREE-LAYER-COMPLETION]
tags: [active, rust, tasks, dispatch]
---

## Intent

Implement the M3 canonical task and dispatch kernel in the existing domain
database. Keep real process launch and authenticated Matrix routing in their
later adapters; no fixture may become an externally supplied authority claim.

## Constraints

### Must
- Keep tasks, dispatches, leases, mutation receipts and task outbox in one SQLite transaction owner.
- Require a current started runner capability and exact active task binding for mutations; scope coordinator reads to its own session.
- Bind mutation replay to dispatch, call identifier and content; rollback task, receipt and outbox together on failure.
- Keep task completion explicit and advance its authorization epoch; runner completion cannot mark tasks done.
- Freeze launch input and fence every attempt; retain exclusive resource leases while parked.
- Requeue only unstarted attempts after ownership loss; quarantine unknown started work without automatic replay.
- Use bounded queues, payloads and paged task projections; generate capability secrets with operating-system randomness.

### Must Not
- Do not expose host-only session creation or recovery as unauthenticated API operations.
- Do not launch real runtimes or change deployed services in this task.
- Do not count the kernel as completion of mailbox, delegation or M4–M9 migration.

## Boundaries

### Allowed Changes
- native/**
- ./Cargo.lock
- specs/task-rust-task-dispatch.spec.md
- knowledge/decisions/adr-095-native-state-ownership.md
- docs/**
- .github/workflows/rust.yml

### Forbidden
- Live state, credentials, deployed JS runtime and Matrix server changes.

## Acceptance Criteria

Scenario: Canonical transition policy matches the existing implementation
  Test: native_task_state_machine_matches_javascript
  Given every pair of canonical task states
  When native and JavaScript transition policies are compared
  Then the allowed and refused transitions agree

Scenario: Canonical mutations require the exact current task capability
  Test: native_task_capability_scope
  Given active allocations and started unstarted parked or stale runner capabilities
  When task reads comments heartbeat and transitions are requested
  Then only authorized scoped operations succeed and runtime completion never completes the task

Scenario: Task mutations and replay receipts are atomic
  Test: native_task_receipt_atomicity
  Given identical or changed tool calls and an injected receipt failure
  When a task mutation commits or rolls back
  Then comments task state epoch receipt and outbox remain consistent

Scenario: Dispatch ownership survives failures conservatively
  Test: native_dispatch_recovery
  Given leased started parked and failed-before-start attempts
  When the owner restarts a lease expires or inspected recovery is requested
  Then only unstarted work can be retried automatically and unknown work retains its audit and quarantine

Scenario: Dispatch resources and coordinator visibility stay scoped
  Test: native_dispatch_resource_and_coordinator_scope
  Given multiple sessions and shared or exclusive resources
  When dispatches compete and a coordinator reads its created tasks
  Then live runner limits resource ownership and session visibility hold

## Out of Scope

Mailbox admission, task graphs and delegation, actual runner processes, Matrix
transport, approvals and production cutover remain subsequent migration work.
