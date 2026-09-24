spec: task
name: "Bound the peer corpus with pins an identity archive and a phase-two tick"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, retention, peer-corpus, store]
---

## Intent

Bind RT-7 (ADR-125's amendment): the retention tick's **second phase**,
`peer`, bounds the three-table peer corpus (`peer_messages` and its
`peer_session_inputs`/`peer_dispatch_inputs` children) — a row is a candidate
only when every pin clause is false, pruning deletes the row with its
children and the paired graph move while recording **identity** (not
content) in `retained_peer_index`, and the phase writes its own receipt
inside its own single `Immediate` transaction with its own per-phase batch
slot. Mirrors the messages-phase set scenario-for-scenario.

## Constraints

### Must
- Implement the pin rule exactly: P1 recency outside the newest CEILING; P2′ an input is pinned only while `processed_at IS NULL` **and** its pair is a member of `conversation_peer_inputs` — the view that additionally requires the conversation `state='active'`, the engagement `state='active'`, and matching generation/binding — so a closed conversation **releases** its unread (P2′ = unprocessed + still-live-and-bound, never "unprocessed alone"); P3′ claimed-not-processed (`dispatch_id` set, `processed_at` null); P4 any live dispatch; P5 any `outcome_unknown` dispatch (the raw state pins — `unresolved_dispatches` never releases); P6 a graph node inside its release boundary. `wake` never pins.
- Freeze the bound's numbers (the design's four, unmeasured hypotheses but the checked constants): `PEER_RETENTION_CEILING = 5000`, `PEER_RETENTION_FLOOR = 100` (the ceiling clamps up to it, never down), `PEER_RECEIPT_CEILING = 10_000` (the identity store's DDL-level bound), and the peer phase's initial batch **512**.
- Delete children-first in one transaction per tick: the paired graph move (when releasable), then `peer_dispatch_inputs`, then `peer_session_inputs`, then `peer_messages`, then the identity insert — a parent is deleted only with all of its children.
- Create `retained_peer_index(source_key PK, digest, sequence, pruned_at_ms)` in migration 027 (`IF NOT EXISTS`), pruned oldest-first in the same tick, bounded by the DDL-level `PEER_RECEIPT_CEILING`.
- Consult the identity store on `admit`'s live-miss idempotency lookup (`peers.rs:166-182`), so a redelivered send keeps its `Conflict`/`replayed` answer after the prune.
- Write the `phase='peer'` receipt row inside the phase's own transaction, carrying the shared table's **seven real columns** — `pruned`, `oldest_ref`, `newest_ref`, `remaining`, `elapsed_ms` (plus `sequence`/`at_ms` auto) — trimmed to the shared limit by the same writer; there is **no `archived` column** and this slice adds none (phase 1's receipt table is not altered, and no migration but 027 is licensed).
- Give the phase its own batch slot in the per-phase state — its own hypothesis, halved on its own breach, floor 1, the split at batch 1 a named visible residue — never the messages scalar.
- Assert no-consumer-observes over the consumer queries (`peers.rs:115/:277`, `execution.rs:743`, `task_intents.rs:366`, `graphs.rs:356`) with a non-empty precondition.

### Must Not
- Do not modify the corpus schemas (`007`, `010` — no column, no view body; the new migration adds indexes only) or any shared view body.
- Do not archive content — the identity store carries `sequence,digest`, never config or message bytes.
- Do not re-create phase 1's `retention_prune_receipts` in migration 027: the receipt table is 026's object (created `IF NOT EXISTS` there), and 027 only adds `retained_peer_index` plus the pin-probe indexes — re-creating the receipt table would silently collide with phase 1's DDL and is explicitly forbidden.
- Do not release `outcome_unknown` rows via `unresolved_dispatches` (D-1: reporting never gates retention), and do not pin on `wake`.
- Do not touch the `admitted_messages` corpus and its children (phase 1's objects), `ceiling_alerts.rs`, the close path, `busy_timeout`/WAL/`foreign_keys`, or the messages phase's own selectors.
- Do not share batch state with the messages phase; do not write the receipt outside the phase transaction.
- Do not gate any scenario by OS or feature in the binding set.

## Boundaries

### Allowed Changes
- native/hagency-store/src/migrations/027-peer-corpus-retention.sql
- native/hagency-store/src/domain.rs
- native/hagency-store/src/domain/peers.rs
- native/hagency-store/src/domain/graphs.rs
- native/hagency-store/src/domain/messages.rs
- native/hagency-store/src/domain_worker.rs
- native/hagency-store/src/lib.rs
- native/hagency/src/bootstrap.rs
- native/hagency/src/lib.rs
- native/hagency-store/tests/peer_retention.rs
- native/hagency-store/tests/
- specs/task-rust-peer-corpus-retention.spec.md
- knowledge/decisions/adr-125-admitted-corpus-retention.md

### Forbidden
- Live services, credentials, deployed state.
- native/hagency-store/src/domain/ceiling_alerts.rs; migrations other than 027 (026 phase 1's, 028 MA-S1's, 029 MA-S4's, 031 MA-S2's, 032 PC-C1's); the schema-head pin move **to 27** is licensed here and nowhere else.

## Acceptance Criteria

Scenario: The peer corpus prunes below the ceiling only when no pin holds
  Test: native_retained_peer_corpus_prunes_below_ceiling_only_when_no_live_reference
  Level: integration
  Test Double: real store rows via the peers.rs setup/send/claim/dispatch helpers; no live service
  Given a corpus over the ceiling whose oldest rows carry no pin
  When the peer phase sweeps
  Then each pruned sequence leaves zero peer_session_inputs and zero peer_dispatch_inputs rows
  And the paired graph binding is moved in the same transaction
  And remaining reflects the distance to the ceiling

Scenario: A pending pin exceeds the ceiling without error
  Test: native_retained_peer_corpus_pending_pin_exceeds_ceiling
  Level: integration
  Test Double: the same fixture with every row pinned
  Given a corpus whose every row is pinned past the ceiling
  When the peer phase sweeps
  Then no Capacity error is raised and remaining is reported honestly
  And no pinned row is pruned

Scenario: A processed dispatch input does not pin
  Test: native_retained_peer_corpus_processed_dispatch_does_not_pin
  Level: integration
  Test Double: one row with processed_at set and its dispatch completed, one with processed_at null
  Given the paired rows
  When the peer phase sweeps
  Then the processed row is pruned and the unprocessed row is retained

Scenario: A closed conversation releases its unread input
  Test: native_retained_peer_corpus_closed_conversation_releases_its_unread
  Level: integration
  Test Double: one unread input under a closed conversation, the same under an active one
  Given the paired rows
  When the peer phase sweeps
  Then the closed conversation's row is pruned and the active conversation's row is pinned by P2′

Scenario: An unknown-fate dispatch retains its input
  Test: native_retained_peer_corpus_unknown_fate_is_retained
  Level: integration
  Test Double: an input claimed by an outcome_unknown dispatch with the reporting view showing it resolved
  Given the row and a dispatch_stops row settled and a dispatch_recoveries row present
  When the peer phase sweeps
  Then the row is still retained — P5 pins on the raw state and unresolved_dispatches never releases

Scenario: A redelivered send answers from the identity store after the prune
  Test: native_retained_peer_corpus_replay_answers_from_identity_store
  Level: integration
  Test Double: a pruned source_key redelivered with the same and a different digest
  Given a corpus row pruned into retained_peer_index
  When admit is called for the same source_key with each digest
  Then the matching digest answers replayed true and the differing digest refuses with Conflict
  And the recipient's inbox count is unchanged across both attempts

Scenario: The schema head advances to 27 and replays after rewind
  Test: native_retained_peer_corpus_migration_replays_after_rewind
  Level: integration
  Test Double: a live store rewound below 027 and reopened twice
  Given a populated store rewound beneath the new head
  When the repository reopens
  Then the head is 27, the identity table exists, and the second open replays nothing
  And every rewind fixture still lands at the head — **14 `pragma_update` rewind sites across 12 files**, each asserting the head is 27, and **only `peer_retention.rs` asserts `retained_peer_index` exists** (the other files assert the head, not the identity table)

Scenario: The phase receipt is written inside the phase transaction
  Test: native_retained_peer_corpus_receipt_row_is_written_inside_the_transaction
  Level: integration
  Test Double: a pruning tick observed before and after commit
  Given a peer phase tick that prunes
  When the transaction commits
  Then a phase=peer row exists in retention_prune_receipts carrying pruned, oldest_ref, newest_ref, remaining and elapsed_ms — and no archived column exists to assert
  And the table is trimmed to the shared limit by the same writer

Scenario: An inactive engagement releases its unread input
  Test: native_retained_peer_corpus_inactive_engagement_releases_its_unread
  Level: integration
  Test Double: one unread input under a revoked engagement, the same under an active one
  Given the paired rows
  When the peer phase sweeps
  Then the revoked engagement's row is pruned and the active engagement's row is pinned by P2′

Scenario: The graph binding moves with the message
  Test: native_retained_peer_corpus_graph_binding_moves_with_the_message
  Level: integration
  Test Double: a terminal node bound to a candidate message, driven through the real workflow path
  Given the terminal node and the candidate
  When the peer phase sweeps
  Then the binding moves as a paired write (column and persisted config) and a workflow read after the prune returns Ok

Scenario: A live graph binding is retained
  Test: native_retained_peer_corpus_live_graph_binding_is_retained
  Level: integration
  Test Double: a dispatched node bound to a past-window message, the same node flipped terminal
  Given the live binding
  When the peer phase sweeps
  Then the row survives (P6)
  And with the node flipped terminal the same setup is pruned

Scenario: No consumer observes the prune
  Test: native_retained_peer_corpus_shared_views_no_consumer_observes_the_prune
  Level: integration
  Test Double: a non-empty pre-sweep consumer result set over the peer corpus views
  Given the pre-sweep set
  When the peer phase prunes a candidate
  Then every consumer query that saw the pair before still answers without error and with no wrong verdict

Scenario: A queued dispatch input is not dropped
  Test: native_retained_peer_corpus_queued_dispatch_input_is_not_dropped
  Level: integration
  Test Double: a candidate with one dispatch still queued, the same flipped completed
  Given the queued claim
  When the peer phase sweeps
  Then the row is pinned (P4, the claim flip must not happen)
  And with the dispatch flipped completed the row is pruned

Scenario: The identity store is bounded by its own ceiling
  Test: native_retained_peer_corpus_identity_store_is_bounded
  Level: integration
  Test Double: more than PEER_RECEIPT_CEILING pruned keys seeded over time
  Given the over-ceiling identity rows
  When the peer phase runs
  Then retained_peer_index is bounded oldest-first at 10_000, not at the corpus ceiling

Scenario: The floor clamps every configured peer ceiling
  Test: native_retained_peer_corpus_floor_is_hundred
  Level: integration
  Test Double: a corpus above the floor under a below-floor ceiling, and under a 1000 ceiling
  Given the two ceilings
  When the store clamps them
  Then the below-floor ceiling clamps to 100 (and 1000 is not clamped)

Scenario: The schema head is current everywhere
  Test: native_retained_peer_corpus_migration_head_is_current
  Level: integration
  Test Double: a fresh database and a deep rewind to 24 reopened twice
  Given the deep rewind
  When the store reopens
  Then every reopen lands at head 27 (025, 026 and 027 all replay in order)

## Decisions

**Depends on Slice 1 landing first.** The `peer` phase appends to the one
retention loop phase 1 created, writes into the receipt table 026 created,
and follows the per-phase batch state the tick contract fixed — none of it
exists until the messages phase is in. Migration **027** is RT-7's per the
ledger; the schema-head pin move to 27 is licensed in this spec and nowhere
else, and lands in this slice's own commit with every rewind preserved.

**The retained JavaScript is byte-frozen.** The oracle for this phase is the
SQL predicate plus the fixtures (phase 1 needed a test-only export because
its keep-set lived in JS module globals; here the predicate is SQL inside
the sweep, so **no export exists or is needed**), and any retained-side
mirroring cites the frozen files rather than editing them.

**Naming.** The selectors carry the `native_retained_peer_corpus_` prefix
(the design's own `peer_corpus_` names under the lane's
`native_retained_…` family). The set is **sixteen selectors against the
messages phase's fourteen** — not a one-for-one mirror: the peer phase adds
the identity-store replay, the engagement-release arm, the graph-move pair
and the head-pin-current test the messages phase has no counterpart for.

## Out of Scope

The messages phase's own selectors (phase 1's), the execution and
engagements phases (later slices), any D-1 settlement surface for
`outcome_unknown` pins (a named class, not wired here), and the retained
Node side (byte-frozen; mirrored by citation only).
