spec: task
name: "Execute durable task graphs through canonical tasks and peer inputs"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREE-LAYER-COMPLETION]
tags: [active, rust, graphs]
---

## Intent

Connect bounded graph planning to the existing task, message, dispatch and
inspection stores without introducing another task completion authority.

## Constraints

### Must
- Create the graph and all canonical node tasks under a current creator capability in one transaction.
- Resolve assignees to exact current internal participant sessions in the graph conversation and project generation.
- Admit each ready assignment and graph progress atomically with an immutable peer input and stable identity.
- Require the exact node task and admitted assignment when dispatching graph work.
- Accept successful node results only for the exact canonical done epoch and current task or inspected report capability.
- Keep repeated results content-bound and prevent duplicate dependent assignments after restart or lost responses.
- Keep graph reads and cancellation scoped to the exact creator session; workers receive only assigned input and explicit dependency results.
- Cancel graph work on membership retirement or allocation revocation while preserving tasks history and unresolved process custody.
- Preserve existing dependency ordering conditions skips and failure propagation.
- Keep workspace resource selection and actual process inspection host-owned.

### Must Not
- Do not complete canonical tasks or the parent task from graph result text or graph terminal state.
- Do not automatically retry unknown started work.
- Do not accept runtime owner identity process evidence or workspace grants in graph commands.
- Do not launch live models or contact homeservers.

## Boundaries

### Allowed Changes
- native/**
- specs/task-rust-durable-graphs.spec.md
- knowledge/decisions/adr-031-native-task-graph-custody.md
- docs/**

### Forbidden
- Live services, deployed state and the original dirty checkout.

## Acceptance Criteria

Scenario: Graph admission and dependency activation are atomic
  Test: native_graph_transactions
  Given a current creator and an injected task or assignment write failure
  When graph creation and dependency results are committed or retried
  Then graph tasks progress input and receipts commit together without duplicate assignments

Scenario: Graph authority follows exact current sessions
  Test: native_graph_authority
  Given foreign allocations stale capabilities participant callers and retired sessions
  When callers create read cancel or report graph work
  Then only the current exact authorized creator or bound node task receives the corresponding permission

Scenario: Graph dispatch requires its admitted canonical work
  Test: native_graph_dispatch_scope
  Given pending ready and completed graph nodes with independent resource scopes
  When host dispatch admission claims and starts their work
  Then only the matching node task and immutable assignment may execute and runtime fields cannot bypass readiness

Scenario: Completed results survive inspected recovery
  Test: native_graph_result_recovery
  Given explicit canonical completion and an uncertain original or report dispatch
  When the host inspects recovery and the bound runner reports a result
  Then the exact completed epoch is preserved dependent work is admitted once and the parent task remains unchanged

Scenario: Cancellation preserves unresolved execution custody
  Test: native_graph_cancellation
  Given queued leased started and parked graph work plus other active tasks
  When a creator cancels membership changes or an allocation is revoked
  Then obsolete work is fenced live resource custody survives until inspection and unrelated work remains available

Scenario: Graph outcomes preserve dependency semantics
  Test: native_graph_dependency_outcomes
  Given dependent conditional independent and failed nodes
  When canonical outcomes advance the durable graph
  Then ordering fractional result conditions skips and failure propagation match the existing planner without fabricating task completion

Scenario: Private HTTP exposes only scoped graph commands
  Test: native_runner_http_graphs
  Level: Integration
  Test Double: Host-issued fixture capabilities and an in-process Salvo service with a real SQLite domain writer
  Given authenticated runners and forged graph identity or inspection fields
  When graph creation reads results and cancellation use the private API
  Then validated commands reach the single writer and host process authority remains inaccessible

## Out of Scope

Actual model processes, Matrix delivery, final reply transport, effective sandbox
permissions and production cutover remain later migration gates. Native fixtures
exercise real repository and HTTP behavior with host-controlled runner capabilities.
