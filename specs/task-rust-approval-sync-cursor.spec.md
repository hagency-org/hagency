spec: task
name: "Retain distinct approval sync responses at one Matrix cursor"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-OWNER-UI-APPROVAL, REQ-EXECUTION-AUTHORIZATION]
tags: [active, rust, matrix, approvals]
---

## Intent

Fix the receipt conflict observed in the explicitly authorized isolated Codex
fleet. Matrix next_batch is a cursor; distinct response bodies may share it.
Use exact cursor and response digest receipt identity, as ordinary intake does.

## Constraints

- Preserve each distinct response's protected prepare/apply/derive/ack custody.
- Replay only an exact cursor and digest pair, without acquiring new targets.
- Keep source tombstones, immutable-event conflict detection and finite bounds.
- Never infer authentication from a cursor, erase uncertain custody or retry a send.
- Test only against local fixtures, never live external services in Cargo tests.

## Boundaries

### Allowed Changes
- native/hagency-matrix/src/sdk/approval_intake.rs
- native/hagency-matrix/tests/approval_intake/mod.rs
- specs/task-rust-approval-sync-cursor.spec.md
- knowledge/decisions/adr-166-approval-sync-cursor.md
- docs/agent-knowledge.md
- docs/progress.md
- docs/plan.md

## Acceptance Criteria

Scenario: Distinct idle responses at the same cursor preserve private intake
  Test: native_matrix_approval_same_cursor_distinct_responses
  Given two different empty approval sync responses sharing next_batch
  When intake processes them and the original SDK reopens
  Then both receipts survive and exact replay creates no additional receipt
  And a subsequent encrypted owner verdict is accepted exactly once

Scenario: Source identity remains immutable at a reused cursor
  Test: native_matrix_approval_same_cursor_changed_source_refuses
  Given a retained source outcome at a sync cursor
  When the response changes that source's content without advancing the cursor
  Then intake refuses and retains uncertain response custody without a grant

## Out of Scope

Unbounded receipt retention, synthetic SDK cursors, recovery of lost SDK effects,
or treating deterministic fixtures as live qualification.
