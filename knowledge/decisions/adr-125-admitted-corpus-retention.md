---
kind: decision
id: ADR-125
title: Installed-corpus retention for admitted messages
status: Proposed
---

## Context

Matrix intake is unbounded on the native side. `admitted_messages` grows one
row per accepted inbound event and nothing ever deletes one: the only global
bound is `bounded_row(admitted_messages, 100_000)`, which refuses new work
rather than reclaiming. It is the fastest-growing surface in the store
(inventory gap G1) and the only one with neither a cap nor a delete.
`matrix_ingress_events` grows beside it, one provenance row per verified
admission, with its own 100 000 stop.

The retained product bounded the analogous `messages` array long ago:
`MESSAGE_RETENTION_LIMIT = max(100, env.AGENT_MESSAGE_RETENTION_LIMIT || 5000)`
(`backend-v2.js:236`), enforced by `planMessagePrune` from
`pruneMessagesInMemory` and `saveMessages`; unread or router-uncopied rows are
exempt, pruned rows are appended to `messages-archive.jsonl`, and the prune
aborts if that archive fails. That archive is itself read back —
`archivedMessageExists` gates re-persistence in `completeMatrixDispatch` — so
the archive is a durable dedupe record, not a write-only log. This ADR ports
that bound; it is slice 1 of the retention plan and closes G1.

## Decision

**Ceiling.** One installation-wide ceiling on the admitted corpus, default
5000, floor 100, named `MESSAGE_RETENTION_CEILING` / `MESSAGE_RETENTION_FLOOR`,
configured as a constant with a `Bootstrap` override — not an environment
variable, because no native installation setting is.

**Pin rule.** A row is a prune candidate only if no retained-reference clause
holds. Pinned: the newest CEILING rows by sequence; any `session_inputs` row
for it with `processed_at IS NULL` (P2) or claimed and not yet processed,
`dispatch_id IS NOT NULL AND processed_at IS NULL` (P3′); any
`dispatch_inputs` row whose dispatch is queued, leased, started, parked (P4)
or `outcome_unknown` (P5); any open task's `task_intents.root_sequence` or
`task_inputs.message_sequence` — a task is open until
`json_extract(canonical_tasks.config,'$.status')='done'`, which is its own
terminal state, written by `finish_task_clock` and irreversible (P6′/P7′); any
`matrix_attachments.message_sequence` or
`session_attachment_visibility.message_sequence`, bounded by the attachment
caps (P9/P10).

**Task lifecycle is the canonical task's, not the intent's — a named product
decision.** The gate is `json_extract(canonical_tasks.config,'$.status')
='done'`; the `task_intents.state` column's `closed` value has no production
writer and is not used as a release. A task that never completes stays pinned
(bounded by task count), and its share is to be measured.

**Provenance moves with the message — a named product decision.** The
`matrix_ingress_events` row is not a pin. On prune it is copied into
`retained_message_archive` with the message, and deleted in the same
transaction. The archive carries `engagement_id` and `source_key`, keys on the
pair, and every archive read is engagement-scoped. The four reads that
outlive a message are answered live first and from the archive on a live
miss, and the two whose callers need the `session_inputs` child reconstruct
`wake`/`config` from the archive row and skip the re-insert, so an exact
redelivery still dedupes and a divergent redelivery is still refused with the
same word. As a consequence the `matrix_ingress_events` 100 000 stop stops
accumulating; it remains a backstop reachable only when the corpus cannot
drain at all.

**Nowhere to hide.** `session_inputs.dispatch_id` is never cleared after
processing and `task_inputs` rows are never deleted; a pin on either column
alone would pin the whole corpus forever. P3′ and P7′ are therefore bounded
by the `processed_at` witness and the canonical task's terminal state.

**Unknown fate is retained — a named product decision.** An admission whose
dispatch is in an unknown outcome (`runner_dispatches.state =
'outcome_unknown'`) is retained indefinitely, and stays retained through a
dispatch recovery — the pinning pair is P4 AND P5 read from
`runner_dispatches.state` directly, never the `unresolved_dispatches` view
(that view is for reporting). Its evidence is the only record that the work
might have run. Nothing carrying custody, settlement or audit meaning is ever
a prune candidate; that is a consequence of the pin rule, and a test asserts
it.

**Where it runs.** A periodic sweep on the domain worker, not inside the
admission transaction — the `messages` phase of ONE retention tick
(`RETENTION_SWEEP_PERIOD` 60 s) beside the ceiling-alert sweep, one
`Immediate` transaction per phase, batch 512 initially, per-phase budget 600
ms with `elapsed_ms` logged every tick and the measured-over-budget batch
reduction rule. On `Busy`/`OutcomeUnknown` the phase logs and waits for the
next tick. A retention failure is never a work refusal.

**The receipt.** One shared table `retention_prune_receipts`, one row per
phase per tick written inside that phase's own `Immediate` transaction —
a receipt exists iff the phase committed (`phase`, `pruned`,
`oldest_ref`/`newest_ref`, `remaining`, `elapsed_ms`, `at_ms`), trimmed to
100 rows by the same writer; a zero-work tick writes nothing. The receipt's
`elapsed_ms` is sampled immediately before the commit and therefore
EXCLUDES the commit's own cost and any lock wait the commit pays; the
sweep outcome's sample is taken after the commit, and that post-commit
number — commit and lock-wait included — is what the measured-over-budget
batch-reduction rule consumes.

**The archive window — a named product decision.** The retained product's
`messages-archive.jsonl` is unbounded; native bounds its archive to the same
ceiling, so content older than about two ceilings is not retained, and the
archive's own prune runs in the same sweep tick.

**Parity, honestly bounded.** The vector oracle covers the shared subset
only (recency vs inbox membership, and archive membership); the retained
predicate has no counterpart for P2..P10, so native-only pairs are pinned by
the store tests.

**The test set is larger than the backlog's four names.** The retention
backlog names four selectors; the landed set carries thirteen (round-3
review: the count is corrected and the set named) —
`native_retained_corpus_prunes_below_ceiling_only_when_no_live_reference`,
`native_retained_corpus_pending_pin_exceeds_ceiling`,
`native_retained_corpus_processed_dispatch_does_not_pin` and
`native_retained_corpus_closed_task_input_does_not_pin` (the P3' and P6'/
P7' release arms), `native_retained_corpus_unknown_fate_is_retained`,
`native_retained_corpus_provenance_moves_with_the_message`,
`native_retained_corpus_attachment_projection_pins_the_message` (P9/P10
attachment custody),
`native_retained_corpus_threaded_root_resolves_from_archive_by_scope_digest`
and `native_retained_corpus_threaded_root_refuses_on_scope_digest_mismatch`
(read 6's archive fallback, both verdicts), and
`native_retained_corpus_archive_is_bounded`,
`native_retained_corpus_parity_with_javascript`,
`native_retained_corpus_floor_is_hundred` and
`native_retained_corpus_schema_upgrade`. These are the design's own
release-arm and custody tests and are part of the set, not additions
beyond it.

## Consequences

Good, because the fastest-growing surface in the store gains a bound that
reclaims rather than refuses; because the bound matches the retained number
(5000/100); because the three defeating pins (provenance, claimed input,
task input) are stated as product decisions with named witnesses instead of
left to an implementer; and because nothing is destroyed silently — the
archive preserves the message and its provenance inside the window, and still
answers the retained membership/dedupe read.

Bad, because a second bounded surface (`retained_message_archive`) now exists
and must be swept in the same tick; because the console must read a new
status method; because the fourteen schema-head assertions move in the same
commit, widening the diff; and because atomic parity is not claimed: native
pins rows the retained product would prune (it has no dispatch, task,
attachment or provenance notion), so native cannot match retained parity on
corpus size, only on the shared subset.

**Open measurement, stated as one.** The 512 batch is a hypothesis until a
tick is timed against the 2 s reply budget; `elapsed_ms` is logged every tick
so the number ships with the measurement or the batch is reduced.

## Alternatives Considered

**Prune inside the admission `Immediate` transaction.** Single-writer-correct,
but the first admission of an upgraded install with 10⁵ rows would pay the
whole catch-up in one transaction, stalling the worker. Rejected.

**Archive to a `messages-archive.jsonl` file under the state directory
(retained shape).** Rejected: the retained archive is unbounded (G7); native
would re-create a known gap plus torn-tail repair and fsync machinery, to
store what a row already stores.

**Delete with a bounded receipt row and no archive.** Cheapest and bounded,
but it destroys content native alone holds (`admitted_messages.config` is the
only copy of the message) and cannot answer the retained membership read.
Rejected as primary; named so the rejection of "archive, do not lose" is
explicit.

**Pin the provenance row.** Rejected: every verified-ingress admission writes
a provenance row, so the pin would defeat the slice — only non-Matrix
admissions would ever drain. Replaced by "moves with the message".

**Gate the task pin on `task_intents.state='closed'`.** Rejected per the v3
review: no production writer sets `closed`, so the release never fires and
the pin is forever. Replaced by the canonical task's terminal `done` state,
which has a production writer and is irreversible.

**A hard cap and refusal at the ceiling (the 100 000 shape).** Rejected: it
stops ingesting rather than reclaiming, and the corpus is not sorted by
importance.
