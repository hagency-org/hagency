spec: task
name: "Preserve native task graph dependency policy"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREE-LAYER-COMPLETION]
tags: [active, rust, graph]
---

## Intent

Port bounded task graph planning and condition semantics using executable vectors
from the existing JavaScript policy before connecting durable graph dispatch.

## Constraints

### Must
- Reject empty graphs duplicate identifiers missing dependencies and dependency or condition cycles.
- Preserve JavaScript primitive equality truthiness missing-versus-null and blocked nested-path behavior for conditions.
- Wait for unresolved dependencies; propagate failed or cancelled dependencies and skip false conditions deterministically.
- Preserve graph node order and publish dispatch candidates only in an uncommitted transition.
- Bound graph size result bytes and JSON nesting while retaining fractional result values.
- Keep host node observations separate from runtime graph definitions.

### Must Not
- Do not treat planning a dispatch as durable message admission or canonical task completion.
- Do not replace canonical task truth with independently writable graph completion.
- Do not launch runtimes or send external messages.

## Boundaries

### Allowed Changes
- native/**
- .github/workflows/rust.yml
- specs/task-rust-graph-policy.spec.md
- docs/**

### Forbidden
- Live services, existing JavaScript graph policy and runtime credentials.

## Acceptance Criteria

Scenario: Conditions match JavaScript policy
  Test: native_graph_condition_vectors
  Given local JavaScript-derived condition vectors including fractional values missing fields and blocked paths
  When the native graph evaluates each condition
  Then readiness matches the existing policy without structural object equality

Scenario: Graph transitions match dependency policy
  Test: native_graph_transition_vectors
  Given JavaScript-derived graph progress and dependency outcomes
  When native planning advances or cancels a graph
  Then dispatch candidates ordering failure propagation skips and terminal state match

Scenario: Graph definitions and results are bounded
  Test: native_graph_validation
  Given malformed cyclic oversized or forged graph input
  When definitions observations and results are validated
  Then invalid input is rejected and valid fractional results remain intact without launching runtimes or sending external messages

## Out of Scope

Durable graph storage, canonical-task observation authentication, mailbox/group
admission, runtime API routing and actual graph execution remain subsequent work.
