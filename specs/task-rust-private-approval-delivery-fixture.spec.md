spec: task
name: "Observe the private approval delivery leg through a test-only fixture"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, approvals, delivery, fixture]
---

## Intent

Bind the PC-C0b fixture slice: make the private-approval delivery leg
**observable in the service composition without any production switch**.
PC-C0 wires the pump; its wiring selector (`native_private_approval_delivery_is_wired`)
is red on the hosted lanes because the composition never delivers — the worker
environment is fixed at `config.rs` (HOME/CODEX_HOME only, no offline-mode
switch), the executable is sha256-pinned per configured path, and the
composition never network-enrolls the approval collector. This slice closes
that observation gap with **test-only** machinery: a callback-capable runtime
probe selected by the test's own pinned executable path, and a scripted
second-identity enrollment of the approval collector against the shared fake
peer. Nothing about the production launch shape changes.

## Constraints

### Must
- Select the callback-capable runtime probe by the test's **own pinned executable path**: the sha256 pin is per configured executable, so the fixture pins its own callback-capable binary and the production executable's pin is never touched.
- Network-enroll the approval collector through a **scripted second-identity flow** against the shared fake peer, so the delivery leg runs against the same in-process peer the composition already uses — no live homeserver, no production endpoint.
- Keep the wiring observation inside the composition: the delivery leg is driven by the pump PC-C0 wired (`bootstrap/approval.rs`), observed end to end by the test's own support.
- The fixture and its helpers exist only under test support; no production code path references the callback-capable binary or the scripted enrollment.

### Must Not
- Do not touch `native/hagency/src/bootstrap/config.rs`'s production launch shape (HOME/CODEX_HOME only, no `HAGENCY_OFFLINE_MODE` or any offline switch).
- Do not touch the production executable's sha256 pin or any `[[bin]]` layout change.
- Do not touch any production code path: no runner capability, workspace access, console route, or MCP tool changes; the wiring itself is PC-C0's, landed.
- Do not weaken PC-C0's Then clauses or rename any of its other selectors; the moved selector binds here with the same name and semantics.
- Do not gate any scenario by OS or feature in the binding set.

## Boundaries

### Allowed Changes
- native/hagency/tests/bootstrap/approval.rs
- native/hagency/tests/support/ (the callback-capable runtime probe fixture and the scripted second-identity enrollment helpers)
- native/fixtures/ (the callback-capable probe's pinned binary manifest/vector, if any)
- native/hagency/tests/bootstrap.rs
- native/hagency/Cargo.toml (the test-probe `[[bin]]` registration only, mirroring the three sibling probes)
- specs/task-rust-private-approval-delivery-fixture.spec.md
- specs/task-rust-private-approval-wiring.spec.md
- knowledge/decisions/adr-138-bounded-native-approval-observation.md
- docs/progress.md

### Forbidden
- Live services, live homeservers, credentials in tree, deployed state.
- native/hagency/src/bootstrap/config.rs; the production executable's sha256 pin; any production code path; native/hagency/src/bootstrap/approval.rs (PC-C0's wiring, landed); native/hagency-matrix/**.

## Acceptance Criteria

Scenario: A service-composed run delivers a request end to end
  Test: native_private_approval_delivery_is_wired
  Level: integration
  Test Double: the test-only callback-capable runtime probe pinned by its own executable path, and a scripted second-identity enrollment of the approval collector against the shared fake peer; no live homeserver
  Given the composition with the fixture probe selected by the test's own pinned path and the collector enrolled against the shared fake peer
  When an owned run raises an approval notice and the pump takes the single-consumer receiver
  Then the card is built from the admitted domain request and send_private_approval_card completes under the cancellation token
  And the pump terminates by itself when the worker drops the sender at run end
  And the delivery leg is observed without any production switch and no runner capability workspace access or driver runtime reaches the pump

## Decisions

**The wiring selector is bound by this slice.**
`native_private_approval_delivery_is_wired` is the selector whose observation
this slice exists to make possible; it was parked as an owed name while the
PC-C0b code did not exist (no `Test:` line, because the checker binds only
names that exist) — the `Test:` line is **restored in this slice's scenario
above** with the same name and the same end-to-end semantics, and the
`#[test] fn` in `native/hagency/tests/bootstrap/approval.rs` carries it
(this slice's Allowed Changes own that file). The PC-C0 wiring spec keeps its
other selector (`native_private_approval_delivery_wiring_refuses_without_enrollment`)
bound where it is, untouched.

**Test-only, never production.** The callback-capable probe and the scripted
enrollment exist only under test support; the production launch shape
(`config.rs`'s HOME/CODEX_HOME-only environment and the sha256-pinned
executable) is the documented boundary this slice observes around, never
bends.

## Out of Scope

PC-C0's wiring itself (landed), its other selector, the fail-closed denial
policy (PC-C1), the observation surface (PC-C2), the MCP pair (PC-C3), the
oracle vectors (PC-C4), and the re-issue decision (PC-C5).
