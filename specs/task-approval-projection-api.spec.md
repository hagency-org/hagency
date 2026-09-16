spec: task
name: "Approval projection request API checkpoint"
status: accepted
satisfies: [REQ-APPROVAL-CANONICAL-PROJECTION]
---

## Intent

Expose durable request projection work to the trusted Matrix bridge and wake it after committed canonical approval changes. This internal checkpoint provides request scheduling and bookkeeping without sending Matrix events.

## Decisions

- Every route uses the existing bridge secret and the store's CAS tokens.
- Listing is bounded to 200 rows and paged by an opaque row cursor.
- Prepare, begin, retry, and receipt bind the URL request and revision plus the row channel, target room, publisher, credential generation, and server-derived transaction identity.
- Startup and the 30-second maintenance timer perform bounded expiry and migration work and emit redacted SSE hints only after durable store calls return.
- Publisher credential authorization and Matrix I/O remain B1.3 dependencies. Independently keyed binding-marker state and API are B1.2b; bridge startup/reconnect drain is B1.4.

## Boundaries

### Allowed Changes
- backend-v2.js
- lib/approval-store.js
- tests/api-approval-projections.test.js
- specs/task-approval-projection-api.spec.md
- specs/task-approval-projection-store.spec.md

### Forbidden

- Matrix sends, provider approval execution, global authentication, agent tokens, A3 trusted-origin fields, public approval detail fields, binding-marker records, service configuration, dependencies, and live services.

## Acceptance Criteria

Scenario: Projection request routes require the bridge secret and preserve privacy
  Test: projection listing is bridge-secret only and keeps metadata out of agent response
Given durable request projection work
When an unauthenticated caller, agent bearer, or trusted bridge lists it
Then only the trusted bridge receives bounded Matrix metadata and opaque cursor pagination, while the agent response exposes no projection plan.

Scenario: Pagination survives normal completion of its anchor
  Test: page cursor survives receipt of its anchor row
Given a first page and more due projection rows
When the bridge receipts the first page row before requesting the next page
Then the opaque cursor still returns the following due row without replaying the anchor.

Scenario: Listing returns one coherent committed revision
  Test: listing expires first and returns one coherent committed projection
Given an unprepared pending row whose approval deadline has passed
When the bridge lists due work
Then bounded maintenance commits expiry before selection and the returned row and approval payload both describe the expired revision.

Scenario: Scheduled maintenance contains persistence errors
  Test: maintenance contains persistence failure and a later tick recovers
Given an expiry save that fails before atomic rename
When scheduled maintenance runs and later retries after persistence recovers
Then the first tick returns an observable failure without throwing or waking and the later tick commits expiry exactly once.

Scenario: Projection plan operations bind every route and CAS identity
  Test: prepare begin retry and receipt preserve exact route and plan CAS
Given one due request projection row
When the bridge prepares, begins, retries, and receipts it
Then the stored winning plan is used, and a mismatched request, revision, channel, room, publisher, credential generation, transaction ID, or CAS token is rejected without mutation.

Scenario: Maintenance emits only redacted durable wake hints
  Test: bounded maintenance emits redacted wake hints for durable due work
Given due projection work and bounded expiry or migration work
When startup or the maintenance timer runs
Then each examined batch remains within its configured limit and SSE wake payloads contain exactly request_id and revision.

Scenario: Canonical transitions wake the bridge after commit
  Test: committed create verdict and consume transitions emit increasing redacted wakes
Given a native approval request and trusted owner verdict
When create, verdict, and consume commit revisions one through three
Then each transition emits one redacted request_id and revision wake in increasing order.

## Out of Scope

Publishing Matrix events, resolving or authorizing publisher credentials, canonical binding-marker rows, bridge reconnect draining, GUI integration, dependency changes, and broad combined Phase A verification are outside this internal checkpoint.
