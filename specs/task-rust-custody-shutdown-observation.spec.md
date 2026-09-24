spec: task
name: "Observe the original custody worker shutdown without changing its verdict"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, diagnostics, custody]
---

## Intent

Distinguish queue pickup repository release and acknowledgement delays when a
Palpo custody fixture shutdown fails without claiming a historical root cause.

## Constraints

### Must
- Keep the original two-second enqueue and acknowledgement waits and original Result verdict.
- Drop the actual Repository and release its database ownership before acknowledging shutdown.
- Reuse independent fixed monotonic phase timestamps with no payloads paths or secrets.
- Preserve the original Palpo success or failure while emitting a phase snapshot only on failure.
- Keep ordinary shutdown unobserved without allocating a probe.

### Must Not
- Do not retry shutdown widen deadlines infer rollback or move release after acknowledgement.
- Do not replace original full-suite results with later diagnostic results.
- Do not add new runtime or service authority to diagnostic observations.

## Boundaries

### Allowed Changes
- native/hagency-store/src/worker.rs
- native/hagency-palpo/tests/transport.rs
- knowledge/decisions/adr-082-native-custody-shutdown-observation.md
- specs/task-rust-custody-shutdown-observation.spec.md
- docs/progress.md
- docs/agent-knowledge.md

## Acceptance Criteria

Scenario: Successful shutdown preserves custody release
  Level: integration
  Test Double: actual private SQLite repository and owned worker
  Test: native_custody_shutdown_release
  Given a real private repository and its worker
  When ordinary and observed shutdown complete
  Then both release database ownership before returning success
  And observed success records phases without changing authority

Scenario: Paused actual worker exposes queue and release stages
  Level: integration
  Test Double: actual worker with private phase gates and original timeout
  Test: native_custody_shutdown_phases
  Given a real worker paused at independent shutdown phases
  When the original acknowledgement deadline expires
  Then the original timeout remains a failure with the exact observed stage
  And releasing the original worker never rewrites its caller verdict

Scenario: A full queue remains an enqueue timeout
  Level: integration
  Test Double: actual paused worker plus an explicitly unconsumed bounded queue
  Test: native_custody_shutdown_queue
  Given a bounded shutdown queue with no receiving worker
  When its unchanged enqueue deadline expires
  Then no worker or repository-release phase is inferred

Scenario: The actual Palpo publication restart flow keeps its result
  Level: integration
  Test Double: actual local HTTP peer and private custody database
  Test: native_outbound_http_publication_frozen_restart_and_rotation
  Given an actual local HTTP publication fixture and durable custody
  When its first store shutdown is observed before reopen
  Then publication replay and generation assertions remain exact
  And any shutdown error still fails with a static redacted phase diagnostic

## Out of Scope

Changing runtime shutdown policy transport waits SQLite checkpoints retry behavior
workflow verdicts live services or claiming the cause of an uninstrumented failure.
