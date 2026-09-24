spec: task
name: "Retain original stopped-owner workspace inspection for explicit recovery"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-LOCAL-THREE-RUNNER-QUALIFICATION, REQ-THREE-LAYER-COMPLETION]
tags: [active, rust, codex, recovery, custody]
---

## Intent

The failed real Codex file task had full stop observed in its original owner but
lost that proof on service shutdown. Preserve a host-only, exact-attempt physical
inspection receipt while the original process result and Started workspace are
still retained. This is the prerequisite for explicit stopped-task recovery,
not an automatic resume or a retrofit of proof into the old failed attempt.

## Constraints

- Capture only inside the original owned worker, after actual whole-tree stop,
  leader exit and accepted signals, and after exact negative fencing.
- Inspect the retained original root, with no-follow traversal, finite entry,
  byte, depth and time bounds and a second matching pass. Hash file contents;
  record symbolic links without following them. Refuse unsupported objects,
  root replacement, changed content and incomplete coverage.
- The inventory records observed file contents/kinds; it is not semantic proof
  that replaying external effects is safe. Explicit operator review remains owed.
- Persist at most one immutable receipt per original dispatch/fence, authenticated
  by its private Started scope and historical capability. Identical receipt
  replay may inspect the original commit; different evidence must conflict.
- No runtime JSON or status object constructs a receipt. Mutating a returned
  Report must not create or change one.
- Receipt creation never clears dirty/quarantine, leases, stop records or inputs;
  never changes Done, final replies or the original failure classification.
- Keep negative/unknown inspection separate and retain an unacknowledged original
  receipt for explicit retry. No background retry or fresh scan after reply loss.
- Tests are offline and use actual owned processes and filesystem effects.

## Allowed changes

- native/hagency-execution/src/**
- native/hagency-execution/tests/**
- native/hagency-store/src/**
- native/hagency-store/tests/**
- specs/task-rust-stopped-owner-inspection.spec.md
- knowledge/decisions/adr-162-native-stopped-owner-inspection.md
- docs/**

## Scenarios

Scenario: A failed actual owner retains exact stopped workspace evidence
  Test: native_owned_stopped_inspection
  Given an actual Started native process with observed full cleanup
  When it fails and its original worker inspects the retained workspace
  Then the exact original attempt has an immutable receipt
  And its task and stop remain failed or incomplete with the lease retained

Scenario: Returned report fields cannot manufacture a receipt
  Test: native_owned_stopped_inspection_refusals
  Given no original Started workspace or unproven process cleanup
  When callers alter diagnostic report fields
  Then no stop inspection can be recorded from them

Scenario: Workspace inventory is bounded and never follows links
  Test: native_stopped_workspace_inventory
  Given real regular files directories and a symbolic link outside the workspace
  When the original root is inspected
  Then the content inventory is exact and the link target is not traversed
  And changed roots unsupported objects and exceeded bounds refuse inspection

## Out of scope

Automatic retry, lease release, semantic operator review, recovery of an older
attempt without original proof, Matrix session successor routing and full E2E.
