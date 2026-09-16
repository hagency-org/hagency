spec: task
name: "Legacy project-side approval plaintext policy"
status: draft
inherits: project
---

## Intent

Align legacy read-only status publication with the established ADR-016 project-side approval room policy while retaining ADR-003 local plaintext restrictions.

## Boundaries

### Allowed Changes
- bridge-matrix.js
- backend-v2.js
- tests/bridge-legacy-approval-projection.test.js
- tests/api-approval-legacy-evidence.test.js
- knowledge/decisions/adr-legacy-approval-original-evidence.md
- specs/task-legacy-approval-original-evidence.spec.md
- specs/task-legacy-approval-projection-bridge.spec.md
- specs/task-legacy-side-approval-policy.spec.md

### Forbidden
- Native projection behavior, scheduler ownership, approval authority, store schema, GUI, Cargo, live Matrix calls, global diagnostic flags, and publisher fallback.

## Acceptance Criteria

Scenario: Verified project-side plaintext remains usable in production
  Test: legacy side uses actual store generation and exact private sender transport
  Level: integration
Given an accepted appservice or registration-token side loaded through the protected acting endpoint
When current membership and exact M_NOT_FOUND prove the private room has no encryption state
Then canonical legacy status is sent under the pinned representative with no diagnostic flags.

Scenario: Local plaintext remains diagnostic only
  Test: legacy API permits only the existing explicit plaintext-test policy
Given a local bot publisher and an unencrypted private room
When production or required mode prepares legacy status
Then the backend rejects it unless all existing non-production diagnostic gates hold.

Scenario: Side security fails closed
  Test: legacy side encrypted or rotated contexts cannot downgrade or continue proof reads
Given a side publisher
When the room is encrypted, security is indeterminate, membership is incomplete, or credentials rotate
Then no plaintext fallback or later Matrix send occurs.

## Out of Scope

Worker integration, native projection policy, marker publication and live deployment remain separate gates.
