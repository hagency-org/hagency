spec: task
name: "Check that every spec-named store write has a production caller"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, spec-governance, wiring]
---

## Intent

Owe the checker ADR-146 (a) requires: `native/scripts/check-production-callers.mjs`.
The 2026-09-14 wiring audit found nine store flows whose writes only tests and
fixtures reach; this checker makes "a spec `Then` line that names a store write
carries a `Production caller:` line, and that caller is present in the
production call graph" a machine gate so the gap class cannot regrow silently.

## Constraints

### Must
- Collect every `Production caller:` line carried by spec `Then` scenarios naming a store write.
- Build the production call graph with tests (`*/tests/*`, `tests.rs`, `#[cfg(test)]` modules), `native/fixtures/**` and the bootstrap probe (`hagency/src/bootstrap/driver.rs`) stripped.
- Treat a root as any binary entry point, Salvo handler registration, or spawned worker/loop reached from one of those.
- Report `Production caller: owed (Gn)` lines as tracked gaps, never as failures.
- Exit 0 only when every non-owed `Production caller:` name resolves inside the stripped graph; exit 1 listing each absent caller otherwise.

### Must Not
- Do not count a call from a test, fixture or the bootstrap probe as production reachability.
- Do not weaken the gate by substring-matching the caller name against test files.

## Boundaries

### Allowed Changes
- native/scripts/**
- specs/task-rust-production-callers-check.spec.md
- docs/**

### Forbidden
- Root manifests. Changes to store methods or their callers to make the gate pass — a missing caller means the flow is a gap, and the fix is wiring it or marking it `owed (Gn)`.

## Acceptance Criteria

Scenario: A spec-named store write with a wired caller passes
  Owed Selector: native_production_callers_wired
  Given a spec Then line naming a store write with a Production caller line
  When the checker builds the stripped production call graph
  Then the caller resolves from a root and the checker reports it wired

Scenario: A spec-named store write with no production caller fails
  Owed Selector: native_production_callers_missing
  Given a spec Then line naming a store write whose Production caller is absent from the stripped graph
  When the checker runs
  Then the checker exits 1 listing the absent caller and no test fixture or probe call satisfies it

Scenario: An owed gap is reported and never failed
  Owed Selector: native_production_callers_owed
  Given a spec Then line whose Production caller line reads owed with a G-number
  When the checker runs
  Then the gap is reported as owed and the exit status is unchanged

## Out of Scope

The checker defines the gate; closing the G1–G8 gaps it reports is owed by the
owners ADR-146 names. Deletion decisions for superseded methods are separate.
