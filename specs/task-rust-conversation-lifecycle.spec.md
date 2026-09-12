spec: task
name: "Retire native conversation membership without reviving old work"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREE-LAYER-COMPLETION]
tags: [active, rust, conversations]
---

## Intent

Allow current conversation creators to change participants or close internal
groups while retaining history and fencing obsolete execution authority.

## Constraints

### Must
- Bind mutations to the current exact creator session and a content-bound receipt with an expected revision.
- Restrict new participants to active allocations in the same project and registration generation.
- Allocate a fresh session when a removed participant rejoins and retain the retired session history.
- Fence affected queued leased started and parked dispatches atomically with membership changes.
- Retain started-work resource custody until host inspection settles a durable stop intent.
- Preserve canonical task status and immutable input history during retirement.
- Close child conversations whose creator session is retired.
- Keep stop inspection host-only and never accept runtime JSON as process termination evidence.

### Must Not
- Do not treat group closure or resource release as task completion or proof that a process stopped.
- Do not expose host stop settlement through the runner API.
- Do not contact live homeservers or launch models.

## Boundaries

### Allowed Changes
- native/**
- specs/task-rust-conversation-lifecycle.spec.md
- knowledge/decisions/adr-030-native-conversation-retirement.md
- docs/**

### Forbidden
- Live services, deployed state and credentials.

## Acceptance Criteria

Scenario: Membership changes are atomic and scoped
  Test: native_conversation_membership_lifecycle
  Given current creators and stale foreign or participant callers
  When membership changes are applied retried or fail during persistence
  Then revision receipts and fresh participant sessions commit together without reviving old authority

Scenario: Closure fences execution and retains uncertain custody
  Test: native_conversation_close_custody
  Given queued leased started parked and nested conversation work
  When a creator closes the group and the host restarts or inspects stopped work
  Then obsolete capabilities are fenced resources remain held until inspection and tasks are never marked done

Scenario: Closure releases valid input without claiming delivery
  Test: native_conversation_close_inbox
  Given a creator dispatch containing peer input from a closing group
  When closure fences that dispatch and inspection settles it
  Then history remains unacknowledged and unrelated live input can be scheduled separately

Scenario: Runner HTTP exposes creator lifecycle only
  Test: native_runner_http_conversation_lifecycle
  Given authenticated scoped runners and forged lifecycle fields
  When callers change or close a conversation through the private API
  Then validated mutations reach the single writer and host inspection remains inaccessible

## Out of Scope

This is internal group lifecycle, not Matrix room membership or transport. Actual
process stop observation and sandbox proof remain obligations of the future host
runner adapter. No production cutover is authorized by these fixtures.
