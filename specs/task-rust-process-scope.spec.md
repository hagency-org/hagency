spec: task
name: "Prove native process startup and owned scope cancellation"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREE-LAYER-COMPLETION]
tags: [active, rust, platform]
---

## Intent

Establish kernel process scope before controlled child code starts, without enabling
Agent execution until descendant and sandbox gates are satisfied.

## Constraints

### Must
- Create Windows children inside a kill-on-close Job Object atomically at process creation.
- Create POSIX process groups before exec and retain the unreaped leader identity until signalling is finished.
- Reject requested guarantees that the active platform adapter cannot prove.
- Keep environment executable arguments and working directory explicit and bounded without a shell.
- Keep unknown cleanup outcomes distinct from verified full descendant cleanup.
- Verify ordinary cancellation early leader exit Unicode arguments and unrelated process isolation using native fixture processes.

### Must Not
- Do not treat a POSIX group as containment for detached descendants or a Job Object as a filesystem/network sandbox.
- Do not take a runtime-supplied PID as signal authority.
- Do not start real Agent models or change live services.

## Boundaries

### Allowed Changes
- ./Cargo.toml
- ./Cargo.lock
- native/**
- specs/task-rust-process-scope.spec.md
- knowledge/decisions/adr-029-native-process-scopes.md
- docs/**

### Forbidden
- Deployed state, credentials, original dirty checkout and live services.

## Acceptance Criteria

Scenario: Native launch preserves its declared scope and exact arguments
  Test: native_process_scope_start_stop
  Given an absolute fixture executable Unicode arguments and an unrelated child
  When the platform scope launches and stops the fixture process tree
  Then startup scope and argument transport are verified and unrelated work remains alive

Scenario: Early exit does not discard cancellation authority
  Test: native_process_scope_early_exit
  Given a leader that exits before the first observation while a child remains
  When the owner cancels the scope
  Then the remaining group or job receives cancellation without recycled PID authority

Scenario: Required crash containment is proven or explicitly refused
  Test: native_process_scope_crash_guarantee
  Given a request that requires cleanup after owner process death
  When the platform accepts or refuses the launch
  Then Windows job children stop on owner death and unimplemented POSIX crash containment refuses before spawn

## Out of Scope

POSIX detached-descendant tracking and guardian crash containment, real runner
protocols, sandbox enforcement, terminal IO, domain dispatch integration and cutover
remain separate required migration gates.
