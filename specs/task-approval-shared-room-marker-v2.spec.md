spec: task
name: "Shared approval room marker v2 migration"
status: draft
inherits: project
---

## Intent

Replace the single-agent approval-room marker scope with one room-owned v2 manifest so multiple agents and projects sharing the same owner room remain discoverable. Migrate v1 marker history without altering native bindings or approval requests, and retire v1 only after a v2 receipt.

## Decisions

- The canonical identity is `(approval_room_id, room_generation, marker_channel)` and never contains a synthetic approval request ID.
- `com.agentchat.approval.room.v2` uses state key `""`, one common owner/private publisher, and at most 64 canonical `(agent, project_room_id, active)` tuples sorted by code-point order.
- `syncBindingMarker` accepts a compatibility agent selector but derives the whole room manifest from canonical bindings; null agent-membership observations remain eligible.
- `migrateMarkerRoomsV2({limit})` examines at most `limit` unmigrated retained v1 candidates, persists its progress marker, revisits late candidates regardless of lexical position, seeds each room generation above all v1/v2 high-water marks, and rolls back progress plus aggregate state on persistence failure.
- A successful v2 receipt queues `room_marker_v1_retirement` with fixed `{}` content. A v1 retirement receipt stops v1 publication; failed or delayed writes remain reconciliation work.
- Equal-generation identical content replays; equal-generation changed content conflicts. Older ready/begin/retry work is superseded while exact receipts for already-attempted I/O remain valid.
- Binding changes and current private-publisher registry changes invalidate stale unattempted plans. Exact receipts remain admissible after the send boundary, and bounded retirement reconciliation creates fresh work after a late legacy receipt.

## Boundaries

### Allowed Changes
- lib/approval-store.js
- backend-v2.js
- tests/approval-binding-marker.test.js
- tests/api-approval-binding-markers.test.js
- tests/approval-shared-room-marker-v2.test.js
- specs/task-approval-binding-marker-store.spec.md
- specs/task-approval-projection-store.spec.md
- specs/task-approval-shared-room-marker-v2.spec.md

### Forbidden

- Native approval request or binding mutation during marker sync/migration, caller-authored association payloads, request projection namespaces, Matrix I/O, bridge scheduling, Robrix source, GUI code, global authentication, dependencies, live configuration, room creation, or per-agent state keys.

## Acceptance Criteria

Scenario: Four canonical bindings share one complete manifest
  Test: four bindings across three agents and two projects form one room manifest
  Given the accepted four-binding topology including null agent membership observations
  When any associated agent synchronizes the room marker
  Then one v2 row contains all four exact active tuples and both project-two agents resolve to the same room.

Scenario: Room ownership and publisher conflicts fail atomically
  Test: room manifest rejects mixed owner or private publisher without partial state
  Given bindings that disagree on canonical owner or verified private publisher
  When marker synchronization runs
  Then it returns conflict and changes no marker, binding, request, generation, or migration cursor bytes.

Scenario: Marker persistence failure preserves canonical governance records
  Test: marker synchronization rolls back its aggregate without rolling back canonical bindings
  Given canonical bindings were committed before marker synchronization
  When the marker aggregate fails before rename
  Then marker state rolls back while the already committed native bindings remain byte-for-byte unchanged.

Scenario: Room manifest bounds and tuple identity are exact
  Test: duplicate tuples and overflow reject while distinct agents in one project remain valid
  Given duplicate, distinct-agent, and over-64 tuple candidates
  When the full room manifest is normalized
  Then duplicate and overflow inputs fail without truncation while distinct agent/project tuples remain separate.

Scenario: V1 migration is bounded durable and monotonic
  Test: v1 scopes migrate above room high-water across batches restart mutation and rollback
  Given more retained v1 scopes than one migration batch and existing room generation history
  When bounded batches run across restart and a pre-rename persistence fault
  Then each batch examines at most its limit, resumes its exact cursor, preserves direct mutations, and assigns v2 generations above every room high-water mark.

Scenario: Post-rename migration recovery trusts the committed file
  Test: post-rename migration commit reloads durable cursor and aggregate before recovery
  Given marker migration commits its rename but directory synchronization reports failure
  When the degraded instance is rejected and the store reloads
  Then the committed cursor and v2 aggregate remain together and normal writes recover only after reload.

Scenario: V2 publication queues explicit V1 retirement
  Test: v2 receipt queues fixed retirement and preserves old attempted receipt semantics
  Given v1 and v2 rows for one room
  When v2 receives an exact receipt
  Then one retirement row carries fixed v1 type, empty key, and `{}` payload while old ready work cannot begin and exact old attempted receipts remain valid.

Scenario: Failed retirement remains explicit reconciliation work
  Test: failed v1 retirement remains pending reconciliation
  Given retirement has crossed the send boundary
  When transport outcome is unknown and retry is scheduled
  Then the retirement stays hidden until due and reappears as uncertain work.

Scenario: Supersession preserves only already-attempted legacy receipts
  Test: v2 supersedes old ready work but preserves exact attempted receipt
  Given one old v1 plan is ready and another has crossed the send boundary
  When v2 work supersedes both legacy generations
  Then the ready plan cannot begin while the exact attempted receipt remains admissible.

Scenario: Marker replay and pagination remain deterministic
  Test: equal replay stable cursor and due channels survive completion
  Given multiple mixed-case rooms with v2 and retirement work
  When identical sync repeats and a page anchor completes
  Then generation stays equal, the next opaque cursor uses one code-point order, and due result counts remain bounded.

Scenario: New associations invalidate stale room plans
  Test: new room agent supersedes stale ready plan and appears in the next manifest
  Given a ready room plan predates a newly committed agent binding
  When the binding refreshes its shared room marker
  Then the old plan cannot begin and the replacement manifest contains the new association.

Scenario: Conflicting ownership persists governance and fails closed
  Test: owner conflict persists binding but blocks stale room plan from beginning
  Given a newly committed binding conflicts with the room's canonical owner
  When marker refresh cannot derive one authoritative manifest
  Then the binding remains durable and previously ready marker work cannot begin.

Scenario: Publisher rotation separates pre-send and post-send authority
  Test: publisher rotation blocks ready work but keeps exact attempted receipt admissible
  Given the private publisher registry advances after one plan is ready and another crossed the send boundary
  When begin and receipt are attempted with the pinned plans
  Then stale ready work is rejected while the exact attempted receipt remains admissible.

Scenario: Every v2 receipt retains eligible retirement work
  Test: each accepted v2 revision retains eligible retirement reconciliation work
  Given a prior retirement completed before a later v2 room revision
  When the later revision receives its exact receipt
  Then a distinct eligible retirement row remains available for that revision.

Scenario: Late legacy receipts are reconciled within a bound
  Test: late v1 receipt queues one bounded retirement reconciliation
  Given a legacy write completes after the room's earlier retirement
  When bounded retirement reconciliation examines that room
  Then exactly one fresh retirement is queued without fabricating a remote receipt or CAS result.

Scenario: Conflicting governance disables uncertain resend
  Test: conflicting binding blocks uncertain resend but preserves its exact receipt
  Given a marker send has an uncertain outcome before a conflicting owner binding is committed
  When due work, begin, retry, and the exact old receipt are evaluated
  Then resend paths fail closed while the exact post-send receipt remains admissible.

Scenario: Resolving governance conflict creates fresh work
  Test: removing a conflicting binding creates fresh work after invalidation
  Given a conflicting binding invalidated the only unreceipted marker generation
  When that binding is removed and the canonical manifest is synchronized again
  Then a higher generation becomes due even when its associations match the last valid snapshot.

Scenario: Publisher rotation replaces stale retirement work
  Test: publisher rotation replaces an unusable pending retirement
  Given a pending retirement is pinned to an obsolete private credential generation
  When a v2 snapshot is accepted under the current private publisher
  Then the old retirement is superseded and one usable current-context retirement remains.

Scenario: Authenticated observation repairs a lost legacy response
  Test: observed legacy state requeues retirement without fabricating a legacy receipt
  Given a v1 state write resurfaced after retirement but its send receipt was lost
  When the authenticated bridge reports the nonempty v1 slot for the exact room
  Then bounded reconciliation queues canonical retirement work from accepted v2 history without inventing a v1 receipt.

## Out of Scope

Matrix state publication and reconciliation scheduling are B1.4. Robrix dual-read ships in its independently owned frontend unit. This draft does not authorize production implementation before B1.3 review and explicit coordinator release.
