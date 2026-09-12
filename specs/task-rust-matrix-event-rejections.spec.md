spec: task
name: "Retain conclusive Matrix event refusals without retiring healthy transport"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-MATRIX-DM-PRIVACY, REQ-THREAD-SCOPED-SESSIONS]
tags: [active, rust, matrix, custody]
---

## Intent

Persist bounded terminal event dispositions after known SDK processing, continue
eligible events, and preserve unknown SDK and negative authority fences.

## Constraints

### Must
- Require exact coverage of raw timelines by actual SDK output before deriving dispositions.
- Persist conclusive rejection and non-target tombstones before any eligible event admission.
- Refuse reinterpretation of terminal sources across new sync tokens target plans or crypto trust changes.
- Keep plaintext encrypted-target traffic and missing or unverified crypto proof ineligible without plaintext fallback.
- Retain Applying ambiguity limited timelines storage uncertainty and genuine identity or room failures under existing fences.
- Keep journal history finite private and inspectable without eviction reset or fabricated legacy coverage.

### Must Not
- Do not convert domain handoff authority conflicts into event rejection or loosen current admission.
- Do not add live services models credentials schema approval authority or service availability.

## Boundaries

### Allowed Changes
- native/hagency-matrix/src/event_batch.rs
- native/hagency-matrix/src/event_batch/**
- native/hagency-matrix/src/sdk.rs
- native/hagency-matrix/src/intake.rs
- native/hagency-matrix/tests/intake/**
- native/hagency/tests/owned_matrix.rs
- specs/task-rust-matrix-event-rejections.spec.md
- specs/task-rust-matrix-owned-workflow.spec.md
- knowledge/decisions/adr-065-native-matrix-event-rejections.md
- knowledge/decisions/adr-062-native-matrix-owned-workflow.md
- docs/agent-knowledge.md
- docs/progress.md

## Acceptance Criteria

Scenario: Conclusive bad events do not disable eligible chat
  Test: native_matrix_rejection_continuation
  Given authenticated SDK output containing invalid unsupported or non-target events and valid chat
  When the private disposition ledger persists before admission
  Then rejected events create no domain input while later valid events are admitted and transport remains available

Scenario: Encrypted private refusals remain terminal after trust changes
  Test: native_matrix_rejection_crypto
  Given plaintext missing-key or unverified private events followed by actual verified SDK messages
  When intake records terminal refusal and later receives a new token or changed crypto trust
  Then rejected sources never become eligible while newly sent verified events are admitted without plaintext fallback

Scenario: Tombstones survive restart and changed target plans
  Test: native_matrix_rejection_replay
  Given durable terminal source dispositions
  When a collector reopens or receives repeated sources under another target plan
  Then original rejections remain terminal and changed source content cannot replace their history

Scenario: Unknown SDK effects and negative authority remain fenced
  Test: native_matrix_rejection_custody
  Given coverage mismatch limited timeline persistence rollback or interrupted SDK output
  When intake cannot prove complete derivation
  Then raw custody remains unresolved without admission and identity or room negatives still retire authority

Scenario: Receipt bounds and legacy inspection cannot erase history
  Test: native_matrix_rejection_bounds
  Given full or inconsistent disposition history or old filtered receipts without tombstones
  When intake attempts to continue
  Then bounds and consistency checks refuse without eviction reset fabricated coverage or automatic unknown replay

## Out of Scope

Domain admission refusal redesign, automatic decrypt retry/manual recovery,
receipt compaction, approval intake, full encrypted owned-helper completion,
live enrollment and native cutover remain separate gates.
