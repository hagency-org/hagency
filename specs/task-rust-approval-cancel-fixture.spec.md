spec: task
name: "Cancel native approval writes only after the fixture observes their phase"
inherits: project
satisfies: [REQ-OWNER-UI-APPROVAL, REQ-EXECUTION-AUTHORIZATION, REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, approval, verification]
---

## Intent

Separate approval cancellation during an observed blocked write from lost writer
responses without changing production deadlines or claiming missing application proof.

## Constraints

### Must
- Preserve the original failed Windows CI verdict and distinguish later local or diagnostic results.
- Observe actual durable Applying and the real response writer before starting blocked-write cancellation.
- Keep a bounded phase wait and the original cancellation interval after the phase is known.
- Preserve cancellation-before-write coverage with real committed writer state and no future repoll after cancellation.
- Assert closed transport retained uncertainty and refusal of duplicate application after restart.

### Must Not
- Do not change production code deadlines permissions process cleanup or application authority.
- Do not invent a successful permission application from observed writes or serial diagnostic reruns.
- Do not contact live runtimes Matrix accounts or deployed services.

## Boundaries

### Allowed Changes
- native/hagency-permissions/tests/coordinator.rs
- specs/task-rust-approval-cancel-fixture.spec.md
- knowledge/decisions/adr-046-codex-approval-adapter.md
- docs/agent-knowledge.md
- docs/progress.md

## Acceptance Criteria

Scenario: Cancellation occurs during an observed blocked write
  Test: native_codex_approval_uncertainty_write_cancel_restart
  Given an owner decision consumed by the actual writer and a blocked response stream
  When the fixture observes the first write attempt then cancels its pending future
  Then the session closes without retry and durable Applying becomes Uncertain after reopen

Scenario: Lost committed writer responses are distinct from attempted bytes
  Test: native_codex_approval_uncertainty_attach_and_consume_lost_response
  Given actual writer commits while the caller future remains unpolled
  When the caller drops that future before receiving its response
  Then no response bytes were attempted and retained uncertainty cannot authorize another application

## Out of Scope

Production approval changes, protocol application qualification, changes to any
runtime or store deadline, process cleanup and full Windows rerun remain separate.
