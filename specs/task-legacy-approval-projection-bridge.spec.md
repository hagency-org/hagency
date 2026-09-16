spec: task
name: "Legacy approval two-hop reader and readonly publisher"
status: accepted
satisfies: [REQ-APPROVAL-CANONICAL-PROJECTION, REQ-OWNER-UI-APPROVAL]
inherits: project
---

## Intent

Recover positively proven legacy original events through the current private Matrix context and publish only canonical read-only status. Add one production MatrixBridge adapter entry, with no scheduler or changes to approval authority.

## Decisions

- Read exactly the stored candidate verdict and its same-room reply target, at most2 event GETs. Raw authenticated event sender/id/room and decrypted envelope must agree; both message roles and canonical tuple/action/runtime/upstream/expiry must match. Edited/redacted/missing/decryption failures remain unresolved without history search or live event dispatch.
- Original full sender must match one current authorized private publisher. Register and attest through protected APIs, fetch canonical version1 legacy_v1 status/relation, prepare with legacy_evidence_cas/private_context, and send only the durable winning plan. Existing uncertain plans reuse exact bytes and transaction without history reads or encryption.
- Every adapter-owned HTTP request and complete response has a maximum10s deadline and the remaining25s adapter attempt budget. Abort releases owned response streams. Stop/rotation is checked before each new adapter I/O and finalPUT. A complete exact send receipt may finish through a separate10s backend-only request after stop or rotation; this permits no new Matrix send or preparation. HTTP redirects are rejected; no automatic retry or publisher fallback occurs within an attempt.
- SDK0.8.0 crypto exposes no cancellation; encryption may run internal key/member HTTP under inherited60s SDK limits. Await crypto in the same worker-owned Promise/slot without Promise.race or mutation of shared doRequest. A crypto overrun prevents later adapter I/O but does not guarantee25s total completion or cancellation of already-started SDK internal I/O.
- Side actor enumeration uses the protected acting-credentials refresh carrying active/accessState/representativeMxid/outboundGeneration into actingSideFor. Tests validate the actual store, protected endpoint and refreshed bridge cache without invented actor fields.
- Local plaintext remains an explicit non-production ADR-003 diagnostic. A current side representative may use the distinct production ADR-016 plaintext topology only when the owner and representative are joined and the encryption-state request returns exact `404 M_NOT_FOUND`; encrypted or indeterminate side rooms fail closed.
- No scheduler, second daemon, canonical request/binding mutation, fake private_request plan, actionable legacy event or public notice is introduced. Native approval adapters remain unchanged.

## Boundaries

### Allowed Changes
- bridge-matrix.js
- lib/legacy-approval-projection.js
- tests/bridge-legacy-approval-projection.test.js
- tests/legacy-approval-proof.test.js
- specs/task-legacy-approval-projection-bridge.spec.md

### Forbidden
- Backend/store authorization changes, request worker/timer/SSE scheduling, marker publication, global settings/SDK request mutation, provider calls, live services, GUI, Cargo and historical onRoomMessage dispatch.

## Acceptance Criteria

Scenario: Real legacy recovery publishes only canonical read-only status
  Test: legacy production adapter proves two hops and publishes canonical encrypted status through real store API
Given a migrated consumed record with a stored candidate verdict
When the actual bridge adapter reads authenticated encrypted verdict and original events
Then it attests exactly that original, encrypts canonical consumed status once, durably begins beforePUT and records the exact receipt without changing canonical requests.

Scenario: Invalid original evidence cannot publish
  Test: legacy proof rejects wrong role tuple sender relation and edited or redacted events
Given bounded raw and decrypted historical events
When any event identity, tuple, role, relation, sender, runtime or expiry mismatches
Then proof validation rejects and no attestation or publication follows.

Scenario: Uncertain replay preserves exact bytes
  Test: legacy uncertain replay skips history and encryption and retains exact transaction bytes
Given a durably attempted plan with an unknown transport outcome
When the adapter retries
Then it sends exactly the stored type/content/transaction and receipts without another event GET or encryption.

Scenario: Owned HTTP respects its complete-response budget
  Test: legacy event HTTP aborts a real stalled response without another hop
  Level: integration
  Test Double: loopback HTTP server and real Express store
Given a historical event response that stalls after headers
When the owned request deadline expires
Then the connection is aborted and no second event GET or MatrixPUT begins.

Scenario: Deadline does not abandon crypto ownership
  Test: legacy crypto overrun retains its promise until settlement and starts no later IO
Given an SDK crypto Promise that remains pending after the adapter deadline
When the deadline expires and crypto later settles
Then the publication Promise remains pending until crypto settles and no later prepare orPUT begins.

Scenario: Stop or rotation prevents later work
  Test: legacy stop and credential rotation between hops prevent later IO
Given a captured current private context
When stop or credential rotation occurs after a historical read
Then no later event GET, attestation orPUT uses that context.

Scenario: Partial begin response preserves uncertainty
  Test: legacy rotation after a durable begin remains uncertain and prevents final PUT
Given a begin-send mutation that already committed
When credential rotation invalidates its response
Then the adapter returns uncertain and starts no Matrix PUT or stale retry bookkeeping.

Scenario: Side credentials retain actual sender and security
  Test: legacy side encrypted or rotated contexts cannot downgrade or continue proof reads
Given an accepted side credential obtained from the current store
When room encryption requires unavailable crypto or the credential rotates
Then the adapter remains unresolved without downgrade or another historical event read.

Scenario: Project-side plaintext is distinct from local diagnostics
  Test: legacy side uses actual store generation and exact private sender transport
Given a production process with no diagnostic flags and an accepted side from the protected acting-credentials endpoint
When joined membership and exact Matrix M_NOT_FOUND prove the side approval room is plaintext
Then appservice and registration-token representatives publish the canonical stored status without enabling local plaintext.

Scenario: Error bodies remain bounded
  Test: legacy rate limit does not retry and its body obeys the same byte bound
Given a captured Matrix rate-limit response
When its body exceeds256KiB or requests a cooldown
Then the adapter bounds body consumption and does not retry inside the attempt.

Scenario: Exact event identity excludes transport redirects
  Test: legacy exact event reads never follow an HTTP redirect to another event
Given an exact stored verdict event URL
When the server returns an HTTP redirect
Then no redirected event or alternate origin is requested.

## Out of Scope

Scheduling, SDK-wide internal crypto cancellation, provider execution, live historical recovery/GUI acceptance and production release are separate gates. This unit cannot claim25s total completion while awaited SDK crypto remains pending.
