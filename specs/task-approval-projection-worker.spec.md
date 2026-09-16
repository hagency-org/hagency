spec: task
name: "Bounded approval projection request worker"
status: accepted
inherits: project
---

## Intent

Drain canonical approval request projections after startup, SSE wakes, reconnects, and timer recovery without restoring the legacy direct-send path.

## Decisions

- One bridge-owned worker reads one page of 20 rows per pass and sends at most two distinct requests concurrently.
- Duplicate wakes coalesce into at most one follow-up pass; a five-second timer provides convergence without an unbounded loop.
- Opaque cursor progress advances past unavailable requests and wraps only after an empty page.
- Stop prevents new work and clears only the worker timer; already-started sends retain uncertain delivery semantics.
- Representative and public-agent raw Matrix requests own a 25-second whole-response deadline; local SDK crypto/send keeps its inherited SDK bound and holds its worker slot until settlement.

## Boundaries

### Allowed Changes
- bridge-matrix.js
- backend-v2.js
- lib/approval-store.js
- tests/api-approval-projections.test.js
- tests/bridge-approval-projection-adapter.test.js
- tests/bridge-approval-reconciliation.test.js
- tests/bridge-matrix-approval.test.js
- tests/approval-fail-closed.test.js
- tests/approval-owner-can-see-it.test.js
- tests/api-project-sides.test.js
- tests/side-provenance.test.js
- specs/task-approval-projection-worker.spec.md

### Forbidden
- Marker and legacy attestation adapters, approval-store schema, native verdict authorization, GUI, Cargo, live Matrix/provider calls, and service restart.

## Acceptance Criteria

Scenario: Startup and missed events converge
  Test: approval events and reconnect only wake the canonical worker
  Given canonical due request projections
  When startup or a redacted wake occurs or an SSE event is missed
  Then one owned nonoverlapping worker eventually reads the due page.

Scenario: Drain work is bounded and fair
  Test: a full page progresses behind two persistently unavailable requests with concurrency two
  Given more than one request and one unavailable publisher
  When one drain pass completes
  Then at most two distinct requests run concurrently
  And cursor progress permits later requests to run.

Scenario: Stop preserves uncertain sends
  Test: timer convergence is nonoverlapping and stop prevents new work
  Given a drain has started
  When the worker stops
  Then no new pass begins and its timer is cleared
  And the started operation is not relabeled as unsent.

Scenario: Raw Matrix requests cannot occupy both slots forever
  Test: final PUT aborts a stalled response body within the owned deadline
  Given a representative or public-agent request returns headers without a complete body
  When the owned whole-response deadline expires
  Then the socket is aborted and the durable attempt remains retryable.

Scenario: Legacy event path only wakes canonical work
  Test: approval_requested only wakes canonical durable publication
  Given an approval requested event
  When the bridge handles it
  Then it queues the canonical worker without direct Matrix delivery or delivery-failed denial.

Scenario: Public notice follows durable private delivery
  Test: failed private publication keeps public notice ineligible and decision pending
  Given the private request has no durable Matrix receipt
  When due projection pages are read again after retry backoff
  Then the public notice remains ineligible and the canonical decision remains unchanged.

## Out of Scope

Marker publication, legacy publisher attestation, GUI integration, and live deployment.
