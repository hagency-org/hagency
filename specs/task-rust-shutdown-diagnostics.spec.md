spec: task
name: "Observe bounded native domain shutdown phases"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, diagnostics, custody]
---

## Intent

Distinguish queued shutdown, repository destruction and acknowledgement delays
without changing service timing or interpreting an unknown outcome as closure.

## Constraints

### Must
- Preserve the existing two-second enqueue and two-second reply waits and exact Result verdicts.
- Keep ordinary shutdown on the same unobserved path without a probe allocation.
- Limit each observed shutdown to one private fixed-size probe and a static host snapshot.
- Publish each phase timestamp before its completed marker with explicit atomic ordering.
- Report only relative monotonic times and static phase/outcome names.
- Keep the repository owned by its existing worker until actual Drop finishes before acknowledgement.
- Preserve failing fixture verdicts and print diagnostics only when the original shutdown fails.

### Must Not
- Do not add runtime authority endpoints serde globals callbacks retries early release or wider deadlines.
- Do not infer rollback worker exit database closure or a root cause from a missing phase.
- Do not expose identifiers paths SQL errors credentials or caller-controlled text in the snapshot.

## Boundaries

### Allowed Changes
- native/hagency-store/src/shutdown.rs
- native/hagency-store/src/domain_worker.rs
- native/hagency-store/src/lib.rs
- native/hagency-store/tests/shutdown.rs
- native/hagency/tests/mcp_coordination/fixture.rs
- native/hagency/tests/runner.rs
- specs/task-rust-shutdown-diagnostics.spec.md
- knowledge/context/native-shutdown-diagnostics.md
- docs/agent-knowledge.md
- docs/progress.md

## Acceptance Criteria

Scenario: Successful shutdown remains actual repository release
  Test: native_domain_shutdown_success
  Given ordinary and observed stores with actual private repositories
  When each store shuts down normally
  Then its original result is successful and repository ownership is released before acknowledgement

Scenario: Queue delay differs from the destruction interval
  Test: native_domain_shutdown_queue
  Given an earlier controlled worker operation that is still held
  When the unchanged reply deadline expires behind that operation
  Then the outcome stays unknown and the snapshot has no worker pickup or Drop marker

Scenario: Worker phase delay is visible without fabricated closure
  Test: native_domain_shutdown_phases
  Given a controlled pause at the Drop-start or acknowledgement boundary
  When the unchanged reply deadline expires
  Then the snapshot identifies only phases actually published and retains the original unknown result

Scenario: Phase snapshots are bounded and independently scoped
  Test: native_domain_shutdown_snapshot
  Given independent observed shutdown probes
  When snapshots are read across publication boundaries
  Then completed markers never appear without their timestamp and snapshots contain only static timing data

## Out of Scope

Attributing historical Windows failures to SQLite disk or scheduling; changing
SQLite close/checkpoint policy; a general telemetry API; new cancellation or
runtime service behavior; full-workspace builds and live deployments.
