spec: task
name: "Coordinate genuine factory project-room membership generations"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-MATRIX-DM-PRIVACY]
tags: [active, rust, matrix, fleet, authority]
---

## Objective

Implement ADR153 so two genuine factory agents can coexist in one project after
the second agent joins, while every original scope and negative observation
retains its existing authority and cleanup meaning.

## Constraints

- Only the original warm factory shares the finite project observation guard.
  It serializes prior-read/HTTP/publication/recheck, conveys no positive evidence,
  is cancellable and uses existing SDK wait limits. No new credential/SDK owner.
- Only original project Group snapshots use the new writer CAS; ordinary room
  publication and all Direct/approval checks are unchanged. Missing/changed
  available-generation evidence refuses; no retry or unavailable-scope revival.
- Exact snapshots retain a generation. Changed joined membership advances once through the
  same original validator/transaction and retire old group routes/work/permissions.
  Encryption, invite policy, privacy, owner and registration changes refuse.
  Never alter old session/reply bindings to make them current again.
- Missing owner/sender or malformed/foreign identity cannot become positive.
  Lost writer results remain unknown and retain existing negative fencing.
- Do not hide the original failure with separate projects, fixture-seeded target
  authority, skipped membership checks, larger deadlines, reset or silent retry.
- Offline tests only; no dependencies, formatter, live deployment, commit or PR.

## Allowed changes

- native/hagency-store/src/domain/matrix_routes.rs
- native/hagency-store/src/domain_worker.rs
- native/hagency-store/tests/replies.rs
- native/hagency-matrix/src/config.rs
- native/hagency-matrix/src/collector.rs
- native/hagency-matrix/src/provisioning.rs
- native/hagency-matrix/src/token_provision.rs
- native/hagency/tests/configured_fleet.rs
- native/hagency/tests/configured_fleet/**
- knowledge/decisions/adr-153-native-factory-room-generations.md
- this spec, docs/progress.md, docs/agent-knowledge.md

## Scenarios

Scenario: Authenticated project refresh advances once and retires only old routes
  Test: native_factory_group_generation
  Given original current project and independent private-DM scope
  When a matching original writer observation adds a genuine project member
  Then exactly one new generation retires old group authority and leaves the DM current
  And identical observations reuse the generation without further retirement

Scenario: Missing stale unavailable and unsafe observations cannot restore authority
  Test: native_factory_group_generation_refusals
  Given existing original room and transport generations
  When old absent unavailable foreign or unsafe observations are submitted
  Then no stale positive or recovered generation is minted and unsafe membership is fenced

Scenario: One configured executable operates two original agents in one project
  Test: native_configured_fleet_executable_two_agents
  Given the original failing same-project two-agent fixture for both account kinds
  When actual membership changes during physical provisioning and recurring intake
  Then both original runtimes complete independently through the native helper and encrypted replies
