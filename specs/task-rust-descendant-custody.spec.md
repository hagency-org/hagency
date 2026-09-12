spec: task
name: "Observe detached descendant cleanup through native custody"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREE-LAYER-COMPLETION]
tags: [active, rust, descendants]
---

## Intent

Extend Linux guardian cleanup to detached and double-forked descendants through
kernel subreaper adoption. Preserve Windows Job Object behavior and explicit macOS
refusal where complete descendant custody is unavailable.

## Constraints

### Must
- Enable Linux subreaper custody before work starts in a separate single-threaded guardian with no pre-existing children.
- Obtain signal authority only through retained pidfds that the kernel confirms are waitable children of this guardian.
- Treat proc children records only as discovery hints; never infer completion from an empty or truncated census.
- Report full cleanup only after the root is reaped and a kernel wait including clone children returns ECHILD.
- Preserve the original root Child reaper ownership and never signal by a recycled numeric PID.
- Keep all discovery work and cleanup waits bounded and preserve unknown outcomes on errors or timeout.
- Verify detached children owner loss early exit and unrelated process survival using native fixtures.

### Must Not
- Do not enable subreaper mode in the service or borrow unrelated children.
- Do not claim guardian-death containment sandbox policy or canonical task completion from observed cleanup.
- Do not run detached fixtures on a backend that cannot clean them up.
- Do not contact deployed services or models.

## Boundaries

### Allowed Changes
- native/**
- specs/task-rust-descendant-custody.spec.md
- knowledge/decisions/adr-029-native-process-scopes.md
- docs/**

### Forbidden
- Live services, credentials and the original dirty checkout.

## Acceptance Criteria

Scenario: Native custody stops detached descendants
  Test: native_descendant_scope
  Given a detached descendant and an unrelated process on a supported backend
  When the host cancels its scope
  Then the owned descendant stops and the unrelated process continues
  And unsupported custody refuses before launching detached work

Scenario: Native custody survives owner loss after a double fork
  Test: native_descendant_owner_loss
  Given a detached descendant whose intermediate parent has exited
  When the controller exits without destructors
  Then kernel adoption or Job Object closure stops the owned descendant
  And unsupported custody refuses before launching detached work

Scenario: Early root exit does not hide remaining descendants
  Test: native_descendant_early_exit
  Given a root that exits while its detached descendant runs
  When the native supervisor observes its exit
  Then full cleanup requires kernel evidence that no owned descendants remain
  And unsupported custody refuses before launching detached work

## Out of Scope

Complete macOS descendant custody, guardian-death recovery, actual runner protocols,
sandbox policy and dispatch integration remain required later work.
