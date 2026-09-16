spec: task
name: "Preserve existing project-side credentials across approval publisher upgrade"
status: draft
inherits: project
satisfies: [REQ-APPROVAL-CANONICAL-PROJECTION]
---

## Intent

Existing project-side files predate outboundGeneration. Initialize that metadata durably before the new approval publisher resolver reads it, retaining the actual credential and every accepted side/binding decision.

## Decisions

- On store construction, add missing generation metadata to active and staged credentials with an existing outbound token. Include inactive sides so later activation is stable. Never issue a token or infer access acceptance.
- Preserve existing valid generations. Registration credentials without a representative token remain unable to act. Invalid nonmissing generation metadata fails closed.
- Persist all additions once through existing protected atomic storage before returning a usable store. Persistence failure aborts construction; no generation exists only in a live reader's memory.
- A second load or a store without eligible missing metadata performs no write. Underlying load/save remains proportional to retained state.

## Boundaries

### Allowed Changes
- lib/project-side-store.js
- tests/project-side-generation-upgrade.test.js
- specs/task-side-generation-upgrade.spec.md
- knowledge/observations/project-side-generation-upgrade.md

### Forbidden
- Token issuance, credential rotation, active/access state changes, binding/approval changes, network probes, new dependencies, global settings, live runtime writes

## Acceptance Criteria

Scenario: Existing appservice credentials remain usable
  Test: old appservice generation is durably added without changing authority
  Given a version1 side file contains an accepted appservice credential without generation metadata
  When the store loads and then reloads
  Then one stable generation is persisted and all prior fields are preserved.

Scenario: Representative and staged metadata stay distinct
  Test: old representative and staged generations preserve pending and inactive state
  Given inactive and staged credentials coexist with a registration credential without a representative token
  When the store loads
  Then eligible existing outbound tokens gain independent stable metadata without activation or token creation.

Scenario: Unchanged stores are not rewritten
  Test: current and empty stores do not write during construction
  Given generations are already present or no outbound token exists
  When the store loads
  Then the source bytes remain unchanged and no save occurs.

Scenario: Existing rotation semantics survive migration
  Test: migrated generation survives equal credentials and changes on actual token rotation
  Given a migrated appservice credential
  When the same token is stored and later a changed token is stored
  Then only the changed token receives a new generation.

Scenario: Failed persistence cannot expose an ephemeral generation
  Test: failed migration persistence aborts construction and preserves original credentials
  Given the destination parent becomes unavailable after reading the old file
  When migration tries to save
  Then construction fails and the original credential file remains intact.

Scenario: Malformed metadata is explicit
  Test: malformed existing generation fails closed without rewriting the file
  Given a credential contains invalid nonmissing generation metadata
  When the store loads
  Then it reports persistence failure without replacing that metadata silently.

Scenario: Actual protected endpoint exposes the migrated identity
  Test: protected acting API exposes durable upgraded generation while public projection stays private
  Given the backend starts from an old version1 appservice or representative-token side file
  When its real protected acting endpoint is read
  Then its generation equals the durable store value and public side responses expose neither credential nor generation.
