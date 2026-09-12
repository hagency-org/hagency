spec: task
name: "Port selected-resource allocation calculations to Rust"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, resources]
---

## Intent

Begin M2 with the deterministic selected-pool and shared-seat calculation from
ADR-025. Keep pending reservations, unknown quotas and automatic-join policy
consistent with the current implementation.

## Constraints

### Must
- Bind Rust calculations to golden vectors produced by the current JavaScript function.
- Preserve unknown remaining capacity as null rather than zero.
- Exclude the retried engagement without releasing other reservations.
- Reject token counts outside the JSON-safe integer range and arithmetic overflow.

### Must Not
- Do not create an approval endpoint or treat a computed budget as execution authority.
- Do not mutate existing pools, project allocations, Matrix accounts or runtime data.

## Boundaries

### Allowed Changes
- native/**
- specs/task-rust-allocation-budget.spec.md
- .github/workflows/rust.yml
- ./.gitattributes
- docs/**

### Forbidden
- Existing runtime implementation and live state.

## Acceptance Criteria

Scenario: Rust preserves pool and seat policy
  Test: allocation_vectors_match_javascript
  Given selected resources with active and pending commitments and optional seat declarations
  When native remaining capacity is calculated
  Then its full projection matches the JavaScript golden vectors including null capacity

Scenario: Unsafe arithmetic is refused
  Test: allocation_rejects_unsafe_token_arithmetic
  Given invalid token counts or commitments whose sum exceeds the JSON-safe range
  When a budget is decoded or accumulated
  Then the calculation fails rather than reporting wrapped or imprecise capacity

## Out of Scope

M2 admission, ownership, transactional approval and fulfillment remain separate
implementation work. The native development API still cannot allocate Agents.
