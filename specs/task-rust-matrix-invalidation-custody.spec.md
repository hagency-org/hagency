spec: task
name: "Retain already enqueued authoritative Matrix invalidations"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, matrix, custody]
---

## Intent

Apply the original enqueued negative Matrix observation even when its caller or
reply receiver disappears before the domain writer executes it.

## Constraints

### Must
- Retain only already enqueued transport and room invalidations through the original finite domain queue and byte permit.
- Preserve exact host supplied expected identity and generation checks and existing transactional retirement behavior.
- Preserve ordinary calls' receiver cancellation check before execution.
- Return the original timeout uncertainty independently from eventual queued mutation evidence.
- Keep pre-enqueue observation custody and failed admission outside this retained execution guarantee.

### Must Not
- Do not retry add queues maps services or execution authority change deadlines or mutate positive observations through the retained call mode.
- Do not treat missing acknowledgements or queue admission failures as successful persisted retirement.
- Do not use live hosts credentials or change production services.

## Boundaries

### Allowed Changes
- native/hagency-store/src/domain_worker.rs
- native/hagency-store/tests/matrix_invalidation/worker.rs
- knowledge/decisions/adr-091-native-matrix-invalidation-custody.md
- specs/task-rust-matrix-invalidation-custody.spec.md
- docs/agent-knowledge.md
- docs/progress.md

## Acceptance Criteria

Scenario: Dropped transport invalidation still retires the original incarnation
  Level: integration
  Test Double: actual SQLite domain writer held by a finite private gate
  Test: native_matrix_invalidation_transport_drop
  Given an authenticated fixture transport and an enqueued exact negative observation
  When its caller is dropped before writer pickup
  Then the original transport and its current route are unavailable after writer release

Scenario: Dropped shared room invalidation still retires the original room
  Level: integration
  Test Double: actual SQLite domain writer held by a finite private gate
  Test: native_matrix_invalidation_room_drop
  Given an available shared room and an enqueued exact room invalidation
  When its caller is dropped before writer pickup
  Then original shared room scope and its route are unavailable after writer release

Scenario: A delayed negative observation cannot retire replacement scope
  Level: integration
  Test Double: original writer applies actual replacement observations before queued invalidation
  Test: native_matrix_invalidation_replacement
  Given an original enqueued transport or room invalidation
  When a newer incarnation exists when the original writer executes it
  Then exact expected identity and generation checks preserve replacement availability

Scenario: Original reply timeout remains uncertain while queued retirement completes
  Level: integration
  Test Double: unchanged real reply timeout and separately released SQLite writer
  Test: native_matrix_invalidation_timeout
  Given an original invalidation already admitted to the finite queue
  When its reply wait times out before writer pickup
  Then its result remains OutcomeUnknown and a separate later query observes retirement

Scenario: Cancelled positive and work producing calls do not execute
  Level: integration
  Test Double: actual queued positive observation and canonical task creation
  Test: native_matrix_invalidation_cancelled_work
  Given ordinary mutation calls waiting behind the held domain writer
  When their receivers are dropped before pickup
  Then no positive replacement or canonical task is created

## Out of Scope

Retaining observations before successful enqueue, Matrix HTTP ownership, SDK or
upload coordination, new invalidation identities, retries, schema changes,
native Windows runtime qualification and replacement of original CI verdicts.
