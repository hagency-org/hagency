spec: task
name: "Approval projection production request adapter"
status: accepted
inherits: project
---

## Intent

Connect canonical approval projection rows to the existing bridge actor, security, encryption, and exact Matrix transport seams without starting a worker.

## Decisions

- The adapter reads and mutates projection state only through bridge-secret backend routes.
- Private local content passes the established approval-room security policy and is encrypted once before durable prepare.
- Matrix receives only the durable winning event type, payload, transaction identity, and pinned current actor.
- Native private content includes the canonical binding tuple and selected outbox revision/state while retaining wire version 1.
- A local bot becomes eligible only after the actual started client and crypto device are verified; stored plaintext is security-checked again before replay.
- Private status rows expose their original request publisher through the protected due API and validate that original scope against current authority.
- Scheduling, shared-room markers, and legacy publication remain later checkpoints.

## Boundaries

### Allowed Changes
- bridge-matrix.js
- backend-v2.js
- tests/api-approval-projections.test.js
- tests/bridge-approval-projection-adapter.test.js
- tests/bridge-approval-projection.test.js
- tests/bridge-appservice-send.test.js
- specs/task-approval-projection-bridge-adapter.spec.md

### Forbidden
- Backend changes outside protected projection publisher validation and provenance; approval-store changes, startup timers, SSE listeners, marker publication, GUI, Cargo, live Matrix/provider calls, native verdict authorization, and plaintext downgrade.

## Acceptance Criteria

Scenario: Local encrypted projection follows the durable protocol
  Test: real store/API prepares encrypted bytes once, begins before exact raw PUT, and receipts
  Given a canonical due private request and verified local bot context
  When the adapter prepares and publishes it
  Then security and encryption run before durable prepare
  And durable begin precedes the exact raw Matrix PUT
  And receipt records the returned event without re-encrypting.

Scenario: Existing prepared work reuses winning bytes
  Test: an uncertain durable plan replays stored ciphertext without preparing content again
  Given an uncertain projection with stored encrypted content
  When the adapter retries it
  Then it skips fresh encryption and sends the exact stored payload and transaction identity.

Scenario: Current identity gates transport
  Test: rechecks the actual actor after every preparation await and immediately before network
  Given a pinned projection actor
  When the captured or current credential generation differs
  Then no later Matrix request uses that context.

Scenario: Status keeps original publisher and selected state
  Test: saved bot fields alone are not publisher readiness and selected status state wins
  Given a terminal status whose approval record has since changed
  When the adapter resolves and prepares that selected row
  Then it uses the original private request publisher
  And the wire version remains 1 with the selected revision state decision migration and full binding tuple.

Scenario: Side security observation has one complete response deadline
  Test: side security bounds the complete response:
  Given an authorized current side publisher checks room encryption state
  When headers arrive before a delayed or continuously partial response body
  Then the original deadline aborts the connection before accepting plaintext
  And 429 cloned bodies remain within the same deadline with no internal retry
  And early encrypted or server-error classification releases the unread stream
  And a valid fragmented absence response within the deadline remains accepted.

## Out of Scope

Automatic startup, SSE or timer drain; marker state events; GUI integration; and live deployment.
