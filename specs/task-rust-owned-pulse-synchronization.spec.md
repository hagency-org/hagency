spec: task
name: "Observe actual owned-child pulse progress within the existing fixture deadline"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, fixture, custody]
---

## Intent

Remove the unsupported scheduling assumption in the real owned-child stream
closure fixture without changing runtime deadlines or custody guarantees.

## Constraints

### Must
- Preserve the original failed macOS run and identify scheduler delay only as an unproven explanation.
- Keep the existing three-second absolute observation deadline and forty-millisecond owner wait.
- Require actual pulse growth after stream closure and preserve actual stop report and platform-specific tree assertions.
- Add a real gated child that cannot append another pulse before explicit fixture release.
- Keep the gate and heartbeat within the child fixture's eight-second lifetime and retain owned cleanup on failure.
- Use only fixed fixture marker suffixes and bounded synthetic bytes with no live model or service.

### Must Not
- Do not increase production CI or existing fixture deadlines weaken progress assertions or treat a rerun as the original result.
- Do not modify process supervision or runtime protocol implementations.

## Boundaries

### Allowed Changes
- native/hagency-runtime/tests/owned.rs
- native/hagency-runtime/src/bin/hagency-runtime-probe.rs
- specs/task-rust-owned-pulse-synchronization.spec.md
- docs/progress.md
- docs/agent-knowledge.md

## Acceptance Criteria

Scenario: Stream closure preserves observed child progress and owned cleanup
  Test: native_owned_runner_custody_stream_close_does_not_stop_child
  Level: integration
  Test Double: real native fixture child through retained platform ownership
  Given a real child with its stdio streams dropped
  When the original owner wait reports it is still running
  Then a further pulse must be observed under the original absolute deadline
  And the original owner stops the child with unchanged platform custody assertions

Scenario: Gate release controls actual progress independently of sleep scheduling
  Test: native_owned_runner_custody_gated_progress
  Level: integration
  Test Double: real native child waiting for a fixed fixture release file
  Given a live child with initial pulses and no release
  When the parent observes it without releasing its gate
  Then no additional pulse has been written
  And explicit release causes observed growth before the same absolute deadline followed by actual owned stop

## Out of Scope

Runtime supervision changes service behavior live models new timing budgets
and proving the historical hosted scheduler or filesystem cause.
