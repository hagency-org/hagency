spec: task
name: "Operator resolution of an inspected native stopped outcome"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-LOCAL-THREE-RUNNER-QUALIFICATION, REQ-THREE-LAYER-COMPLETION]
tags: [active, rust, recovery, console, custody]
---

## Intent

Port the retained TypeScript inspection-token workflow and its three operator
actions: continue with a distinct instruction, accept_completed, keep_blocked.
Require the original ADR162 host stop receipt; an operator token cannot replace
missing process-stop evidence. Preserve the original unknown attempt.

## Constraints

- Inspection issuance and resolution require console lifecycle authority and
  bind the addressed engagement in the serialized writer transaction.
- Issue random single-use credentials, store only their hash, expire in 1..60
  minutes (15 by default), and bind the receipt plus task and current scope.
  Revalidate custody, original scope, current route, sibling owners and media
  effects before resolution. Changes since inspection refuse atomically.
- Consume the token, settle the original stop, mutate the task or enqueue a
  distinct replacement, and store a content-bound request receipt in one commit.
  Exact replay survives expiry/restart; changed content or a second resolution
  conflicts. Never store the plaintext token in the receipt or operator evidence.
- Acceptance marks canonical Done once but grants no runner output, graph result
  or final-reply receipt. Blocking enqueues no work. Both preserve original input
  assignments without marking them processed. Supersede older queued work.
- Graph tasks retain their separate graph command authority: terminal operator
  actions refuse graph tasks in this slice; continuation uses existing graph guards.
- Bound issuance, clean expired unused tokens, retain consumed resolution evidence.
  Schema 35 upgrades preserve existing data and do not manufacture stop evidence.
- ADR164's explicit receipt-bound continuation remains compatible; it is not a
  bearer-token API. UI controls and live service changes are outside this slice.

## Allowed changes

- native/hagency-store/src/**
- native/hagency-store/tests/**
- native/hagency/src/console/agents.rs
- native/hagency/tests/console/agents.rs
- native/hagency-execution/tests/owned/inspection.rs
- knowledge/decisions/adr-165-native-outcome-resolution.md
- this spec
- docs/**

## Scenarios

Scenario: Each operator action commits once and preserves independent evidence
  Test: native_outcome_resolution_actions
  Given an owned failure with its original stopped-owner receipt
  When an operator resolves a fresh inspection credential
  Then continue transfers inputs or acceptance marks Done or blocking marks Blocked
  And original unknown outcome and unprocessed evidence remain intact

Scenario: Stale or conflicting authority cannot release custody
  Test: native_outcome_resolution_refusals
  Given an unresolved stopped owner and a scoped inspection token
  When token expiry identity scope task custody or media evidence conflicts
  Then the transaction refuses without consuming the token or releasing custody

Scenario: Inspection issuance is private bounded and repeatable after expiry
  Test: native_outcome_inspection_capacity
  Given an unresolved stopped dispatch
  When operators issue bounded short-lived inspection credentials
  Then only hashed secrets persist and expired unused inspections can be replaced

Scenario: A failed final receipt rolls back the entire operator decision
  Test: native_outcome_resolution_rollback
  Given any supported operator action with valid inspection authority
  When the final durable resolution insert fails after custody and task changes
  Then stop settlement task events input transfer and token consumption all roll back

Scenario: Console mutations enforce lifecycle scope and agent identity
  Test: native_console_outcome_resolution
  Given a native stopped dispatch with an original host receipt
  When requests run through inspection and resolution routes
  Then lifecycle operators resolve once and read-only or foreign requests refuse
  Production caller: hagency::console::agents::resolve_stopped_dispatch

Scenario: Schema upgrade retains unresolved custody and replay evidence
  Test: native_outcome_resolution_schema35_upgrade
  Given a schema 34 database with original stopped-owner evidence
  When the native store upgrades and reopens
  Then existing custody is preserved and new resolution tables start empty

Scenario: A real failed process supports explicit operator resolution
  Test: native_owned_outcome_resolution_real_process
  Given an actual owned subprocess failed and its original host recorded cleanup
  When a lifecycle operator resolves the retained inspection through the writer
  Then continuation executes a distinct process or acceptance and blocking execute none
  And the failed process never gains accepted output or a fabricated final reply
