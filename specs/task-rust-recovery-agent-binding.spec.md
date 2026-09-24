spec: task
name: "Bind console orphan recovery to the addressed agent"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREAD-SCOPED-SESSIONS]
tags: [active, rust, recovery, console]
---

## Intent

The existing agents/{id}/recover-dispatch route currently only validates the
shape of id. Bind that agent to the original dispatch in the same transaction
that performs recovery, preserving every existing orphan/stop/custody rule.
This closes a concrete routing defect; it does not implement the missing TS
inspection/resolution workflow or authorize settlement of the old live failure.

## Constraints

- The lifecycle scope gate remains required before store work.
- Missing and foreign agent/dispatch combinations return not_found without
  releasing custody, enqueuing work, recording evidence or changing tasks.
- The serialized writer must carry the route's agent ID; no separate read/check
  before a later unscoped mutation can satisfy the binding.
- Preserve direct host repository recovery and all existing stop-row refusals.
- Offline tests only. No new live account, room, fleet or deployment.

## Allowed changes

- native/hagency/src/console/agents.rs
- native/hagency/tests/console/agents.rs
- native/hagency-store/src/domain/execution.rs
- native/hagency-store/src/domain_worker.rs
- native/hagency-store/tests/tasks.rs
- knowledge/decisions/adr-148-operator-recovery-resume.md
- this spec
- docs/**

## Scenarios

Scenario: Console recovery addresses exactly the named agent
  Test: native_console_agent_recover_dispatch_agent_binding
  Given a recoverable orphan owned by one agent and a lifecycle operator
  When recovery names another existing or missing agent in the URL
  Then the request is not_found and all original custody remains unchanged
  And recovery through the owning agent still succeeds

Scenario: The writer enforces recovery identity atomically
  Test: native_dispatch_recovery_agent_binding
  Given an actual started dispatch made unknown by expiry
  When scoped writer recovery names a foreign agent
  Then no recovery is committed and the owning agent may still recover
