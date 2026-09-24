spec: task
name: "Inspect exact protected settled-send history without opening the SDK"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-MATRIX-DM-PRIVACY]
tags: [active, rust, matrix, custody]
---

## Intent

Strengthen real operator rollover evidence with exact old-receipt membership
under the protected archive root. The existing read-only operator example must
neither open a second SDK owner nor emit private receipt IDs, content or keys.
This diagnostic is factual only; it cannot settle delivery or authorize a send.

## Constraints

### Must
- Keep the existing private-file, 32-byte key, 1024-byte cipher-export and 16-MiB encrypted-record bounds and read-only SQLite connection.
- Project only bounded hot outgoing receipt count, pending-attempt presence, allowlisted phase and an optional exact requested-receipt result.
- Check requested IDs/fences before private inspection; retain at most 64 hot receipts.
- Authenticate every visited immutable node with the original journal cipher, exact content hash and retained SDK identity.
- Check canonical ID/fence key, strict increasing branch bits, leaf path consistency and at most 256 branches.
- Refuse missing, malformed, foreign-identity or hash-substituted nodes; a corrupt archive cannot be hidden by a hot-cache hit.
- Compare exact hot/archive receipt bytes when both exist; never accept disagreement.
- Keep original protected journal root and all SDK/domain files unchanged.
- Use offline real encrypted SQLite fixtures for deterministic diagnostic tests; only explicit operator invocations inspect live private state.

### Must Not
- Do not export IDs, accepted-attempt digests, roots, identities, bodies, tokens, signing keys or raw nodes.
- Do not reset/open/migrate the SDK, write SQLite, query lifetime receipt inventories, replay HTTP, grant current authority or use the result as first Delivered evidence.
- Do not relabel private diagnostic fixtures as actual SDK sends or entire-port/retention qualification.

## Boundaries

### Allowed Changes
- native/hagency/examples/matrix_custody.rs
- docs/progress.md

## Acceptance Criteria

Scenario: Exact archived receipts are authenticated and redacted
  Test: native_matrix_custody_settled_receipt
  Level: integration
  Test Double: read-only SQLite with real StoreCipher-protected journal/nodes
  Given a private exact settled receipt outside the hot cache
  When the operator supplies its bounded ID and fence
  Then only factual membership, fixed kind and bounded visited-node count are returned without changing the database

Scenario: Missing or substituted proof cannot become an absence or hot-cache success
  Test: native_matrix_custody_settled_receipt_refusals
  Given missing nodes, changed hash or SDK identity, conflicting hot/archive receipts or malformed branch structure
  When an exact receipt is inspected
  Then inspection fails without any private-value projection or state mutation

Scenario: Hot-cache and pending-attempt metadata stay bounded and private
  Test: native_matrix_custody_outgoing_metadata
  Given original private pending content and bounded settled receipts
  When the operator requests only status metadata
  Then only allowlisted counts/presence/phase are projected and excessive cache size refuses

## Out of Scope

This inspector is independent operator evidence, not a runtime receipt authority.
Real SDK send/replay, automatic provisioning, physical retention, media and
entire-port qualification remain separate required parts of the migration goal.
