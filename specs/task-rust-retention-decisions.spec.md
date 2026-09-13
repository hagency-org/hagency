---
spec: task
name: "Bound the decision receipt inside the deciding command's own transaction"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, retention, decisions]
---

## Intent

The `decisions` table — one command receipt per verdict — is bounded to a
`rowid` window enforced where the verdict is written, so the corpus stops
growing without a sweeper, a timer, or a refusal of new work. The replay
identity a command may still need is never pruned out from under a legitimate
first retry.

## Constraints

### Must
- Trim in-write, inside the deciding command's own transaction, after
  `record_decision`; add no phase, no period and no bootstrap constant.
- Count-only, cap `DECISION_RETENTION_LIMIT`, ordered by `rowid` alone; never
  delete the maximum-`rowid` row; at most `DECISION_PRUNE_BATCH` per command.
- Exclude every `retry_cleanup` decision from the candidate set, by recomputing
  `decision_digest("retry_cleanup", id)` in Rust over each candidate's stored
  `result` and comparing the stored `digest` exactly — no `LIKE` on a digest and
  no new column.
- Write one `retention_prune_receipts` row with `phase='decisions'`,
  `oldest_ref`/`newest_ref` = `decisions.rowid`, trimmed by the shared clause.
- Roll back with the failing command: a rolled-back verdict carries no receipt.

### Must Not
- Do not refuse a legitimate **first** `retry_cleanup` — the reason the command
  is excluded rather than refused.
- Do not add `created_at` or any caller-supplied clock as an ordering key.
- Do not create a second receipt table or a decision-specific prune surface.
- Do not prune `effects`: it is bounded per engagement by
  `UNIQUE(engagement_id,kind)` and is anchored on the engagement.
- Do not add a table, column or `ADD COLUMN` migration.

## Boundaries

### Allowed Changes
- native/hagency-store/src/domain.rs
- native/hagency-store/src/domain_worker.rs
- native/hagency-store/tests/domain.rs
- native/hagency-store/tests/retention_decisions.rs
- specs/task-rust-retention-decisions.spec.md
- knowledge/decisions/adr-095-native-state-ownership.md
- knowledge/decisions/adr-125-admitted-corpus-retention.md
- docs/progress.md

## Acceptance Criteria

Scenario: The decision window is bounded oldest-first by rowid
  Given more than 500 decisions
  When a new command records its decision
  Then the oldest decisions beyond the window are gone and the newest 500 survive
  And the row with the maximum rowid is never deleted
  And a receipt row with phase decisions records the prune

Scenario: A legitimate first retry_cleanup is never refused because of the prune
  Given a pruned decision row for an older approve command
  When retry_cleanup runs with a fresh command_id for a revoked engagement whose retire effect is failed
  Then the retire effect is reset to pending and the command records its decision

Scenario: A retried retry_cleanup leaves the failed retire effect unchanged
  Given a pruned engagement identifier and a retire effect in state failed
  When retry_cleanup is retried with the same command_id
  Then the stored result replays and the retire effect is unchanged

Scenario: The prune rolls back with the failing command
  Given a command that fails after recording its decision
  When the transaction rolls back
  Then the decision count is unchanged the previously-oldest surviving command_id is present
  and no receipt row was written

Scenario: The receipt is itself bounded
  Given more than 100 decision receipts
  When one more prune writes its receipt
  Then the receipt table holds at most 100 rows

## Out of Scope

**Owed test selectors (parked, not bound).** Every selector below is owed: none
is a `#[test] fn` on the integration base `review/retention-1`, so binding it as a
`Test:` line would bind a name that does not exist and the spec-binding gate would
fail. Each scenario above therefore stands **unbound** — its name, its given, its
when and its then are kept — and the selector it will carry once its slice's code
lands on `<head-of-slice-1>` is parked here. Integration moves a line back to a
`Test:` under its scenario when that selector exists.

- owed `native_decision_prune_keeps_the_newest_and_never_the_live_effect` — scenario "The decision window is bounded oldest-first by rowid"
- owed `native_decision_prune_never_refuses_a_legitimate_first_retry` — scenario "A legitimate first retry_cleanup is never refused because of the prune"
- owed `native_decision_replay_window_is_bounded` — scenario "A retried retry_cleanup leaves the failed retire effect unchanged"
- owed `native_decision_prune_rolls_back_with_the_failing_command` — scenario "The prune rolls back with the failing command"
- owed `native_decision_prune_receipt_records_what_left` — scenario "The receipt is itself bounded"
