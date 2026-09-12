spec: task
name: "Bind one owned native Codex operation to durable dispatch custody"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREAD-SCOPED-SESSIONS, REQ-THREE-LAYER-COMPLETION]
tags: [active, rust, runner, custody]
---

## Intent

Connect exact host dispatch authority to the existing owned native child pipes in
one bounded operation, while keeping production workspace and sandbox qualification open.

## Constraints

### Must
- Read exact frozen dispatch inputs, current task, resource leases and provisioned model settings from the domain writer.
- Commit Started with a fresh writer clock before any child creation; never spawn after a lost start response.
- Retain one worker and process owner across caller cancellation and stop the actual owner before any successful dispatch settlement.
- Preserve historical attempt fencing and dirty leases after failed, cancelled, stale or unknown execution.
- Separate upstream completion, physical cleanup, canonical task state and domain settlement.
- Bound execution, cancellation, domain calls, output and synchronous join observations explicitly.
- Restrict offline fixtures to fixed host-owned directories; resource/path equality is not physical directory custody or sandbox proof.

### Must Not
- Do not accept model-supplied task identity, paths, resource selection or permission overrides as authority.
- Do not mark canonical Done or send Matrix replies from upstream completion.
- Do not enable owner approvals without their existing exact application gate.
- Do not enable a production runner, live models, a scheduler or an arbitrary-command HTTP endpoint.
- Do not clear quarantined or dirty resources through negative observations.

## Boundaries

### Allowed Changes
- native/README.md
- ./Cargo.toml
- ./Cargo.lock
- native/hagency-execution/**
- native/hagency-store/src/domain/owned_dispatch.rs
- native/hagency-store/src/domain/execution.rs
- native/hagency-store/src/domain.rs
- native/hagency-store/src/domain_worker.rs
- native/hagency-store/src/lib.rs
- native/hagency-store/tests/owned_dispatch.rs
- native/hagency-runtime/src/bin/hagency-runtime-probe.rs
- native/hagency-platform/src/lib.rs
- native/hagency/src/runner.rs
- knowledge/decisions/adr-053-native-owned-dispatch.md
- specs/task-rust-owned-dispatch.spec.md
- docs/**

## Acceptance Criteria

Scenario: Frozen authority precedes child creation
  Test: native_owned_dispatch_start_scope
  Given a claimed dispatch with a current task and exact frozen inputs
  When the host validates and commits its start
  Then changed or stale scope is refused before workspace code executes

Scenario: Native completion remains distinct from canonical completion
  Test: native_owned_dispatch_real_pipes
  Level: integration
  Test Double: offline native app-server child on fixed host-owned directory
  Given a fresh DomainStore and an actual owned native child
  When the exact turn completes and its owner is stopped
  Then canonical Done is not inferred and uncertain platform cleanup keeps leases quarantined

Scenario: A bounded reply expiry names its side of the writer queue
  Test: native_domain_unknown_reports_dequeue
  Given a parked earlier job with a command still queued and then a command the writer begins
  When the two-second reply bound expires in each position
  Then the dequeue attribution distinguishes running from queued and grants no retry or completion authority

Scenario: Cancellation and lost replies retain custody
  Test: native_owned_dispatch_cancel_and_unknown
  Level: integration
  Test Double: offline native child and explicit expiry/revocation
  Given cancellation expiry or revocation
  When the host operation reconciles its exact historical attempt
  Then no cancelled operation releases a dirty lease or infers canonical completion

Scenario: Lost writer receipts preserve exact durable custody
  Test: native_owned_dispatch_queue_reply_loss
  Level: integration
  Test Double: bounded private reply-channel gates around the actual domain writer
  Given a start command queued or already durably committed
  When its response receiver expires before the gate opens
  Then the unstarted command is skipped and the committed start is fenced without replay

Scenario: Lost committed start receipt never launches a native child
  Test: native_owned_dispatch_lost_receipt_never_spawns
  Level: integration
  Test Double: library-test-only discard of the actual committed start response
  Given a real start transaction and lost response
  When the owned coordinator performs normal reconciliation
  Then no child is spawned and the actual started attempt retains its lease

Scenario: Negative evidence cannot change a replacement attempt
  Test: native_owned_dispatch_negative_historical_fence
  Given an expired or revoked historical capability
  When the host reports an execution failure
  Then only that authenticated attempt can be fenced and a replacement is unchanged

Scenario: Admission and protocol failures cannot complete work
  Test: native_owned_dispatch_admission_and_protocol_failure
  Level: integration
  Test Double: actual native offline child using fixed host paths
  Given wrong workspace scope failed spawn EOF wrong thread or a server approval request
  When the operation terminates
  Then known unstarted work is released and all started failures remain quarantined

Scenario: Database lock failure retains a bounded negative retry
  Test: native_owned_dispatch_database_lock_refuses_before_spawn
  Level: integration
  Test Double: external test-only SQLite writer lock
  Given the existing database busy timeout
  When admission and negative reconciliation cannot acquire the writer lock
  Then no child runs and an explicit retained retry can reconcile the clean unstarted lease

Scenario: Absolute operation deadline stops silent work
  Test: native_owned_dispatch_deadline_stops_and_fences
  Level: integration
  Test Double: actual silent native offline child
  Given a finite absolute host deadline and no terminal protocol response
  When the deadline expires
  Then retained process stop precedes negative fencing and no clean completion is inferred

## Out of Scope

Live model execution, physical directory provisioning, actual sandbox efficacy,
MCP launch configuration, owner approval application, Matrix delivery, terminal
parity, warm reuse, a global scheduler and full M4 completion.
