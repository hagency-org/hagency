spec: task
name: "One canonical approval worker across requests and retained rooms"
status: accepted
inherits: project
satisfies: [REQ-APPROVAL-CANONICAL-PROJECTION, REQ-OWNER-UI-APPROVAL]
---

## Intent

Connect accepted request, legacy read-only status, room inventory and marker adapters to the existing MatrixBridge worker. Preserve the shared owner room and canonical authority while making progress across unavailable rows, restarts and missed wakes.

## Decisions

- Reuse the existing startup/SSE/reconnect/five-second timer, wake bit, drain Promise and epoch. One wake runs one pass and at most one coalesced follow-up; no second scheduler or recursive full drain.
- Each pass snapshots at most20 request rows,20 inventory rooms and20 due marker rows using independent opaque cursors. An empty successful page wraps only its cursor; a failed page retains its cursor and does not discard the other categories. Commit a nonempty page tail only after every distinct selected identity has settled; stopped unstarted work leaves that page cursor unchanged.
- Interleave categories through one two-slot pool. Acquire a request-ID lock and target-room lock atomically before a request job; inventory and marker jobs acquire their approval-room lock. A locked job stays queued without occupying a slot or being silently dropped. Release locks and slots only after the entire adapter Promise, including crypto and exact receipt cleanup, settles.
- Dispatch native_v2 request/status/public rows to the accepted native entry. Dispatch only legacy_v1/private_status to the legacy proof entry. Reject other legacy channels and old room_marker v1 publication; only room_marker_v2 and room_marker_v1_retirement reach state PUT.
- Inventory jobs synchronize eligible current-owner rooms from backend-derived associations, then observe exact v1 state and reconcile retained rooms even when the due queue is empty. Conflicting owners remain unresolved. Optional exact-room migration of inactive v1 scopes touches only that room under verified current publisher authority; one invalid room cannot block another. Never rewrite canonical bindings or infer membership from null.
- V2 receipt precedes a separately prepared/sent/receipted empty-object v1 retirement. Matrix state PUT has no transaction deduplication or remote CAS; repeated observation provides convergence, not exactly-once delivery.
- Pass the captured worker epoch predicate into marker and legacy inner I/O. Their optional caller signal remains supported. Stop or epoch change prevents new page jobs, registration, synchronization, history/state reads, prepare, begin and Matrix send. Already-started crypto remains awaited; SDK internal key/member I/O is not cancellable here and has inherited transport limits, so no total25s completion guarantee.
- Once an already-started Matrix send has yielded a valid exact event ID, preserve its immutable plan identity and attempt only that receipt through a fresh bounded backend cleanup request even after stop/epoch change. This exception never permits another Matrix send, prepare, begin, publisher switch or history read. Failed/aborted sends without a complete event ID remain uncertain; no success is invented. Cleanup failure keeps durable attempted work recoverable.
- Adapter-owned HTTP retains absolute complete-response deadlines, response byte caps, redirect rejection and safe error codes. Underlying retained JSON scans/clones/saves remain proportional to retained state; only page/job/network selection is bounded.

## Boundaries

### Allowed Changes
- bridge-matrix.js
- lib/approval-marker-bridge.js
- lib/legacy-approval-projection.js
- lib/approval-store.js
- backend-v2.js
- tests/bridge-approval-single-worker.test.js
- tests/bridge-approval-reconciliation.test.js
- tests/bridge-approval-projection-adapter.test.js
- tests/bridge-approval-room-marker.test.js
- tests/bridge-legacy-approval-projection.test.js
- tests/approval-shared-room-marker-v2.test.js
- specs/task-approval-single-worker-integration.spec.md
- specs/task-legacy-approval-projection-bridge.spec.md

### Forbidden
- New daemons, dependencies, global settings, native verdict authorization changes, fake private_request plans, alternate publisher delegation, room creation/config relayout, SDK doRequest mutation, uncancelled crypto Promise races, GUI, Cargo, provider calls, live service writes, commit/push before root review.
- Backend/store edits beyond exact-room marker migration selection/current publisher validation and corresponding regression coverage.

## Acceptance Criteria

Rule: single-owner-fairness — One owner and fair bounded selection

Scenario: Missing wakes still converge
  Test: single worker startup timer SSE and reconnect converge through actual adapters
  Given canonical due work exists before startup and another row is persisted without SSE
  When startup, SSE, reconnect and controlled timer wakes occur
  Then one drain Promise and timer drive real protected APIs without legacy direct delivery.

Scenario: Categories share the same two slots
  Test: single worker holds two global slots across native legacy inventory and marker jobs
  Given selected native, legacy, inventory and marker work on multiple rooms
  When captured HTTP or SDK crypto keeps two jobs pending and more wakes arrive
  Then no third job enters adapter I/O and at most one immediate follow-up pass is queued.

Scenario: Locks do not drop selected identities
  Test: single worker serializes shared rooms and request identities without skipping page rows
  Given one page contains distinct jobs sharing a room or request and a job for another room
  When the first shared-room job remains pending
  Then unrelated work uses the spare slot and all selected identities eventually settle once without overlapping room writes.

Scenario: Each cursor progresses beyond unavailable predecessors
  Test: single worker independent opaque cursors reach later work and wrap after insertion
  Given more than20 requests and23 retained rooms with unavailable predecessors
  When bounded passes run and an earlier room is inserted
  Then later native legacy and room work progresses, failed page cursors remain unchanged, and the insertion is visited after its category wraps.

Rule: retained-room-convergence — Canonical migration and retained discovery

Scenario: Shared topology remains one manifest
  Test: single worker publishes four shared bindings as v2 before distinct v1 retirement
  Given oldprobe and Claude project1 plus Claude and Codex project2 share one owner room with null membership values
  When the actual inventory synchronization and marker adapters run
  Then v2 contains all four associations, its receipt precedes v1 retirement, and bindings and membership values remain unchanged.

Scenario: Old marker rows cannot send
  Test: single worker refuses v1 sends and isolates exact room migration failures
  Given a due old room_marker row and two inactive legacy scopes where the first has conflicting authority
  When each selected room is processed
  Then no nonempty v1 PUT occurs, the invalid room remains unchanged, and the independently valid room can migrate and publish v2.

Scenario: Retained rooms remain observable after receipts
  Test: single worker reconciles a late v1 write beyond room page twenty after reload
  Given23 inventoried rooms have no due work and the last room receives a nonempty authenticated v1 state
  When room inventory wraps after restart
  Then that exact room is observed and a new empty retirement is queued without a fabricated old receipt.

Rule: exact-completion-boundary — Authority, failure and completion boundaries

Scenario: Native private failure cannot expose a public request
  Test: single worker native private failure preserves ciphertext and gates redacted public notice
  Given a native pending private request fails at its first Matrix PUT
  When another pass retries and then succeeds
  Then exact stored ciphertext and transaction are reused, public notice follows durable private receipt, and public content contains no private fields or controls.

Scenario: Legacy evidence never becomes a new actionable request
  Test: single worker routes positive and unresolved legacy rows without actionable resend
  Given consumed legacy rows with valid two-hop evidence and missing evidence coexist
  When startup and repeated wakes run
  Then positive evidence publishes only canonical read-only status and unresolved rows cannot create requests, notices or replayed verdicts.

Scenario: Stop reaches marker and legacy inner boundaries
  Test: single worker stop inside prepare crypto and state begin blocks later Matrix IO
  Given actual adapter work pauses in backend prepare, SDK crypto, history GET or marker begin
  When the worker epoch changes
  Then subsequent Matrix I/O is absent and awaited crypto keeps its slot until settlement.

Scenario: Exact known receipts survive stop
  Test: single worker records exact native legacy and marker receipts after stop
  Given a started native, legacy or marker PUT yields a complete valid event ID
  When stop occurs before its receipt API call
  Then only the exact immutable plan receipt is attempted with the existing native or marker backend deadline of12s or the legacy receipt deadline of10s and no additional Matrix work begins.

Scenario: Lost send responses remain uncertain
  Test: single worker preserves uncertain sends and stop recovery without false receipts
  Given a Matrix response body is incomplete or a receipt API fails
  When stop or deadline occurs and the worker later restarts
  Then no event ID is invented and immutable prepared work remains recoverable without another actionable legacy card.

## Out of Scope

Live history/key availability, user approval actions, provider completion, GUI acceptance, deployment, general storage redesign and cancellation of SDK-internal crypto I/O are root-owned later gates. Preparation in the ignored evidence directory does not authorize source edits before root releases the integrated worker tree.
