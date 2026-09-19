spec: task
name: "Classify the pinned migration entrypoints with source evidence"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, inventory, build-tools]
---

## Intent

Replace the candidate-only M0 list with checked source registrations and explicit
migration ownership. Source enumeration is not native implementation parity.

## Constraints

### Must
- Parse JavaScript registrations without executing application modules or contacting services.
- Record source paths locations hashes resolved registration arguments and migration gates.
- Resolve dynamic inputs within the declared AST detector scope or fail explicitly without silently skipping a recognized route.
- Document unsupported syntax and runtime reachability as detection gaps rather than implying a whole-program call graph.
- Classify every runtime helper build script installer and service entry candidate explicitly.
- Keep source-derived inventory deterministic and reject unreviewed drift.
- Preserve requirement and release gates for runtime versions hardware and unported behavior.

### Must Not
- Do not infer deployed reachability or native parity from source registration alone.
- Do not import application entrypoints read live credentials or mutate deployed state.

## Boundaries

### Allowed Changes
- native/scripts/**
- native/README.md
- ./package.json
- ./package-lock.json
- native/fixtures/legacy-inventory.json
- native/fixtures/inventory-policy.json
- tests/native-migration-inventory.test.js
- specs/task-native-migration-inventory.spec.md
- knowledge/decisions/adr-035-native-migration-inventory.md
- docs/**

### Forbidden
- Production source code credentials running services and other working trees.

## Acceptance Criteria

Scenario: Registration paths are resolved from parsed source
  Test: native_inventory_routes_resolve_registration
  Given literal computed chained defaulted and delegated registration fixtures
  When the source inventory runs
  Then every accepted registration has source evidence and unresolved input is refused

Scenario: Entrypoint classification fails closed on drift
  Test: native_inventory_classification_rejects_drift
  Given changed source new helpers and incomplete ownership policies
  When the inventory is checked
  Then missing or stale classifications fail rather than becoming completed migration rows

Scenario: The checked legacy inventory reproduces exactly
  Test: native_inventory_reproduces_current_sources
  Given the pinned legacy source corpus and explicit migration policies
  When the inventory is regenerated without executing runtime code
  Then the committed inventory file matches without runtime imports or credential reads and all implementation parity gates remain open

## Out of Scope

These are Node/Vitest build-tool tests. Cargo-only agent-spec lifecycle cannot run
these selectors; exact Vitest execution and binding checks remain required.
M0 event/schema/store/feature traceability, chosen runtime versions, measured
hardware budgets and production cutover are not established by this inventory.
