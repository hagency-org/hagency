spec: task
name: "Expose capability-scoped native runner task APIs"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREE-LAYER-COMPLETION]
tags: [active, rust, runner, api]
---

## Intent

Provide the narrow M3 service interface for disposable runners. Runtime adapters
use structured task operations over a private local API rather than editing task
repositories. Keep host lifecycle and operator configuration out of runner scope.

## Constraints

### Must
- Require an exact loopback authority and a complete current started runner capability on every request.
- Separate runner authorization from operator configuration authority and reject duplicate or URL-supplied credentials.
- Restrict task reads writes comments and inbox pages to the current capability scope.
- Use bounded bodies and host clocks and recheck authority at the actual operation after reading a request body.
- Preserve content-bound mutation replay and fail closed on unknown actions or identity fields.
- Return private no-store responses without credentials or internal storage error details.

### Must Not
- Do not expose session admission dispatch claiming process recovery or resource administration through runner credentials.
- Do not let final output or HTTP success invent task completion.
- Do not call live homeservers or models in tests.

## Boundaries

### Allowed Changes
- native/**
- specs/task-rust-runner-api.spec.md
- knowledge/decisions/adr-095-native-state-ownership.md
- docs/**

### Forbidden
- Live state, credentials, deployed JS runtime and Matrix server changes.

## Acceptance Criteria

Scenario: The writer evaluates authority after queueing
  Test: native_runner_clock_after_queue
  Given a valid capability whose request waits behind another domain command
  When the lease expires before the queued command executes
  Then the writer rejects the operation using its current clock

Scenario: Runner HTTP authority is isolated
  Test: native_runner_http_authority
  Given valid missing stale duplicated browser or operator credentials
  When the native runner or resource routes are called
  Then only current started runner capabilities authorize the runner surface

Scenario: Structured task operations remain scoped and replayable
  Test: native_runner_http_task_lifecycle
  Given a started runner bound to one canonical task
  When task reads comments heartbeat wait resume and done calls are submitted
  Then exact scoped mutations succeed with host-owned fields and changed replay content fails

Scenario: Input pages and request limits are enforced
  Test: native_runner_http_inbox_and_limits
  Given frozen input and later messages together with malformed or oversized bodies
  When the runner reads or attempts an unsupported operation
  Then it sees only its frozen input and invalid requests have no task side effects

## Out of Scope

Native process launch, MCP transport, task graph/delegation and live Matrix/Palpo
workflows remain subsequent migration work. This API is not full production parity.
