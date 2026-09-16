spec: task
name: "Approval projection Matrix transport seam"
inherits: project
tags: [approval, matrix, projection, transport]
satisfies: [REQ-APPROVAL-CANONICAL-PROJECTION, REQ-OWNER-UI-APPROVAL]
---

## Intent

Extend the existing project-side representative transport so an immutable approval projection plan can send its prepared Matrix payload with its final transaction ID and authenticated publisher unchanged. Add the matching empty-state-key marker write without creating another general delivery system.

## Constraints

- Existing seed-based message callers retain their current URL and identity behavior.
- Projection sends require the complete tuple of final transaction ID, prepared event type, and expected publisher MXID.
- Appservice sends use the configured sender localpart through Matrix masquerade; registration-token sends use the verified representative MXID without masquerade.
- Invalid or mismatched projection context fails before fetch.
- Tests use request-capture fixtures and contact no Matrix service.

## Decisions

- Final transaction IDs match `[A-Za-z0-9._~-]{1,128}` and are placed verbatim in the send URL.
- Prepared event type is limited to `m.room.message` or `m.room.encrypted`.
- The marker helper writes a caller-supplied custom event type at the empty state key; Matrix state PUT has no transaction ID.
- Shared actor selection is the single authority for token, publisher MXID, and appservice masquerade.

## Boundaries

### Allowed Changes
- lib/matrix-representative.js
- tests/matrix-representative.test.js
- specs/task-approval-projection-transport.spec.md

### Forbidden
- backend-v2.js
- bridge-matrix.js
- lib/approval-store.js
- Credential stores or schemas
- New dependencies

## Acceptance Criteria

Scenario: Final projection transaction and event type are sent unchanged
  Test: projection send uses final transaction id and prepared event type verbatim
  Given an authenticated project-side representative and a complete projection send context
  When the prepared payload is sent
  Then the captured URL contains the exact final transaction ID and prepared event type

Scenario: Existing seed callers retain derived transactions
  Test: IDEMPOTENT BY SEED
  Given an existing seed-based representative send
  When the same seed is sent twice
  Then both captured URLs contain the same derived transaction ID

Scenario: Appservice and registration-token publishers remain distinct
  Test: projection publisher binding follows authenticated actor mode
  Given each supported project-side credential mode
  When its expected publisher is exact
  Then appservice uses user_id masquerade and registration-token uses no masquerade

Scenario: Projection publisher mismatch fails before transport
  Test: projection send rejects mismatched publisher before fetch
  Given the expected publisher differs from the authenticated representative
  When projection send is requested
  Then no fetch occurs

Scenario: Partial or invalid projection send context is rejected
  Test: projection send rejects invalid final context before fetch
  Given a missing tuple field, invalid transaction ID, or unsupported prepared event type
  When projection send is requested
  Then no fetch occurs

Scenario: Empty-key marker uses the authenticated representative
  Test: approval marker writes custom empty state key through actor binding
  Given an exact expected publisher and custom marker event type
  When the marker is written
  Then the captured state URL ends at the encoded event type and empty state key
  And appservice masquerade follows the shared actor selection

Scenario: Marker identity mismatch and invalid input fail closed
  Test: approval marker rejects mismatched publisher and invalid state input
  Given mismatched publisher, nonempty state key, or invalid custom event type
  When marker write is requested
  Then no fetch occurs

Scenario: Prepared projections cannot use ordinary direct-chat encryption
  Test: canonical projection refuses ordinary direct-chat re-encryption without changing normal sends
  Given a managed direct-chat destination and a canonical durable projection plan
  When sendAsAgentContent reaches the direct-chat transport boundary
  Then it reports approval_projection_direct_chat_unsupported before sending or encrypting
  And ordinary non-projection direct messages still use their existing transport

## Out of Scope

- Local-bot encryption and bridge orchestration
- Credential generation and projection authorization
- Projection store preparation, begin, uncertainty, retry, and receipt lifecycle
- Live Matrix validation
