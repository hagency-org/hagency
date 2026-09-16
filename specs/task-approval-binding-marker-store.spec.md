spec: task
name: "Canonical approval binding marker store and API"
status: accepted
satisfies: [REQ-APPROVAL-CANONICAL-PROJECTION]
---

## Intent

Persist independently keyed private approval-room discovery markers and expose their CAS lifecycle to the trusted bridge. Marker work must never borrow approval request identity or publish Matrix state in this checkpoint.

## Decisions

- Marker identity is `(approval_room_id, binding_generation, marker_channel)` where marker_channel is `room_marker`.
- One marker represents the bounded active project-room associations for an `(agent, owner, approval room)` scope; overflow rejects the update without truncation.
- Association or pinned publisher changes advance generation and supersede unsent older generations. Exact receipts remain valid for older generations whose send had durably begun.
- Marker payload is the fixed `com.agentchat.approval.room.v1` empty-state-key content and contains only version, generation, publisher, owner, agent, and project-room associations.
- Bridge-secret list/sync/prepare/begin/receipt/retry routes use stable opaque cursors and internal CAS. Matrix state PUT has no remote transaction-ID guarantee.

## Boundaries

### Allowed Changes
- lib/approval-store.js
- backend-v2.js
- tests/approval-binding-marker.test.js
- tests/api-approval-binding-markers.test.js
- specs/task-approval-binding-marker-store.spec.md
- specs/task-approval-projection-store.spec.md

### Forbidden

- Approval request IDs for markers, request projection event namespaces, Matrix I/O, bridge publication, credential authorization, global authentication, provider execution, GUI code, dependencies, and live services.

## Acceptance Criteria

Scenario: Marker generations follow canonical binding associations
  Test: marker generations track old room new room duplicate and publisher changes
Given multiple project bindings sharing one private approval room
When associations are added, rebound, deactivated, or synced with a changed publisher
Then each affected room advances independently, the old association becomes inactive, and duplicate sync adds no generation.

Scenario: Marker payload is bounded and private
  Test: marker payload namespace contains no approval request identity
Given one canonical marker row
When its payload is returned
Then it contains only the fixed discovery fields and no approval request identity.

Scenario: Marker overflow and persistence faults are atomic
  Test: marker overflow and persistence faults roll back memory and disk
Given the maximum association set or an injected pre-rename failure
When another association or publisher generation is written
Then the operation fails without truncation and memory and disk remain unchanged.

Scenario: Marker CAS survives races and restart
  Test: marker preparation is first-writer-wins and ready work cannot retry or receipt
Given concurrent preparations for one marker generation
When both contexts prepare and a caller retries or receipts before begin-send
Then one immutable plan wins and premature bookkeeping is rejected without mutation.

Scenario: Superseded marker generations cannot overwrite newer state
  Test: superseded ready markers cannot begin while attempted markers can receipt after reload
Given a prepared or attempted old generation followed by a new generation
When the bridge begins or retries old work or reports an exact old receipt
Then never-attempted work is blocked, retry is blocked, and only the already-attempted exact receipt is recorded.

Scenario: Marker API is bridge-secret only and cursor-stable
  Test: marker routes are secret-only independently keyed and cursor-stable
Given multiple due marker generations
When callers list and mutate marker work
Then only the bridge succeeds and completing one page does not invalidate its opaque cursor.

Scenario: Marker ordering and room ownership are globally consistent
  Test: marker cursor uses the same code-point order as marker sorting
Given mixed-case room IDs in the due marker set
When the caller follows a one-row cursor
Then the next row is selected with the exact same total order used to sort the page.

Scenario: One Matrix room has one marker scope
  Test: one Matrix room rejects a second agent marker scope without mutation
Given an empty-key marker scope already owns an approval room
When another agent tries to synchronize a marker for that room
Then the conflicting scope is rejected and no row, CAS token, memory, or disk state changes.

## Out of Scope

Publisher credential authorization is B1.3. Matrix state publication, reconciliation, and bridge reconnect draining are B1.4.
