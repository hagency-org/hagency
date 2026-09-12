spec: task
name: "Persist native project resource admission and allocation"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, resources, security]
---

## Intent

Implement the M2 domain transaction in ADR-025 and ADR-095: project requests,
selected-resource reservations and recoverable provisioning intents share one
authoritative domain database. Adapter observations remain distinct from input.

## Constraints

### Must
- Normalize Unicode Agent names and preserve current public/runtime identities using JavaScript golden vectors.
- Verify complete Matrix identities, observed source content, reception, project authority and private owner room before admission and again before initial approval.
- Recheck generation, publication, qualification, pool and shared-seat capacity in the reservation transaction.
- Persist reservations and effect intents atomically; ambiguous external outcomes retain reservations and require reconciliation.
- Keep private owner rooms and internal resource identifiers out of public projections.
- Run database work on a bounded dedicated worker; expose only operator resource management until authenticated transport is connected.

### Must Not
- Do not turn custody fixtures or client-supplied authorization flags into verified admission.
- Do not apply legacy project-side budgets to project-defined selected-resource requests.
- Do not retry an ambiguous provisioning operation or claim Matrix/runtime provisioning has run.

## Boundaries

### Allowed Changes
- native/**
- ./Cargo.toml
- ./Cargo.lock
- specs/task-rust-domain-allocation.spec.md
- knowledge/decisions/adr-095-native-state-ownership.md
- docs/**
- .github/workflows/rust.yml
- tests/documented-env-vars-exist.test.js

### Forbidden
- Existing deployed runtime implementations, credentials and live state.

## Acceptance Criteria

Scenario: Project identity matches existing behavior
  Test: project_identity_vectors_match_javascript
  Given accepted and rejected Unicode definitions and scoped request identities
  When native identity validation and derivation run
  Then results match the current JavaScript implementation

Scenario: Admission requires authentic scoped evidence
  Test: project_authority_fails_closed
  Given mismatched source events, owners, room policies or registration scope
  When the domain receives a request or an approval verification
  Then it refuses admission without trusting a display name or localpart

Scenario: Request replay preserves identity and publication rules
  Test: domain_request_replay_and_publication
  Given project-defined requests with normalized names and selected resources
  When requests replay, collide or follow withdrawal
  Then exact replays retain identity and new unqualified requests are refused

Scenario: Atomic reservations enforce selected pool and shared seat
  Test: domain_reservations_are_atomic
  Given competing requests and an injected outbox commit failure
  When approvals attempt to reserve capacity
  Then commitments never exceed capacity and failed commits reserve nothing

Scenario: Recovery never repeats uncertain effects
  Test: domain_effect_recovery_and_revocation
  Given a started provision operation followed by restart or revocation
  When the service recovers or reconciles an observed outcome
  Then ambiguous work stays fenced and cleanup remains independently visible

Scenario: Native resource API isolates operator management
  Test: native_resource_management_is_authenticated
  Given an operator and unauthenticated or browser-origin clients
  When they create and query native resources
  Then only authorized management succeeds and no fixture grants Agent execution

## Out of Scope

Live Matrix IO, actual runner provisioning, legacy role-only allocation and full
console parity remain subsequent migration work. This contract does not mark
the complete M2–M9 migration done.

The existing JavaScript documentation source scan must exclude Cargo build output
while retaining real extensionless scripts. Its Vitest regression is run separately
from this Cargo lifecycle; no Node scenario is counted as a Cargo pass.
