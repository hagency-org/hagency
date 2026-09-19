spec: task
name: "Supervise native process scopes across owner connection loss"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREE-LAYER-COMPLETION]
tags: [active, rust, guardian]
---

## Intent

Add a native guardian entrypoint and bounded private startup protocol so Unix
process-group cleanup survives loss of its controlling process. Windows retains
its existing kernel Job Object containment without an extra guardian process.

## Constraints

### Must
- Require explicit prepare and start messages over an anonymous inherited channel before launching work on Unix.
- Bound startup frames and all partial-frame waits; malformed input and owner EOF initiate cleanup.
- Retain kernel child identity and the unreaped leader until final cancellation signals.
- Clean the owned scope when the leader exits or its owner disappears.
- Report the observed guarantee accurately: POSIX group cleanup does not prove detached descendant cleanup.
- Keep native Agent execution disabled until descendant discovery sandbox and runner protocol gates are met.
- Verify owner exit without destructors and unrelated process survival with real fixtures.
- Prevent unrelated inherited host descriptors and guardian reply endpoints from reaching work.

### Must Not
- Do not use shell startup inherited application environments public sockets or numeric-PID commands.
- Do not kill a guardian on timeout and assume its children were stopped.
- Do not claim canonical task completion from process exit or accept a stronger crash guarantee on POSIX yet.
- Do not contact models or deployed services.

## Boundaries

### Allowed Changes
- ./Cargo.lock
- native/**
- specs/task-rust-guardian.spec.md
- knowledge/decisions/adr-029-native-process-scopes.md
- docs/**

### Forbidden
- Live services, credentials and the original dirty checkout.

## Acceptance Criteria

Scenario: Supervised native startup and cancellation preserve scope
  Test: native_guardian_start_stop
  Given explicit native launch arguments and an unrelated running process
  When the host starts and cancels supervised work
  Then its child scope stops without affecting the unrelated process
  And repeated cancellation retains the same observed report

Scenario: Actual native CLI exit has bounded terminal evidence
  Test: native_guardian_cli_entry
  Level: integration
  Test Double: actual native CLI executable through owned guardian or Job Object
  Given a native CLI version command with empty PATH and Unicode cwd
  When the owned process terminates within the unchanged five-second report window
  Then LeaderExited and the platform cleanup scope accompany exact version bytes and stderr EOF

Scenario: Native cleanup survives loss of its owner
  Test: native_guardian_owner_loss
  Given a running fixture with a grandchild
  When the controller exits without running destructors
  Then Unix guardian EOF cleanup or Windows kernel job closure stops the owned work

Scenario: Early leader exit retains cleanup authority
  Test: native_guardian_early_exit
  Given a leader that exits before its child
  When the supervisor observes leader exit
  Then it stops the remaining scope and reports its actual cleanup guarantee

Scenario: Guardian admission fails without launching work
  Test: native_guardian_admission
  Given an incomplete malformed or uncommitted launch request
  When the guardian startup boundary refuses it
  Then no work is launched and startup does not wait indefinitely

## Out of Scope

Complete POSIX detached-descendant discovery, guardian loss recovery, runner IO,
effective sandbox policy and connection to canonical dispatch remain required.
