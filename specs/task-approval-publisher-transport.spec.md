spec: task
name: "Pinned approval projection publisher and exact Matrix send"
status: accepted
inherits: project
---

## Intent

Pin each approval projection to the verified Matrix actor and credential generation that will perform its network request. Persist final event bytes and transaction identity before I/O so retries cannot change publisher, ciphertext, route, or transaction.

## Decisions

- Local bot, agent token, appservice `asToken`, and registration representative token have separate opaque credential generations tied to the actual outbound credential.
- Request prepare, begin, and retry validate the current authoritative publisher record; an exact receipt for an already attempted plan remains accepted after rotation.
- A private status inherits its request's immutable private publisher. A public notice uses the registered agent publisher.
- The pure publish checkpoint performs prepare, durable begin, exact send, receipt, and uncertain retry. Startup, SSE, timer scheduling, marker publication, and shared-room marker v2 are B1.4 or B1.2c work.
- Side plaintext follows the existing ADR-003 diagnostic-room exception. An encrypted side room cannot downgrade to plaintext.

## Boundaries

### Allowed Changes
- backend-v2.js
- bridge-matrix.js
- lib/approval-store.js
- lib/project-side-store.js
- tests/api-approval-projections.test.js
- tests/approval-store-projection.test.js
- tests/project-side-store.test.js
- tests/bridge-agent-credential-record.test.js
- tests/agent-credential-supplied.test.js
- tests/bridge-appservice-send.test.js
- tests/bridge-approval-projection.test.js
- specs/task-approval-publisher-transport.spec.md

### Forbidden

- Native verdict authorization, trusted router origin, global bearer authentication, provider hooks, GUI code, runtime startup hooks, live Matrix calls, marker schema changes, secrets in publisher records, and plaintext fallback for encrypted rooms.

## Acceptance Criteria

Scenario: Publisher registry is durable and private
  Test: publisher registry rotates transactionally and reloads without exposing a credential
  Given a verified publisher scope and opaque credential generation
  When the backend stores or rotates the record
  Then reload returns the exact public identity and generation without any credential
  And a failed pre-rename save restores memory and disk.

Scenario: Backend rejects caller-selected publisher state
  Test: publisher generation is authoritative for prepare begin and retry but not an exact late receipt
  Given a valid bridge secret and canonical projection row
  When prepare, begin, or retry supplies a stale publisher generation
  Then the endpoint returns conflict without changing the projection
  And an exact receipt for a send that durably began remains accepted.

Scenario: Channel actor provenance is preserved
  Test: private status keeps the private request publisher across a binding or credential change
  Given a private request published by one verified actor
  When its terminal private status is prepared after rotation
  Then only the request's original publisher context is accepted.

Scenario: Exact durable plan wins
  Test: prepares once and sends only the winning durable bytes and transaction identity
  Given encryption yields candidate bytes and prepare returns a stored winning plan
  When the helper begins and sends the projection
  Then it sends the stored event type, payload, publisher, and transaction ID exactly once.

Scenario: Rotation blocks network I/O
  Test: rechecks the actual actor after every preparation await and immediately before network
  Given the actual credential generation changes after durable begin
  When the helper reaches the network boundary
  Then it performs zero Matrix requests and records uncertain retry state for reconciliation.

Scenario: Transport uncertainty retains immutable identity
  Test: an ambiguous send failure records retry against the immutable attempted plan
  Given durable begin succeeded
  When Matrix returns an ambiguous failure
  Then retry bookkeeping uses the same plan CAS, publisher generation, event type, payload, and transaction ID without preparing content again.

Scenario: Plaintext diagnostic mode remains explicit
  Test: plaintext approval diagnostics require explicit non-production opt-in
  Given an approval room without encrypted event state
  When the normal security policy checks the room
  Then plaintext is refused unless the existing diagnostic opt-in is enabled outside production.

## Out of Scope

Automatic drain scheduling, marker remote state writes, shared-room marker v2 migration, GUI integration, service restart, and live provider or Matrix operations.
