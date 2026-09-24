---
spec: task
name: "Bound the ended-engagement record on the retention sweep's engagement phase"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, retention, engagements]
---

## Intent

A terminal engagement whose retention window has passed is removed with the
whole reachable set it owns, child-first, inside one transaction — so the
engagement record stops growing without a refusal of new work, and no engagement
that still carries custody is ever a candidate.

## Constraints

### Must
- Run as phase 4, `engagements`, of the retention sweep tick defined by the
  contract every retention slice cites: its own `Job::Run`, its own `Immediate`
  transaction, child-first to a fixed point, `[engagement]` on refusal, then wait
  for the next tick.
- Count-only, cap `ENDED_LIMIT`, ordered `rowid ASC`; keep the newest window and
  delete oldest-first.
- Admit an engagement as a candidate only when it is terminal and no custody pin
  holds (P1–P7) and every child is already gone or cleared by this phase's own
  cascade in the same tick.
- Pin P5 on the raw dispatch state pair
  (`leased`,`started`,`parked`,`outcome_unknown`), never on the
  `unresolved_dispatches` reporting view.
- Delete `owned_task_completions` in tier 1, before `task_operation_receipts`
  (composite FK) and before `final_replies`; delete `retained_message_archive`
  rows for the pruned engagement in the same transaction.
- Record the ended-at instant in the `engagement_ends` side table, and carry the
  per-id terminal state and child counts in the receipt's `payload`.
- Report a deferred engagement as `remaining > 0`; never refuse admission or an
  operator command.

### Must Not
- Do not add a column to `engagements`: an `ALTER TABLE … ADD COLUMN` would be
  replayed over an already-upgraded table by every fixture that rewinds
  `user_version`, which the store's own rule forbids.
- Do not delete an engagement whose retire effect is `failed` (retryable) or
  whose approval is in the live set, and do not delete a live or unknown-fate
  dispatch reached through it.
- Do not delete `admitted_messages`; do not touch `decisions`.
- Do not replace the FK refusal — the predicate is added, not substituted for it.
- Do not prune `task_outbox` except for rows whose owning task is deleted in the
  same transaction.

## Boundaries

### Allowed Changes
- native/hagency-store/src/migrations/030-engagement-retention.sql (registry slot 30, its number assigned by landing order right after MA-S4 (which kept 029) — the strict one-by-one upgrade loop cannot skip a version, so the ledger's pre-allocation was abandoned)
- native/hagency-store/src/domain/engagement_retention.rs
- the eleven store fixtures that assert the schema head (approvals, ceiling_alerts, conversations, file_delivery, file_uploads, owned_completion, received_files, replies, retention, schema_fixtures, usage tests .rs) — each carries the one literal head-pin assertion the slot-27 move requires
- native/hagency-store/src/domain.rs
- native/hagency-store/src/lib.rs
- native/hagency-store/src/domain_worker.rs (the two public DomainStore wrappers the tick loop submits through)
- native/hagency-store/tests/retention_engagements.rs
- native/hagency/src/bootstrap.rs
- specs/task-rust-retention-engagements.spec.md
- knowledge/decisions/adr-095-native-state-ownership.md
- knowledge/decisions/adr-125-admitted-corpus-retention.md
- docs/progress.md

## Acceptance Criteria

Scenario: The ended-engagement cap is enforced oldest-first by rowid
  Test: native_engagement_prune_enforces_the_cap_oldest_first
  Given more than 500 engagements in a terminal state
  When the engagement phase runs
  Then the oldest terminal engagements are removed and the newest 500 survive
  And a row with a later rowid and a smaller id survives

Scenario: A terminal engagement with live custody survives
  Test: native_engagement_prune_keeps_a_terminal_engagement_with_live_custody
  Given a revoked engagement whose retire effect is failed or uncertain
    and an engagement whose approval is decided applying or uncertain
  When the engagement phase runs
  Then those engagements and all their children remain

Scenario: An owned completion no longer wedges the cascade
  Test: native_engagement_prune_removes_an_owned_completion_with_its_cascade
  Given a terminal engagement whose session ran an owned completion
  When the engagement phase runs
  Then the completion and its dispatch and task and all receipts are gone
  And the completion was deleted before task_operation_receipts and final_replies

Scenario: The archive row is cleared with its engagement
  Test: native_engagement_prune_clears_the_archive_row_with_its_engagement
  Given a terminal engagement with a retained_message_archive row for its id
  When the engagement phase runs
  Then no archive row for that id remains

Scenario: Children are removed with the parent in one transaction and the delete is refused while a child survives
  Test: native_engagement_delete_is_refused_while_any_child_survives
  Given a candidate engagement with a session an effect and all child classes
  When the engagement phase removes it
  Then no child row remains and no orphan is observable
  And at delete time no retire row was started uncertain or failed and no approval was live

Scenario: The receipt names what left and is itself bounded
  Test: native_engagement_prune_receipt_names_what_left
  Given a pass that removed engagements
  When an operator reads the receipt
  Then the row carries phase engagements and the per-id terminal state in its payload
  And the receipt table's row count is at most the contract's receipt limit

Scenario: A pruned engagement id can be re-admitted deterministically
  Test: native_engagement_prune_readmits_a_pruned_id
  Given an engagement that has been pruned
  When the same fleet and request id is admitted again
  Then a fresh pending engagement is created with the same deterministic id
  And its usage periods start from zero
