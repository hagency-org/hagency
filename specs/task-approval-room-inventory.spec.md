spec: task
name: "Bounded approval room reconciliation inventory"
status: accepted
inherits: project
satisfies: [REQ-APPROVAL-CANONICAL-PROJECTION, REQ-OWNER-UI-APPROVAL]
---

## Intent

Expose a read-only room inventory for the existing approval reconciliation worker. Retained rooms remain discoverable after their marker queue empties, so a delayed legacy state can be reconciled without inventing an approval request or creating another room.

## Decisions

- The bridge-secret GET `/api/approval-bindings/matrix/rooms` returns at most 200 rooms, default 20, with an opaque room cursor.
- Derive rooms from canonical bindings including inactive bindings, retained legacy/v2 marker scopes, marker history and retirement records. Preserve the existing active-only bindings API.
- Order room identifiers with the same codepoint comparison used for cursor advancement. Empty final pages permit the worker to wrap independently.
- Expose a synchronization candidate only when current active bindings have one owner consistent with retained marker ownership; conflicts have no candidate.
- Choose the smallest active agent by codepoint order solely as a synchronization representative; the backend still derives the complete shared-room manifest. Retained owner conflicts require explicit operator resolution and never transfer room ownership automatically.
- Return routing fields only. Reading neither saves nor migrates the store, and the underlying in-memory scan remains proportional to retained state.

## Boundaries

### Allowed Changes
- lib/approval-store.js
- backend-v2.js
- tests/approval-room-inventory.test.js
- specs/task-approval-room-inventory.spec.md

### Forbidden
- Binding/request mutation, approval authorization changes, Matrix I/O, new timers, credentials in responses, dependency changes, live runtime changes, or GUI code.

## Acceptance Criteria

Scenario: Shared bindings produce one inventory entry
  Test: shared bindings produce one read-only room candidate
  Given four bindings across three agents and two projects share one owner room
  When the store lists reconciliation rooms
  Then one candidate represents that room and canonical binding bytes stay unchanged.

Scenario: Empty queues and inactive bindings retain rooms
  Test: inactive and retained marker-only rooms remain discoverable
  Given inactive bindings and receipted legacy or v2 marker history
  When their due queue has no work
  Then the inventory still includes those rooms without claiming an active synchronization candidate.

Scenario: Conflicting ownership cannot select an owner
  Test: conflicting current or retained owners have no synchronization candidate
  Given current bindings or retained marker ownership disagree
  When the store lists rooms
  Then it exposes a conflict with no owner or agent candidate and performs no write.

Scenario: Real completed publication survives restart
  Test: receipted and deactivated shared room remains inventoried after reload
  Given real v2 and retirement receipts followed by deactivation of the four bindings
  When the persisted store reloads with no due marker work
  Then the room remains inventoried without an active synchronization candidate.

Scenario: Cursor progress survives changing inventory
  Test: room pages use codepoint cursors and preserve late insertion on wrap
  Given more rooms than one page with mixed-case identifiers
  When a page anchor disappears and a room is inserted before its cursor
  Then later rooms remain reachable and the inserted room appears after wrap.

Scenario: Invalid cursors fail without mutation
  Test: invalid inventory cursors and limits fail without mutation
  Given an invalid cursor or nonpositive or fractional limit
  When the store lists rooms
  Then it rejects the request without saving or changing canonical state.

Scenario: Inventory API keeps the protected routing boundary
  Test: inventory API is bridge-secret only bounded and read-only
  Given the concrete backend with bound rooms
  When callers request the inventory
  Then unauthenticated and operator-only calls are rejected and an authenticated page contains routing fields only.

## Out of Scope

Scheduler integration, exact-room migration, actual Matrix publication, and live GUI acceptance.
