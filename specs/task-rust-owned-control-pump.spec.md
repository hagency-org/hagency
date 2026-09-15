spec: task
name: "Retain original owned runner IO while polling finite approval control"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREAD-SCOPED-SESSIONS, REQ-CONTRIBUTION-CONSOLE]
tags: [active, rust, approval, custody]
---

## Intent

Permit private host control to progress alongside the original owned session's
ordered typed observations without cancelling a read or fabricating activity.

## Constraints

### Must
- Retain the original session process streams partial frames and observation sequence in place across successful control returns.
- Borrow the caller's pinned control future without owning cancelling reconstructing or repolling its completed output.
- Prepare the unique original typed response frame synchronously before the host awaits durable response admission.
- Deliver buffered and newly observed callbacks cancellation and usage before any not-yet-written prepared frame.
- Bind fixed owner and response deadlines to each original callback admission and refuse a budget that cannot fit the original transport lifetime.
- Preserve normal event frame protocol write and lifetime deadlines across control wakes buffer drains and unsent-frame continuation.
- Preserve stop-on-drop for every started public session and owned process operation and retain original uncertain write evidence.
- Keep full typed response write and flush distinct from router authority and native permission application.
- Keep each retained callback and prepared response bounded and forbid duplicate or foreign response use.

### Must Not
- Do not change the current execution host's thirty-second operation or two-second response policies.
- Do not introduce background IO tasks another writer driver extraction replay or synthetic keepalive observations.
- Do not add domain schema permission coordinator execution host platform or live-service integration.
- Do not claim a control result response flush or callback resolution proves native permission application.

## Boundaries

### Allowed Changes
- native/hagency-runtime/src/codex/connection.rs
- native/hagency-runtime/src/codex/transport.rs
- native/hagency-runtime/src/codex/transport/control.rs
- native/hagency-runtime/src/codex/session.rs
- native/hagency-runtime/src/codex/session/driver.rs
- native/hagency-runtime/src/codex/session/control.rs
- native/hagency-runtime/src/owned/session.rs
- native/hagency-runtime/tests/session.rs
- native/hagency-runtime/tests/session/control.rs
- native/hagency-runtime/tests/owned.rs
- knowledge/decisions/adr-034-native-codex-transport.md
- knowledge/decisions/adr-036-native-codex-session.md
- knowledge/decisions/adr-040-native-owned-runner-io.md
- knowledge/decisions/adr-070-native-owned-usage-capture.md
- knowledge/context/native-owned-control-pump.md
- specs/task-rust-owned-control-pump.spec.md
- docs/agent-knowledge.md
- docs/progress.md

## Acceptance Criteria

Scenario: Pinned host control and partial input retain original custody
  Test: native_codex_control_partial_frame_and_pinned_future
  Given one running typed session and a pinned host admission future
  When partial input control wakes usage and a new callback interleave
  Then the original future survives ordered observations without read cancellation or sequence gaps

Scenario: Original prepared responses cannot overtake observed cancellation
  Test: native_codex_control_prepared_response_order_and_once
  Given an original prepared typed response and additional received updates
  When send admission drains buffered or ready input
  Then updates are delivered before bytes and resolution or foreign reuse refuses the original frame

Scenario: Owner wait and normal operation deadlines stay finite
  Test: native_codex_control_absolute_deadlines
  Given fixed callback owner response frame event and connection deadlines
  When private control wakes repeatedly or another callback arrives
  Then no original clock is reset and insufficient original lifetime is refused

Scenario: Started response failure keeps accepted bytes uncertain
  Test: native_codex_control_write_cancellation
  Given a prepared response whose original writer is blocked
  When its started send future is dropped
  Then original write evidence survives and the same frame cannot be retried

Scenario: Dropped cooperative operation stops the original owned process
  Test: native_owned_control_drop_stops_original_process
  Given a real guardian-owned offline child and retained session
  When a started cooperative operation is cancelled
  Then the original process owner stops with its existing exact cleanup report

## Out of Scope

Database grant semantics approval permission application execution host integration
new launch budgets native provider qualification Windows runtime qualification and
production cutover remain separate contracts.
