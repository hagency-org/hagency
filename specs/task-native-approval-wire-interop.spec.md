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
- schemas/approval/owner-request-v1.schema.json
- schemas/approval/owner-verdict-v1.schema.json
- docs/architecture/owner-ui-approval.md
- specs/task-native-approval-wire-interop.spec.md
- tests/native-approval-wire-interop.test.js
- tests/fixtures/native-approval-wire.json
- native/hagency-store/tests/approvals/card.rs
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

The two JavaScript validator scenarios are bound by the Node runtime contract
`specs/task-native-approval-wire-interop-node.spec.md`; this Rust contract binds
only the Cargo corpus producer.

## Out of Scope

Robrix parser and owned-click origin fixes, encrypted SDK sending, and executable
approval coordination remain separately qualified work. JSON Schema cannot
express the encoded-byte budget; the actual producer and explicit wire validator
tests enforce that separate ceiling. Schema conformance is not event authority.
