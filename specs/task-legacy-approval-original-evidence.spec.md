spec: task
name: "Legacy approval original evidence store and API"
status: accepted
satisfies: [REQ-APPROVAL-CANONICAL-PROJECTION, REQ-OWNER-UI-APPROVAL]
---

## Intent

Persist positively attested historical approval origins separately from canonical requests. Supply the trusted bridge with read-only legacy status content under the original actual sender, without manufacturing native request plans or approval authority.

## Decisions

- A bridge-secret-only attestation accepts at most 16 KiB of normalized two-event evidence. The bridge authenticates retrieval and decryption; backend validation establishes tuple consistency and current publisher registry/side credential authority, not independent Matrix transport proof.
- Evidence binds the selected legacy private_status CAS and the stored candidate verdict event ID, complete owner/DM/request/agent/project/project-room/digest tuple, owner verdict action and reply target, and original sender/runtime/upstream/expiry. Missing, edited, redacted, mixed-protocol or mismatched evidence is rejected.
- Evidence is immutable and idempotent in a separate legacyOriginalEvidence map. Historical credential generation remains null; the first read-only status plan pins the verified current private publisher context. The full MXID stays equal to the original actual sender; rotation blocks begin/retry while exact attempted receipts remain valid.
- GET returns canonical com.agentchat.approval.status.v1 content, version 1, migration_kind legacy_v1 and the original reply relation, with no actions or private preview. Legacy event types are exact: padded types are rejected before plaintext comparison. Plaintext preparation must match it; encrypted bytes are prepared once by the trusted bridge. Local plaintext retains the explicit non-production ADR-003 policy; a current side representative uses the distinct ADR-016 project-side plaintext policy and cannot publish to an encrypted room.
- Attestation commits no canonical request/binding changes or additional projection rows. Existing pre-rename rollback and post-rename committed/degraded semantics remain unchanged. Native publisher validation and native private_request plans are unchanged.

## Boundaries

### Allowed Changes
- lib/approval-store.js
- knowledge/decisions/adr-legacy-approval-original-evidence.md
- backend-v2.js
- tests/helpers/approval-legacy-fixture.js
- tests/approval-legacy-evidence.test.js
- tests/api-approval-legacy-evidence.test.js
- specs/task-legacy-approval-original-evidence.spec.md

### Forbidden
- Matrix proof retrieval, bridge drain/scheduler, provider calls, native approval authorization changes, canonical request or binding mutation, global authentication, dependencies, runtime state, and live services.

## Acceptance Criteria

Scenario: Protected attestation supplies canonical readonly wire
  Test: legacy attestation persists only authenticated bridge evidence and returns readonly wire
Given one migrated legacy status and complete normalized two-hop evidence
When the bridge attests the original and reads the result
Then only the bridge secret is authorized and the returned consumed status retains allow and the original relation without actions.

Scenario: Evidence is exact and immutable
  Test: legacy evidence rejects tuple relation and CAS conflicts without mutation
Given a legacy status with canonical owner DM and candidate verdict ID
When a tuple, event role, sender, relationship, path or CAS is changed
Then attestation rejects and both memory and disk retain their prior bytes.

Scenario: Duplicate evidence survives restart without canonical mutation
  Test: legacy evidence is idempotent after reload without canonical or queue mutation
Given accepted legacy evidence
When the exact evidence is repeated and the store reloads
Then the same evidence remains durable with historical generation null and no request, binding or projection changes.

Scenario: Evidence preserves atomic persistence semantics
  Test: legacy evidence rolls back before rename and retains committed degraded state after rename
  Level: integration
  Test Double: temporary-directory filesystem with commit-point failure injection
Given injected pre-rename and post-rename filesystem failures
When evidence is persisted
Then pre-rename failure restores memory and disk while post-rename retains committed evidence and reports degraded health.

Scenario: Legacy status pins current credentials and preserves exact receipt
  Test: legacy status pins current publisher and rejects rotation while accepting exact attempted receipt
Given attested evidence and a prepared legacy status plan
When the current publisher generation rotates
Then prepare or begin/retry cannot replace the pinned identity but an exact attempted receipt remains accepted.

Scenario: Readonly preparation rejects fabricated actionable content
  Test: legacy plaintext status must equal canonical readonly content
Given accepted original evidence
When preparation changes state, relation, tuple or adds actions
Then it rejects without a plan and canonical content remains read-only.

Scenario: Current private context is mandatory
  Test: legacy API accepts current project-side plaintext and rejects encrypted or stale side context
Given an accepted side representative with current credentials
When evidence or send preparation uses the positively unencrypted side room, encrypted side room, or stale credentials
Then the API accepts only the current ADR-016 plaintext context and otherwise rejects without mutation.

Scenario: Native approvals cannot use legacy authority
  Test: native rows cannot attest legacy evidence or borrow its status exception
Given a native request projection
When the legacy evidence path is used
Then it rejects and normal native publisher checks remain unchanged.

Scenario: Evidence API reports committed durability failure
  Test: legacy API persists evidence atomically and reports committed degraded health
  Level: integration
  Test Double: actual Express and temporary-directory filesystem
Given a write fault before and after rename
When the bridge attests evidence through the API
Then rollback returns the existing persistence failure response without a wake and a committed write reports degraded health with readable evidence.

Scenario: Ambiguous current identity remains unresolved
  Test: legacy API rejects two currently valid private contexts for the same original sender
Given matching local and side registry identities with valid current credentials
When original evidence is attested before the first status pin
Then both proposals return 409 without choosing a context or mutating evidence.

Scenario: Direct store sends retain the current pin
  Test: legacy store send operations independently reject rotated registry but retain receipts
Given a legacy status plan whose publisher registry rotates
When direct store begin or retry is attempted
Then both operations reject without mutation and an exact attempted receipt remains accepted.

Scenario: Normalized observations exclude raw content
  Test: legacy normalized evidence rejects raw bodies secret fields and oversized senders
Given a normalized original evidence envelope
When raw content, token fields, edited content or an oversized sender is included
Then attestation rejects without changing memory or disk.

Scenario: Reads require exact CAS and malformed contexts fail closed
  Test: legacy API requires explicit CAS and rejects absent publisher objects without internal errors
Given an unprepared legacy row
When a read omits its CAS or attestation omits its publisher object
Then the API returns 409 without using an undefined plan-token match or an internal error.

Scenario: Event type normalization cannot bypass readonly content validation
  Test: legacy plaintext API rejects padded event type without mutation under explicit test policy
Given the existing explicit plaintext-test policy and accepted original evidence
When preparation supplies a padded message type with forged state or approval actions
Then it returns 400 with unchanged state and disk; even a padded type with canonical content is rejected.

## Out of Scope

Historical Matrix GET/decryption, legacy retry discovery, provider/runtime execution, frontend rendering, marker v2 integration and complete bridge publication are separate units. Node/Vitest selectors run separately. This unit invokes installed agent-spec with only lint and boundaries layers to honor Node-only execution; skipped lifecycle scenarios are never reported as passing.
