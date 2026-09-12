spec: task
name: "Derive native resource qualification from the accepted model policy"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, resources]
---

## Intent

Complete the model-derived part of M2 using the existing role-capacity policy.
Projects select resources; neither project fields nor provider-supplied role
labels can promote an unqualified model.

## Constraints

### Must
- Embed the existing role-capacity JSON as the single model policy source.
- Preserve provider and reasoning matching, tier subsumption, explicit role exclusions and cheapest-sufficient ordering.
- Derive catalog roles, persist explicit role withdrawal and enforce cross-family review using active Agents on the requested registration.
- Recheck qualification before admission and initial reservation; retain exact replays and reserved recovery identities.
- Upgrade native schemas transactionally with an explicit migration file and verify rollback and repeated startup.

### Must Not
- Do not count unprovisioned resource configurations as active model families.
- Do not accept user-supplied eligible-role lists as qualification authority.
- Do not change live deployments or the accepted model table.

## Boundaries

### Allowed Changes
- native/**
- specs/task-rust-resource-qualification.spec.md
- knowledge/decisions/adr-095-native-state-ownership.md
- docs/**
- .github/workflows/rust.yml

### Forbidden
- Deployed JS runtime, live state, credentials and model policy changes.

## Acceptance Criteria

Scenario: Model policy matches the existing implementation
  Test: qualification_matches_javascript
  Given model profiles with matching missing or contradictory provider and reasoning fields
  When native model tier family and role eligibility are calculated
  Then results and preset ordering match vectors from the JavaScript implementation

Scenario: Catalog and reservation enforce qualification
  Test: domain_qualification_and_cross_family
  Given published resources and active Agents belonging to different registrations
  When roles are withdrawn models change or a review request is submitted
  Then only currently qualified resources and same-registration active families authorize new reservations

Scenario: Native schema upgrade is atomic
  Test: native_schema_migrations_are_atomic
  Given existing native state and a failing migration
  When the database upgrades or opens again
  Then committed version and data are preserved and successful migrations run once

## Out of Scope

Framework executable discovery, real provisioning and full M3–M9 behavior remain
subsequent work. This contract does not mark the entire migration complete.
