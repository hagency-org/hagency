spec: task
name: "Retain finite JSON numbers in native execution payloads"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, dispatch]
---

## Intent

Allow structured runner input and future peer/graph results to contain fractional
JSON numbers without weakening the integer-only signed authority DTO contract.

## Constraints

### Must
- Encode finite execution-payload numbers with JavaScript-compatible shortest decimal formatting.
- Preserve sorted UTF-16 object keys array-index ordering negative zero normalization and nesting limits.
- Keep signed authority DTO encoding restricted to JSON-safe integers.
- Bind queued payload receipts to the full structured numeric content across retry and restart.
- Canonicalize stored execution data under JavaScript double semantics and reject prototype fields.

### Must Not
- Do not stringify fractional results or discard structured fields.
- Do not change authority token count generation or timestamp numeric validation.

## Boundaries

### Allowed Changes
- native/**
- ./Cargo.lock
- ./Cargo.toml
- .github/workflows/rust.yml
- specs/task-rust-payload-json.spec.md
- docs/**

### Forbidden
- Live services, legacy source and deployed state.

## Acceptance Criteria

Scenario: Finite JSON encodings match JavaScript
  Test: native_payload_number_vectors
  Given JavaScript-derived finite numeric payload vectors with sorted UTF-16 keys array-index ordering nesting limits and protected authority DTOs
  When native canonical encoders validate and format them
  Then execution payloads retain decimals under JavaScript double semantics while signed authority DTOs still reject fractions and unsafe integers

Scenario: Numeric dispatch receipts survive restart
  Test: native_payload_dispatch_replay
  Given a queued structured payload containing fractional values
  When it is retried restarted claimed and started
  Then the same content replays and altered numeric content conflicts without losing fields

## Out of Scope

Internal peer mailbox, durable graph integration and actual runtime execution.
