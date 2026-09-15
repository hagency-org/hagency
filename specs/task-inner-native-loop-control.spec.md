spec: task
name: Control the original periodic audit loop with correlated native evidence
inherits: project
satisfies: [REQ-INNER-LOOP-MONITOR]
tags: [herdr, loop, recovery, evidence]
---

## Intent

Prepare the periodic-loop controls needed by the authorized full Robrix →
HAgency → native middle → Herdr/Octoloop recovery acceptance. Extend the
reviewed readonly-plan controller to create one fixed60second loop, then
pause, resume or delete that same loop without replaying uncertain actions.

## Constraints

### Must
- Retain the original goal identity and exact process/session/trace binding.
- Require an empty complete loop list before create and exactly one matching owned loop before control.
- Match exact loop ID, creation time, prompt digest, fixed interval and nested scope.
- Publish immutable intent before each mutation and retain partial failed RPC evidence.
- Verify each successful mutation through its correlated response and a fresh complete loop list.
- Preserve unsupported, failed, skipped and uncertain results without claiming full acceptance.
- Exercise local real-child fixtures; tests must not contact live services.

### Must Not
- Send fire-now commands, recreate a paused loop, alter the goal objective or retry an unknown operation.
- Treat schedule creation or active status as evidence of natural scheduled execution.
- Write business implementation, checkpoints or receipt data; change live deadlines or old observer guards.
- Delete a foreign loop or accept a missing or malformed observation as proof.

## Decisions

- Node22 ESM with existing built-in helpers; no dependency or runtime deployment change.
- Add loop-create, loop-pause, loop-resume and loop-delete to the existing plan CLI.
- loop-create carries loop_template with prompt, prompt_sha256, mode=fixed_interval and interval_seconds=60. The readonly plan pins these approved prompt bytes.
- Other loop operations carry expected_loop with loop_id, created_at_ms, prompt_sha256, mode and interval_seconds. Fresh response data supplies the raw prompt; stdout does not disclose it.
- Create/resume/delete require an originally paused or blocked goal, complete terminal-only turns and Herdr idle. Pause may let a current turn settle and never claims session idle.
- Loop mutation parameters follow actual R5 protocol: create includes profile_id; pause/resume/delete include session_id and loop_id only. Nested records still require exact profile and session.
- The helper verifies observed identity before/after but has no atomic admission or compare-and-set guarantee. Post-intent conflicts remain outcome_unknown without rollback.

## Boundaries

### Allowed Changes
- skills/hagency-inner-loop/scripts/native-control.mjs
- skills/hagency-inner-loop/SKILL.md
- tests/inner-native-control.test.js
- tests/fixtures/fake-herdr-native-control.mjs
- specs/task-inner-native-loop-control.spec.md
- knowledge/decisions/adr-034-native-periodic-loop-control.md
- docs/superpowers/plans/2026-09-13-native-loop-control.md

### Forbidden
- router/**
- backend-v2.js
- bridge-matrix.js
- Live runtime state, lower business trees and frozen E2E artifacts

## Acceptance Criteria

Scenario: Create one fixed periodic loop
  Test: creates one fixed interval loop and verifies the fresh scoped list
  Given an idle bound session with the original paused goal and no loop
  When a fixed60second approved prompt is submitted once
  Then the actual create response and new list identify the same active loop without claiming an audit ran

Scenario: Control the original loop without replacing it
  Test: pauses resumes and deletes only the original loop identity
  Given the sole bound loop with its original prompt schedule and creation time
  When pause resume or delete is requested through the exact native protocol
  Then the same identity is confirmed in the response and fresh list

Scenario: Reject invalid scope and creation eligibility
  Test: rejects invalid loop templates and unavailable creation preconditions
  Given an invalid prompt schedule active goal existing loop or active turn
  When loop creation is requested
  Then no loop mutation is sent

Scenario: Reject changed existing loop identity
  Test: rejects changed missing and ambiguous loop identities before control
  Given missing duplicate foreign or changed loop records
  When an existing loop control is requested
  Then no loop mutation is sent

Scenario: Preserve an uncertain delivered creation
  Test: retains a delivered loop create without response and refuses reuse
  Given a delivered loop create that updates state but has no response
  When observation expires and the exact operation is requested again
  Then the intent and partial request remain while no second create is sent

Scenario: Reject conflicting mutation responses and stale deletion proof
  Test: rejects conflicting loop responses and a deleted loop still listed
  Given a changed loop response or a fresh list contradicting deletion
  When a single loop mutation returns
  Then the outcome remains unknown with no corrective mutation or retry

Scenario: A loop pause does not imply a settled turn
  Test: pauses a loop during active work without claiming idle
  Given the original loop and an active native turn
  When one pause is applied
  Then the loop is paused but the report does not claim session idle

Scenario: Preserve prompt bytes through the actual native parser
  Test: rejects loop prompt whitespace that the native parser would trim
  Given a pinned prompt with leading or trailing whitespace
  When creation is requested
  Then it rejects before delivery instead of creating different prompt bytes

Scenario: Delete an active schedule from an idle session
  Test: deletes the original active loop from an idle paused goal without claiming turn cancellation
  Given the sole original active loop with a paused goal and terminal history
  When deletion is requested
  Then the loop is removed without claiming that a concurrent turn was cancelled

## Out of Scope

- Replacing natural scheduled audit, interruption, restart or full Matrix acceptance with fixture tests.
- Atomic CAS, cross-platform process adaptation, global operation-ID storage or general arbitrary native commands.
