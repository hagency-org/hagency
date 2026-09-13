---
spec: task
name: "Bound the per-dispatch execution corpus on the retention sweep's execution phase"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, retention, execution]
---

## Intent

A settled dispatch's execution evidence — its runner outputs and its receipt
family — is bounded, so the per-dispatch corpus stops growing without a delete.
Nothing that still carries dispatch or attempt authority, and nothing whose
fate is unknown or whose completion is unpublished, is ever a candidate.

## Constraints

### Must
- Run as phase 3, `execution`, of the retention sweep tick defined by the
  contract every retention slice cites (its own `Job::Run`, its own `Immediate`
  transaction, never one transaction for the tick; `[retention]` on refusal,
  then wait for the next tick).
- Prune only a dispatch whose `state IN ('completed','superseded')` **and**
  `capability_hash IS NULL`; keep the newest `EXECUTION_RETENTION_DISPATCHES`
  settled dispatches, oldest-first, under the per-table backstop and the tick's
  batch hypothesis.
- Keep the newest 1 accepted `runner_outputs` row per `(dispatch_id, fence)`,
  even inside a pruned dispatch — a deliberate residue, bounded and never
  expiring.
- Write one `retention_prune_receipts` row with `phase='execution'`,
  `oldest_ref`/`newest_ref` = `runner_dispatches.id`, carrying `pruned`,
  `remaining` and `elapsed_ms`.
- Report an over-window corpus as `remaining > 0`; never refuse new work.

### Must Not
- Do not delete any `runner_attempts` row: both late paths authenticate against
  the attempt row and neither consults a clock.
- Do not make an `outcome_unknown` dispatch, a dispatch listed by
  `unresolved_dispatches`, or a dispatch whose owned completion is `held` a
  candidate — the state pair is the release proof.
- Do not prune `task_outbox`; do not prune `canonical_tasks`, `task_graphs`,
  `graph_nodes` or `graph_dependencies`.
- Do not archive the pruned content: no supported surface reads it.
- Do not add a second cadence, a second receipt table, or a private timer.

## Boundaries

### Allowed Changes
- native/hagency-store/src/migrations/NNN-execution-retention.sql (NNN is allocated from the backlog's migration ledger, section 0.2, when the slice is implemented; it is not the Slice 1 head)
- native/hagency-store/src/domain.rs
- native/hagency-store/src/domain/execution.rs
- native/hagency-store/src/domain/graphs.rs
- native/hagency-store/src/lib.rs
- native/hagency-store/tests/retention_execution.rs
- native/hagency/src/bootstrap.rs
- specs/task-rust-retention-execution.spec.md
- knowledge/decisions/adr-053-native-owned-dispatch.md
- knowledge/decisions/adr-031-native-task-graph-custody.md
- knowledge/decisions/adr-125-admitted-corpus-retention.md
- docs/progress.md

## Acceptance Criteria

Scenario: Settled dispatch evidence is pruned inside its window
  Given a settled dispatch older than 500 dispatches with outputs attempts and receipts
  When the execution phase runs
  Then its output and receipt-family rows are gone and a receipt records the prune
  And the newest accepted output row per dispatch and fence survives

Scenario: A held completion pins its whole dispatch
  Given a settled dispatch whose completion state is held
  When the execution phase runs
  Then that dispatch and every receipt it carries remain

Scenario: An unresolved dispatch is never a candidate
  Given a dispatch in outcome_unknown
  When the execution phase runs
  Then its attempt output and receipt rows remain
  And after a recovery copies the linkage the original still remains

Scenario: The attempt row is never pruned and the late path still authenticates
  Given a settled dispatch with an attempt row
  When the execution phase runs
  Then the attempt row remains and record_late_output still authenticates against it

Scenario: The receipt is bounded and the phase logs its cost
  Given more than 100 execution receipts
  When the phase writes one more
  Then the receipt table holds at most 100 rows
  And the row carries pruned remaining elapsed_ms and at_ms

## Out of Scope

**Owed test selectors (parked, not bound).** Every selector below is owed: none
is a `#[test] fn` on the integration base `review/retention-1`, so binding it as a
`Test:` line would bind a name that does not exist and the spec-binding gate would
fail. Each scenario above therefore stands **unbound** — its name, its given, its
when and its then are kept — and the selector it will carry once its slice's code
lands on `288a9c5c` is parked here. Integration moves a line back to a
`Test:` under its scenario when that selector exists.

- owed `native_execution_prune_keeps_the_window_and_writes_a_receipt` — scenario "Settled dispatch evidence is pruned inside its window"
- owed `native_execution_prune_retains_the_held_completion_evidence` — scenario "A held completion pins its whole dispatch"
- owed `native_execution_prune_retains_unsettled_and_unknown_fate_dispatches` — scenario "An unresolved dispatch is never a candidate"
- owed `native_execution_prune_leaves_the_attempt_anchor` — scenario "The attempt row is never pruned and the late path still authenticates"
- owed `native_execution_prune_receipt_is_bounded_and_logs_its_cost` — scenario "The receipt is bounded and the phase logs its cost"
