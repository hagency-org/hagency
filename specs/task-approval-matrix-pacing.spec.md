spec: task
name: "Shared approval Matrix request admission"
status: draft
inherits: project
satisfies: [REQ-APPROVAL-CANONICAL-PROJECTION, REQ-OWNER-UI-APPROVAL]
---

## Intent

Prevent a fast approval backlog from repeatedly exhausting Palpo's shared per-IP burst budget. Keep canonical security checks and the existing two-job worker while spacing explicit approval Matrix HTTP starts across native, legacy and marker adapters.

## Decisions

- One per-bridge FIFO admission queue uses a monotonic clock, a default 200 ms gap and burst 1. It invokes the actual request-start callback at admission; delayed timers cannot create catch-up bursts.
- Admission covers explicit native security/state/send calls, legacy private-context/event/send calls and marker read/write calls. Backend prepare/begin/receipt cleanup is not paced. SDK-internal crypto key/member I/O remains opaque, awaited and outside the admission guarantee.
- Queue time consumes existing request deadlines. Abort, actor identity, worker epoch, deadline and the existing shared 429 cooldown are rechecked immediately before starting I/O. Admission never resets or shortens cooldown.
- Native local security methods accept optional admission controls; callers without those controls retain existing security and transport semantics. Already-started SDK I/O remains awaited under its inherited transport limitations.
- An already-known exact send event ID still reaches bounded receipt cleanup after stop; no new Matrix I/O is authorized by that cleanup exception. Worker wake, pool, room locks and page cursors are unchanged.

## Boundaries

### Allowed Changes
- bridge-matrix.js
- lib/approval-matrix-pacer.js
- lib/approval-marker-bridge.js
- lib/legacy-approval-projection.js
- tests/approval-matrix-pacer.test.js
- tests/bridge-approval-single-worker.test.js
- tests/bridge-approval-projection-adapter.test.js
- tests/bridge-approval-room-marker.test.js
- tests/bridge-legacy-approval-projection.test.js
- specs/task-approval-matrix-pacing.spec.md

### Forbidden
- Worker scheduling/cursor changes, backend/store schema or API changes, new daemons/dependencies/global settings, SDK monkeypatches, crypto Promise races, authorization bypasses, Palpo/config changes, live/provider requests, Cargo, or publication before root review.

## Acceptance Criteria

Scenario: Native legacy and marker HTTP share one default admission gap
  Test: approval HTTP admission shares the default gap across real native legacy and marker adapters
  Level: integration
  Test Double: loopback Matrix HTTP and crypto engine boundary; actual MatrixBridge, SDK, protected Express and durable store
  Given real canonical request, legacy and marker work reaches one MatrixBridge
  When the existing worker invokes actual adapters against protected Express and a loopback Matrix server
  Then request starts across categories are separated by the default 200 ms admission policy and exact receipts remain durable.

Scenario: Delayed timers cannot accumulate a burst
  Test: approval pacer delayed timers start one request and reschedule from actual admission
  Given multiple waiting admissions and a delayed timer
  When the monotonic clock advances beyond multiple nominal slots
  Then only one callback starts and the next waits 200 ms from that actual start.

Scenario: Side adapters share the same admission owner
  Test: approval side security send and marker PUT share actual HTTP admission
  Level: integration
  Test Double: loopback Matrix HTTP; actual protected side credentials, MatrixBridge, Express and durable store
  Given a current accepted project-side appservice publisher from the protected credential endpoint
  When native security checks and sends run beside marker PUT through the concrete bridge
  Then their loopback HTTP starts share the default 200 ms gap and preserve the exact masquerade identity.

Scenario: Queued work is canceled before network I/O
  Test: approval queued stop rotation and deadline prevent new Matrix I/O
  Level: integration
  Test Double: loopback Matrix HTTP and synthetic credential rotation; actual adapter admission and store
  Given an admission is waiting behind another request
  When its worker stops, actor rotates or request deadline expires
  Then the real adapter starts no Matrix request for that queued operation.

Scenario: Genuine 429 retains shared cooldown
  Test: approval pacing honors a real429 before any later category starts
  Given a loopback Matrix response completes with 429
  When another approval category reaches admission
  Then the shared cooldown prevents its request without being reset or shortened.

Scenario: Known send receipt survives stop
  Test: single worker records exact native legacy and marker receipts after stop
  Given an admitted send completes with a valid immutable event ID
  When the worker stops before cleanup
  Then its exact receipt remains durable without a new Matrix send.

Scenario: Admission cancellation does not abandon started I/O
  Test: approval pacer keeps a started response owned after admission signal aborts
  Given a request callback has already started and its response is pending
  When its admission signal aborts
  Then the caller still awaits the response Promise and can retain its exact known receipt.

Scenario: Optional owner observation fails closed on rotation
  Test: approval owner observation skips unavailable publisher after exact receipt
  Given the canonical request receipt is committed
  When the current local publisher becomes unavailable before the owner-membership observation
  Then the known receipt remains valid and no unpaced fallback observation starts.

## Out of Scope

Durable legacy negative-proof backoff, SDK-internal HTTP pacing, Palpo policy changes and live business/GUI acceptance remain separate work.
