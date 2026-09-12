spec: task
name: "Retain exact receive grant identity and post-lock current workspace authority"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREAD-SCOPED-SESSIONS, REQ-MATRIX-DM-PRIVACY]
tags: [active, rust, attachments, authority]
---

## Intent

Close two concrete ADR105 prerequisite gaps found during sink integration:
check_owned_dispatch must sample current time inside its original SQLite
transaction, and the opaque receive write must expose an exact read-only
capability association check. Neither helper grants replacement write authority.

## Constraints

### Must
- Sample check time after the original writer queue and SQLite lock acquisition and check all scope facts in one transaction.
- Compare the complete original validated capability digest, including dispatch runner fence and secret.
- Return false for malformed or different capabilities without exposing stored secrets or a setter.
- Preserve all existing lease durations failure handling and original writer ownership.

### Must Not
- Do not create another domain owner change schema add retries or extend deadlines.
- Do not claim SDK file or complete incoming workflow proof from domain checks.

## Boundaries

### Allowed Changes
- native/hagency-store/src/domain/owned_dispatch.rs
- native/hagency-store/src/domain/received_files.rs
- native/hagency-store/src/domain_worker.rs
- native/hagency-store/tests/received_files.rs
- specs/task-rust-receive-authority.spec.md
- docs/agent-knowledge.md
- docs/progress.md

## Acceptance Criteria

Scenario: Workspace check samples the current clock after original lock and queue waits
  Level: integration
  Test Double: original domain writer and independent real SQLite lock inspection
  Test: native_receive_workspace_check_clock
  Given a started original dispatch and independently held writer queue
  When its lease expires before the queued workspace check runs
  Then the check refuses and the same transaction clock runs only under the acquired SQLite lock

Scenario: Original write grant matches only its complete capability
  Level: integration
  Test Double: actual domain reservation and unique committed WritePossible grant
  Test: native_receive_write_capability
  Given an original unique write grant
  When any capability identity field changes or is malformed
  Then the opaque read-only match refuses and only the exact original matches

## Out of Scope

Execution sink service MCP physical file proof and platform activation remain
separate accepted partitions and require their own actual tests.
