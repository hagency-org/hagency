spec: task
name: "Own bounded native receive jobs and their original workspace destinations"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREAD-SCOPED-SESSIONS, REQ-MATRIX-DM-PRIVACY]
tags: [active, rust, matrix, attachments, lifecycle]
---

## Intent

Implement the root-accepted ADR105 application ownership partition. Root accepted
this bounded partition on 2026-09-11 under the operator's migration authorization.
The separate sink and adapter partitions supply their concrete dependencies;
real incoming executable qualification remains required for the full workflow.

## Constraints

### Must
- Keep the same original Shared Collector DomainStore and WorkspaceAccess.
- Capture the absolute deadline before admission and keep two live jobs one fixed synchronous worker and one bounded command queue.
- Reserve permanent domain quota before GET and retain unique write ownership before any sink effects.
- Associate each task completion and panic with its exact original job and retain ambiguous sources and destinations through caller loss and failed close.
- Release completed checked bytes and live job slots only after Ready and original task completion while retaining bounded original read-only cache owners.
- Revalidate exact original facts current ticket current workspace and retained file readback before every successful path response.
- Use closed request and safe response schemas with fixed errors and explicit default-disabled startup configuration.

### Must Not
- Do not use per-request blocking workers another writer Collector hidden authority test proof setters write retries or historical metadata as path authority.
- Do not claim the full executable workflow or platform qualification passes from this application partition alone.

## Boundaries

### Allowed Changes
- native/hagency/src/receive_service.rs
- native/hagency/src/receive_service/types.rs
- native/hagency/src/receive_service/job.rs
- native/hagency/src/receive_service/worker.rs
- native/hagency/src/receive_service/pipeline.rs
- native/hagency/src/receive_service/recovery.rs
- native/hagency/src/bootstrap.rs
- native/hagency/src/bootstrap/config.rs
- native/hagency/src/bootstrap/driver.rs
- native/hagency/src/bootstrap/workspace.rs
- specs/task-rust-receive-service.spec.md
- knowledge/decisions/adr-105-native-receive-file-workflow.md
- docs/progress.md
- docs/agent-knowledge.md

### Forbidden
- No domain schema Matrix transport execution sink HTTP MCP runtime helper or live service changes belong to this partition.

## Acceptance Criteria

Scenario: Safe receive projection refuses authority and destination injection
  Test: native_receive_service_projection
  Given closed event-only input and generated relative destination output
  When invalid metadata destinations hashes size or unknown fields are supplied
  Then the request or output refuses before it can represent a successful receive

Scenario: Live replay and worker completion retain original job identity
  Test: native_receive_service_owned_jobs
  Given the bounded application registry and actual worker completions
  When the original caller is dropped and exact requests replay
  Then original jobs remain associated and unknown completion cannot release another job

## Out of Scope

The separately owned sink transport adapters and actual encrypted executable
acceptance remain integration gates; application-only checks do not replace them.
