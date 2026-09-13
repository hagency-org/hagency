---
spec: task
name: "Admitted-message corpus retention bound"
inherits: project
satisfies: [REQ-RUST-MIGRATION-PLAN, ADR-125]
tags: [active, retention, store, rust]
---

## Intent

Bound the admitted-message corpus (the fastest-growing unbounded surface in
the native store, inventory gap G1) with a pinned, archived sweep that
reclaims instead of refusing, and keep the retained product's "archive, do
not lose" property inside a named window.

## Constraints

### Must
- One installation-wide ceiling, default 5000, floor 100 (`Math.max(100, …)`,
  the retained env guard), configured as a constant with a `Bootstrap`
  override — never an environment variable, because no native installation
  setting is.
- A row is a prune candidate only when every pin clause is false: P1 recency,
  P2 unprocessed session input, P3' claimed-but-unprocessed, P4 live dispatch,
  P5 unknown fate, P6'/P7' open canonical task (status `done` is the release —
  never `task_intents.state='closed'`, which has no production writer), P9/P10
  attachment custody.
- Provenance moves with the message: copied into `retained_message_archive`
  (keyed `UNIQUE(engagement_id, source_key)`, every read engagement-scoped)
  and deleted in the same transaction.
- The four ingress reads that outlive a message answer live first and from
  the archive on a live miss; both callers reconstruct `wake`/`config` from
  the archive row, so an exact redelivery of a pruned admission still dedupes
  and a divergent one is still refused.
- The sweep runs on the domain worker as the `messages` phase of ONE
  retention tick (`RETENTION_SWEEP_PERIOD` 60 s, `RETENTION_PHASE_BUDGET_MS`
  600, `elapsed_ms` logged every tick, one `Immediate` transaction per phase,
  skipped-or-committed never torn, `retention_prune_receipts` with
  `phase='messages'` trimmed to 100).
- Unknown fate is retained indefinitely — a named product decision; the pin
  is the dispatch state pair P4/P5 read from `runner_dispatches.state`
  directly, never the `unresolved_dispatches` view (that view is reporting).
- The archive is bounded to the same ceiling and pruned oldest-first in the
  same tick — a named product decision (content older than about two
  ceilings is gone).
- `retention_status()` is the one read; no console or CLI page work.

### Must Not
- Never refuse new admission to protect the bound; never fail the sweep
  transaction to "protect" a row (a pinned row is simply not a candidate).
- Never pin on `task_intents.state='closed'` or on a column with no release
  witness (`session_inputs.dispatch_id` is never cleared after processing).
- Never touch the pin-rule table schemas, the retained prune behaviour, the
  close path, the busy timeout, WAL mode or foreign keys.

## Boundaries

### Allowed Changes
- `native/hagency-store/src/migrations/026-corpus-retention.sql`
- `native/hagency-store/src/domain.rs`, `domain/messages.rs`,
  `domain/verified_ingress.rs`, `domain_worker.rs`, `lib.rs`
- `native/hagency/src/bootstrap.rs`
- `native/hagency-store/tests/retention.rs`,
  `native/hagency-store/tests/fixtures/corpus-retention-vectors.json`
- `native/scripts/corpus-retention-vectors.mjs`, `backend-v2.js`
  (`__backendV2TestInternals` export only), the fourteen schema-head
  assertions, `specs/retention.spec.md`, `knowledge/decisions/adr-125-*`,
  `docs/progress.md`

### Forbidden
- The pin-rule tables' schemas (004, 005, 012, 018 — indexes only, in 026)
- The peer corpus (007, 010) or any shared view
- `archivePrunedMessages`/`planMessagePrune` behaviour in `backend-v2.js`
- The close path, the busy timeout, WAL mode, `foreign_keys`

## Acceptance Criteria

Scenario: The admitted corpus keeps its bound without a longer timeout or a silent refusal
  Test: native_retained_corpus_prunes_below_ceiling_only_when_no_live_reference
  Given an admitted corpus over the ceiling, the oldest row processed and unreferenced
  When the corpus sweep runs
  Then only rows above the ceiling that no pin clause holds are pruned
  And no row with an unprocessed session input, a live or unknown-fate dispatch, an open
  task root or input, or an attachment projection is touched
  And the receipt row records the messages phase, the batch and the over-ceiling figure

  Test: native_retained_corpus_pending_pin_exceeds_ceiling
  Given every row is pinned by an unprocessed session input
  When the corpus sweep runs
  Then the corpus stays over the ceiling
  And the sweep reports the over-ceiling count instead of refusing admission

  Test: native_retained_corpus_processed_dispatch_does_not_pin
  Given a message whose dispatch completed and whose session input carries
  processed_at and a stale non-null dispatch_id
  When the corpus sweep runs
  Then the message is a prune candidate (P3' does not pin it)
  And its session input and the admitted row are removed together

  Test: native_retained_corpus_closed_task_input_does_not_pin
  Given a message attached to a canonical task whose config status is done
  When the corpus sweep runs
  Then the message is a prune candidate (P7' released by the terminal state)
  And the mirror holds: an open task's root and input pin the message

  Test: native_retained_corpus_unknown_fate_is_retained
  Given a message whose dispatch outcome is unknown and whose session input is processed
  When the corpus sweep runs
  Then the message survives indefinitely, reported in the over-ceiling figure
  And the pin is read from runner_dispatches.state, never the unresolved view

  Test: native_retained_corpus_provenance_moves_with_the_message
  Given a pruned verified-ingress message
  When the archive row is inspected and an exact redelivery arrives
  Then the row carries engagement_id, source_key, scope_digest, source_session_id and wake
  And the live provenance row is gone from matrix_ingress_events
  And the redelivery is recognised as admitted with the same sequence and wake
  And a divergent redelivery under the same event id is refused with Conflict

  Test: native_retained_corpus_archive_is_bounded
  Given the corpus sweep has pruned rows past the ceiling
  When further sweeps run
  Then the pruned content is present in retained_message_archive
  And retained_message_archive is itself bounded and pruned oldest-first in the same tick

  Test: native_retained_corpus_parity_with_javascript
  Given the retained planMessagePrune and the native prune see the same row sequence
  When each partitions it
  Then the retained and pruned sets are identical on the shared recency/inbox subset
  And the retained archivedMessageExists and the native archive membership read agree
  on whether a message is already durably recorded
  And the native-only clauses (P2, P3', P6', P7') are pinned by the store tests, not this vector

  Test: native_retained_corpus_floor_is_hundred
  Given a ceiling below the floor is configured
  When the store clamps it
  Then the effective ceiling is 100

  Test: native_retained_corpus_schema_upgrade
  Given a live database rewound to the previous schema head over populated rows
  When the store reopens
  Then the migration replays idempotently (CREATE ... IF NOT EXISTS only), creates the
  archive, the receipt table and the seven pin-probe indexes, and drains nothing
  And the sweep entry point drains through the batch with every pinned class surviving
