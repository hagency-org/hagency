spec: task
name: "Observe unrelated native process liveness and fresh progress"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, platform, fixtures]
---

## Intent

Distinguish an exited unrelated child from a live child whose heartbeat did not
advance during one short sample without weakening process isolation assertions.

## Constraints

### Must
- Require native owned liveness before and after observing a fresh heartbeat within the fixed three-second observation bound.
- Fail immediately on observed exit and separately fail missing progress at the observation deadline.
- Preserve existing launch argument environment cancellation repeated-stop and crash-containment assertions.
- Demonstrate the old short-sample false diagnosis using an actual paused native child and reject its actual later exit.
- Preserve original failed macOS CI evidence and keep its historical cause unknown.

### Must Not
- Do not change production ownership signalling reaping stop deadlines or cleanup guarantees.
- Do not count mere liveness stale heartbeats retries or unsupported guarantees as success.

## Boundaries

### Allowed Changes
- native/hagency-platform/tests/process_scope.rs
- knowledge/decisions/adr-086-native-process-scope-progress-observation.md
- specs/task-rust-process-scope-progress.spec.md
- docs/progress.md
- docs/agent-knowledge.md

## Acceptance Criteria

Scenario: Unrelated process remains alive and makes fresh progress
  Test: native_process_scope_start_stop
  Level: integration
  Test Double: actual native owned and unrelated child processes
  Given a real unrelated process and owned group or job
  When the owned process is stopped
  Then unrelated native liveness and fresh progress are both required
  And original scope and repeated-stop assertions remain intact

Scenario: Paused progress and actual exit remain distinct
  Test: native_process_scope_progress_observation
  Level: integration
  Test Double: actual pausable native child and existing filesystem gate
  Given an alive native child whose heartbeat is deliberately paused
  When a short sample stays unchanged and the child later resumes
  Then the observation requires fresh progress without declaring the paused child dead
  And an actual subsequent exit is refused despite its retained old heartbeat

Scenario: Early-exit and Drop cancellation retain ownership
  Test: native_process_scope_early_exit
  Level: integration
  Test Double: actual native child group or job
  Given an exited leader or a dropped owner with remaining work
  When existing cancellation runs
  Then the original stopped-child and ownership assertions remain exact

Scenario: Crash containment remains proven or refused
  Test: native_process_scope_crash_guarantee
  Level: integration
  Test Double: actual native controlled owner process
  Given an explicit crash-containment request
  When the platform proves or refuses its guarantee
  Then the existing platform-specific result remains unchanged

## Out of Scope

Production lifecycle changes historical CI cause attribution complete descendant
or sandbox qualification live services and entire migration completion.
