spec: task
name: "Align the finite native and retained owner approval v1 wire profiles"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-OWNER-UI-APPROVAL, REQ-EXECUTION-AUTHORIZATION]
tags: [active, approvals, protocol, rust]
---

## Intent

Implement ADR115's explicit paired peer-schema amendment in an isolated tree at
1baa80d. Preserve exact native forty-hex and retained thirty-two-hex request IDs,
typed upstream metadata and the already accepted finite scoped choices. Validate
actual original-writer cards, without changing domain or Matrix authority.

## Constraints

### Must
- Admit exactly thirty-two or forty lowercase hexadecimal characters after approval_ without rewriting the identifier.
- Preserve legacy field limits; bound the native profile by its existing 48 KiB encoded packet limit with explicit schema field ceilings.
- Keep private detail objects closed, preserve exact bindings and require reusable scope for task or always choices.
- Keep once and deny in canonical order and reject duplicate unknown reordered or unscoped reusable actions.
- Validate checked-in fixture bytes captured from the actual original-writer card test and retain reproducible generation evidence.
- Run exact Node selectors separately from the Cargo-only lifecycle and report skips as nonpassing.

### Must Not
- No producer authority, permission, request ID, native schema version, client, runtime or live service change.
- No truncation, display-based permission, schema validation as authentication, or complete client interoperability claim.

## Boundaries

### Allowed Changes
- knowledge/decisions/adr-115-native-approval-wire-interop.md
- knowledge/decisions/adr-110-native-private-approval-card.md
- knowledge/decisions/adr-143-native-approval-wire-oracle.md
- schemas/approval/owner-request-v1.schema.json
- schemas/approval/owner-verdict-v1.schema.json
- docs/architecture/owner-ui-approval.md
- specs/task-n...[credential-redacted].spec.md
- specs/task-n...[credential-redacted].spec.md
- tests/native-approval-wire-interop.test.js
- tests/fixtures/native-approval-wire.json
- native/hagency-store/tests/approvals/card.rs
- native/hagency-matrix/tests/fixtures/approval-vectors.json
- native/scripts/approval-vectors.mjs
- docs/progress.md
- docs/agent-knowledge.md

### Forbidden
- No production Rust JavaScript client or transport source changes.

## Acceptance Criteria

Scenario: The corpus comes from real current private writer cards
  Test: native_approval_wire_corpus
  Given original native requests in actual isolated SQLite fixtures
  When current private cards are created for fixed operation and scope cases
  Then exact emitted JSON is recorded without substituting a hand-authored producer

Scenario: The native approval and origin rules match the executed retained producer
  Test: native_approval_origin_vectors_match_retained_proxy
  Level: integration
  Test Double: regenerated oracle vectors computed by the retained bridge and proxy
  Given the regenerated approval vectors produced by executing the retained request, notice, verdict and origin rules
  When the native encoder, verdict parser and console origin predicate replay each row
  Then packet shapes, accept and refuse verdicts, action lists and origin decisions agree with the retained result for every row, and the named divergences where the profiles differ are asserted rather than left implicit

Scenario: A drifted retained producer fails the oracle instead of re-blessing it
  Test: native_approval_oracle_pins_drift
  Level: unit
  Test Double: checked-in sha256 pins over both retained sources
  Given the checked-in approval vectors carrying the sha256 of every retained source they executed
  When either retained source changes without regenerating the fixture
  Then the oracle check fails naming the drifted file, and no native rule is silently re-blessed against the new source

The JavaScript validator scenarios are bound by the Node runtime contract
`specs/task-n...[credential-redacted].spec.md`; this Rust contract binds
the Cargo corpus producer and the two oracle-comparison scenarios above.

## Out of Scope

Robrix parser and owned-click origin fixes, encrypted SDK sending, and executable
approval coordination remain separately qualified work. JSON Schema cannot
express the encoded-byte budget; the actual producer and explicit wire validator
tests enforce that separate ceiling. Schema conformance is not event authority.
