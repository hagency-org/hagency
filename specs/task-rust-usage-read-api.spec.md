spec: task
name: "Expose bounded native usage observations to the operator"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, usage, api]
---

## Intent

Expose the durable observation ledger through the existing authenticated native
management boundary without treating untrusted transcript counts as quota truth.

## Constraints

### Must
- Require the existing exact loopback authority and operator bearer authentication.
- Read the known engagement summary and both requested UTC periods in one bounded writer operation.
- Preserve unknown observations absent periods incomplete history and lower-bound labels.
- Return only typed aggregate counts period keys and explicit evidence classification.
- Reject unknown engagements malformed duplicate unknown or oversized query fields.
- Preserve the original store queue deadlines and explicit unavailable or unknown results.

### Must Not
- Do not expose source records task identifiers workspace paths owner rooms source digests or credentials.
- Do not add observation writes source restoration usage enforcement or runner permission through HTTP.
- Do not enable live capture transport execution a browser console or production parity.

## Boundaries

### Allowed Changes
- ./Cargo.lock
- native/hagency/Cargo.toml
- native/hagency/src/lib.rs
- native/hagency/src/usage.rs
- native/hagency/tests/usage.rs
- native/hagency/tests/usage/**
- native/hagency-store/src/domain/usage.rs
- native/hagency-store/src/domain/usage/reads.rs
- native/hagency-store/src/domain/usage/types.rs
- native/hagency-store/src/domain.rs
- native/hagency-store/src/domain_worker.rs
- native/hagency-store/src/lib.rs
- knowledge/decisions/adr-067-native-usage-read-api.md
- specs/task-rust-usage-read-api.spec.md
- docs/agent-knowledge.md
- docs/progress.md

## Acceptance Criteria

Scenario: Usage observations require operator authority
  Test: native_usage_api_authority
  Level: integration
  Test Double: Salvo request transport over a fresh real writer
  Given persisted usage and the native management service
  When credentials authority headers or HTTP methods are invalid
  Then no usage counts or source secrets are returned and no mutation is exposed

Scenario: Usage reads preserve unknown and historical evidence
  Test: native_usage_api_projection
  Level: integration
  Test Double: fresh canonical execution fixtures and actual normalized ledger records
  Given a known engagement with absent partial regressed and later complete observations
  When the authenticated operator reads a selected UTC period
  Then optional latest counts historical lower bounds and incomplete indicators survive without leaking source records

Scenario: Query failures and missing state remain explicit
  Test: native_usage_api_refusals
  Level: integration
  Test Double: actual native service with missing closed or fresh domain writer
  Given malformed duplicated unknown oversized or out-of-range queries and unavailable state
  When a usage read is requested
  Then the exact refusal remains visible rather than reporting zero usage

## Out of Scope

Browser integration secure transcript capture provider measurement quota enforcement
historical source export and deployed service cutover remain separate work.
