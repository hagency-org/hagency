spec: task
name: "Validate the paired owner approval v1 wire profiles from the retained JavaScript side"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-OWNER-UI-APPROVAL, REQ-EXECUTION-AUTHORIZATION]
tags: [active, approvals, protocol, node]
---

## Intent

Bind the Vitest half of ADR115's paired peer-schema amendment. The Rust
contract `specs/task-native-approval-wire-interop.spec.md` binds the Cargo
corpus producer; the spec-binding checkers select a runtime per contract, so
the JavaScript validator scenarios need their own Node contract.

## Constraints

- Validate real corpus packets from `tests/fixtures/native-approval-wire.json`
  against `schemas/approval/owner-request-v1.schema.json` and
  `schemas/approval/owner-verdict-v1.schema.json` without rewriting bindings.
- No production JavaScript client or transport source changes.

## Boundaries

### Allowed Changes
- tests/native-approval-wire-interop.test.js
- tests/fixtures/native-approval-wire.json
- schemas/approval/owner-request-v1.schema.json
- schemas/approval/owner-verdict-v1.schema.json
- specs/task-native-approval-wire-interop-node.spec.md

### Forbidden
- All paths outside this exact partition and all live operations.

## Acceptance Criteria

Scenario: Both accepted peer profiles validate without rewriting bindings
  Test: native approval wire profiles
  Given actual native cards and retained JavaScript producer controls
  When request and verdict schemas validate each finite scope choice
  Then complete original IDs and bindings survive and typed RPC metadata stays request-only

Scenario: Malformed and oversized profiles never become schema-valid
  Test: native approval wire refusals
  Given valid original packets
  When IDs scopes ordering metadata bounds or closed detail fields change
  Then invalid packets refuse and exact legacy limits remain unchanged

## Out of Scope

Robrix parser and owned-click origin fixes, encrypted SDK sending, and executable
approval coordination remain separately qualified work.
