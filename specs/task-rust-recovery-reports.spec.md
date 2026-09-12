spec: task
name: "Recover completed native task reports without reopening work"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREE-LAYER-COMPLETION]
tags: [active, rust, recovery]
---

## Intent

Allow inspected result reporting after a completed task's process becomes uncertain,
including Matrix intent sessions, without granting new work or task mutation authority.

## Constraints

### Must
- Bind a report dispatch to the inspected original and exact completed task epoch in a durable transaction.
- Recheck current allocation and task intent authority at enqueue claim start and runtime operations.
- Preserve frozen Matrix and peer input through repeated inspected recovery.
- Keep report permission separate from task execution delegation and peer send permission.
- Compare recovery payload changes using the stored finite-number canonical encoding.

### Must Not
- Do not accept runtime JSON as inspection evidence or a report permission grant.
- Do not reopen completed tasks or consume fresh arrivals when reporting an old result.
- Do not launch models or contact live homeservers.

## Boundaries

### Allowed Changes
- native/**
- specs/task-rust-recovery-reports.spec.md
- docs/**

### Forbidden
- Live services, deployed state and credentials.

## Acceptance Criteria

Scenario: Completed Matrix work permits only an inspected report
  Test: native_completed_intent_report
  Given a completed Matrix task an uncertain process and newly admitted input
  When the host inspects the result and replaces the dispatch
  Then the report and its authority commit atomically while the task remains done and new work permissions are denied

Scenario: Report authority survives only within its completed epoch
  Test: native_report_recovery_epoch
  Given an inspected report that becomes uncertain again and queued report authority
  When the host recovers it or its task epoch changes
  Then input remains recoverable and changed task or allocation authority invalidates stale reports

Scenario: Recovery instructions use canonical payload identity
  Test: native_recovery_payload_identity
  Given numerically equivalent JSON instruction payloads
  When inspected recovery compares old and replacement content
  Then equivalent data does not bypass the requirement for a distinct recovery instruction

## Out of Scope

Actual process termination proof, external result delivery, graph orchestration and
production cutover remain subsequent migration work.
