spec: task
name: "Check that every spec-named store write has a production caller"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, node, spec-governance, wiring]
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
- Build the production call graph with `#[cfg(test)]` items and `#[test]` fns removed, and `*/tests/*` (including `tests.rs`), `native/fixtures/**` and the probe/fixture binaries stripped: the bootstrap probe `hagency/src/bootstrap/driver.rs`, `hagency-platform/src/bin/hagency-platform-probe.rs`, `hagency-platform/src/bin/hagency-cgroup-probe.rs`, `hagency-progress-runtime/src/bin/hagency-progress-probe.rs`, `hagency-runtime/src/bin/hagency-runtime-probe.rs`, `hagency-runtime/src/bin/approval_probe/`, and the `hagency/tests/fixtures/*.rs` `[[bin]]` peers (`owned_mcp_peer.rs`, `file_mcp_peer.rs`, `receive_mcp_peer.rs`, `approval_mcp_peer.rs`, `matrix_crypto_peer.rs`).
- Treat a root as the product binaries and services only: the `hagency` bin (`hagency/src/main.rs`), its Salvo handler registrations, the workers and sweeps spawned from it, and the MCP stdio entry (`mcp/stdio.rs` via `main.rs`) — never a probe or fixture bin, so a probe-only write can never score wired.
- Report `Production caller: owed (Gn)` lines as tracked gaps, never as failures; the `Gn` must resolve to a row in ADR-146's gap table and an unknown id fails the checker.
- Exit 0 only when every non-owed `Production caller:` name resolves from a root in the stripped graph and every `owed (Gn)` id resolves; exit 1 listing each absent caller or unknown gap id otherwise.

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
  Test: native_production_callers_wired
  Given a spec Then line naming a store write with a Production caller line
  When the checker builds the stripped production call graph
  Then the caller resolves from a root and the checker reports it wired

Scenario: A spec-named store write with no production caller fails
  Test: native_production_callers_missing
  Given a spec Then line naming a store write whose Production caller is absent from the stripped graph
  When the checker runs
  Then the checker exits 1 listing the absent caller and no test fixture or probe call satisfies it

Scenario: An owed gap is reported and never failed
  Test: native_production_callers_owed
  Given a spec Then line whose Production caller line reads owed with a G-number that resolves to a row in ADR-146's gap table
  When the checker runs
  Then the gap is reported as owed and the exit status is unchanged

Scenario: An unknown owed gap id fails the checker
  Test: native_production_callers_unknown_gap
  Given a spec Then line whose Production caller line reads owed with a G-number that names no row in ADR-146's gap table
  When the checker runs
  Then the checker exits 1 listing the unknown gap id

## Out of Scope

The checker defines the gate; closing the G1–G8 gaps it reports is owed by the
owners ADR-146 names. Deletion decisions for superseded methods are separate.
