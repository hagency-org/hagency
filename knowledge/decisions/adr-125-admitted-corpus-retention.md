---
kind: decision
id: ADR-125
title: Installed-corpus retention: admitted messages (phase 1) and peer messages (phase 2)
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
backlog names four selectors; the landed set carries fourteen (the r4
review's count: the spec carries fourteen `Test:` lines, the test file
fourteen `native_retained_corpus_*` fns) —
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
(read 6's archive fallback, both verdicts),
`native_retained_corpus_archive_rearchive_is_keyed_not_fatal` (S4's keyed
re-archive and its NULL-engagement exception), and
`native_retained_corpus_archive_is_bounded`,
`native_retained_corpus_parity_with_javascript`,
`native_retained_corpus_floor_is_hundred` and
`native_retained_corpus_schema_upgrade`. These are the design's own
release-arm and custody tests and are part of the set, not additions
beyond it.

## Decision — phase 2: the peer corpus (Slice 7)

**Ceiling.** One installation-wide ceiling on `peer_messages`, default 5000,
floor 100 (`PEER_RETENTION_CEILING` / `PEER_RETENTION_FLOOR`), a constant with
a `Bootstrap` override. Native chooses its own number: the retained product
has **no peer lane** — peer sends are projected into the router store's
`router_messages` and never enter the retained `messages` array, so
`MESSAGE_RETENTION_LIMIT` and `planMessagePrune` never see one, and nothing
ever archives one. That is a finding, not an omission, and it is why there is
no retained "archive, do not lose" property to preserve for this corpus.

**Pin rule (peer).** A `peer_messages` row is a prune candidate only when no
clause holds. Pinned: the newest CEILING rows by `sequence` (P1); any
`peer_session_inputs` row that is unread (`processed_at IS NULL`) **and** a
member of `conversation_peer_inputs` — the view that additionally requires
the conversation `state='active'`, the engagement `state='active'`, and
matching generation/binding (P2′); any such row claimed and not yet processed
(`dispatch_id` set, `processed_at` null — P3′); any `peer_dispatch_inputs`
row whose dispatch is live, `queued/leased/started/parked` (P4), or
`outcome_unknown` (P5) — the pin is the raw `runner_dispatches.state` pair,
never the `unresolved_dispatches` view (the tick contract's D-1); any live
graph binding (P6, below). `wake` never pins: it is set once and never
cleared, so it would pin every Request/Response forever.

**Delete, keep identity — never content.** A pruned row is deleted with its
children in the same transaction — children first, the paired graph move
first of all — and its identity is recorded in
`retained_peer_index(source_key PK, digest, sequence, pruned_at_ms)`, so a
re-presented key still answers `replayed`/`Conflict` exactly as the live
lookup did. **Bodies are not retained**: the only post-prune reader —
`admit`'s idempotency lookup (`peers.rs:177`, the identity-store consult at
`:228`) — needs identity, not
content. The identity store's own bound is `PEER_RECEIPT_CEILING = 10_000`,
pruned oldest-first by the same tick — a code fact with a real statement,
not a prose claim. **A pinned row is simply not a candidate, never a failed
transaction**: the phase never protects a row by failing its transaction.

**Graph custody.** A graph binding releases only when the node is terminal
(`complete`, `failed`, `skipped`, `cancelled`), and the release is a **paired
move** — the column and the persisted `WorkflowNode.message_sequence` config
together, through the store's own save path — because `read` equality-checks
the two; a column-only NULL would poison every later read with
`Error::State`. The move is a release of custody, not an authorization:
`admit_recovery` (`graphs.rs:394`) accepts a `complete` node *as well as*
live states, so a moved binding refusing there with `RunnerAuthority` is the
**intended direction** — the message is gone, and the recovery must be
re-driven from the identity store.

**The tick, receipt and prefix.** The `peer` phase is phase 2 of the ONE
retention tick (`messages → peer → …`), one `Immediate` transaction, its own
per-phase batch hypothesis (initially 512, halved on over-budget, floor 1),
the receipt row `phase='peer'` written inside the phase's own transaction
carrying the shared table's **seven real columns** — `pruned`, `oldest_ref`,
`newest_ref`, `remaining`, `elapsed_ms` (plus `sequence`/`at_ms`) — trimmed
to the shared 100-row limit; **there is no `archived` column** and this
slice adds none. On `Busy`/`OutcomeUnknown` the phase logs with the
`[retention] peer phase` prefix (the code's own wording at
`bootstrap.rs:636-659`, not a bare `[peer]` prefix) and waits for the next
tick, never an in-line retry.

**Ownership of the shared tables.** This phase is the **pin owner** of
`peer_messages`, `peer_session_inputs`, `peer_dispatch_inputs` and
`retained_peer_index` — it states the release proof and owns the bound.
**Cascade delete rights on the three corpus tables belong to Slice 6's
engagement cascade**, not the execution phase: `engagements` is the only
phase whose object is a root, and its reachable-set cascade removes these
leaf rows when their parent engagement is deleted, exactly as the ownership
table above records for the Slice 1 tables.

**Unknown fate, named not wired.** The `outcome_unknown` pin is the tick
contract's D-1 class, inherited and named here, not created by this slice.
The engagement-deactivation release arm of P2′ is the same class as the
recovery release: its `end()` path is reached only through worker wrappers
nothing outside tests calls, so it is never a live release. **Acceptance
line:** if a reopen path, an engagement reactivation, or a participant
re-insert appears on the base, P2′ must be revisited.

**The refusal backstop.** With the graph release and the bound in place, the
`bounded_row(peer_messages, …, 100_000)` refusal (`peers.rs:246`) becomes
reachable only when the corpus cannot drain at all — the all-pinned
condition, one property — so it remains a backstop, never a reclaiming
mechanism.

## Retention sweep tick

Retention in native is **one periodic sweep on the domain worker**, not a
per-surface timer. This section is the contract every retention slice cites; it
fixes the tick's phases, its budget, its receipt and the ownership of the tables
more than one slice touches. Slices 1, 2, 6 and 7 add a phase; Slice 3 refuses one
and trims in-write; Slice 4 refuses one and rotates files off the service.

**The writer is serial.** The store has one serial writer thread,
`hagency-domain`, consuming `Job::Run` synchronously (`domain_worker.rs:2311-2322`).
Every mutation is such a closure. A tick is therefore a sequence of `Job::Run`
submissions that **occupy the same thread** every foreground command needs, and
every caller carries a 2 s reply bound (`domain_worker.rs:2393`, expiring as
`Error::OutcomeUnknown`). The tick's shape is a budget question, not a scheduling
one.

### The phases — four, and two refusals

A **phase** is a unit of work that is a candidate for the tick; a **refusal** is a
slice that states its prune does not belong in the tick. Both are stated, so
"Slice 3 adds no phase" reads as a decision rather than an omission.

| Order | Phase name | Slice | Work | Own transaction? |
|---|---|---|---|---|
| **1** | `messages` | Slice 1 | archive + delete admitted-message rows and their per-session children | yes — one `Immediate` tx |
| **2** | `peer` | Slice 7 | delete peer-message rows, their two children, and the releasable `graph_nodes` move | yes |
| **3** | `execution` | Slice 2 (ADR-053/031) | delete settled dispatches' output and receipt-family evidence | yes |
| **4** | `engagements` | Slice 6 (ADR-095) | the whole reachable-set cascade, child-first, to a fixed point | yes — one `Immediate` tx |
| — | *(refused as a phase — in-write)* | Slice 3 (ADR-095) | the decision trim runs inside `record_decision`'s own transaction | n/a |
| — | *(refused as a phase — off-tick)* | Slice 4 | file/log rotation in the maintenance binary | n/a |

**The order is fixed: `messages → peer → execution → engagements`.** Children
before parents *across* slices, not only within one: `engagements` is the only phase
whose object is a *root*, and phases 1–3 delete leaves and mids that hang off those
roots, so running the leaves first makes the cascade strictly shorter and never
longer. The fastest-growing corpus goes first so a tick that dies after two phases
has still reclaimed the fastest surface.

**The dependency invariant, stated once.** *Within one tick, phase k may depend only
on the completed work of phases < k. No phase's correctness may presuppose a later
phase's deletes in the same tick. A candidate whose child is pruned by a later phase
waits for the next tick.* "Candidate in the same tick" therefore means a child *this
phase's own transaction* clears — never a child another phase might clear.

**A tick is one period, N submissions, never one transaction.** The tick submits one
`Job::Run` per phase, sequentially awaited, shaped on `start_ceiling_sweep`
(`bootstrap.rs:436-475`). Between submissions the writer's FIFO channel drains any
foreground caller that arrived in between, so no caller waits for more than one
phase plus its own queue position. Each phase owns one `Immediate` transaction (the
`ceiling_alerts.rs:112-114` shape); a phase that fails mid-way rolls back whole, and
a tick is skipped or committed, never torn. On `Busy` or `OutcomeUnknown` a phase
logs its refusal with its slice prefix (`[corpus]`, `[retention] peer`,
`[retention]`, `[engagement]`, `[decision]`) and waits for the next tick — never an in-line retry.

### Budget

> **`RETENTION_SWEEP_PERIOD = Duration::from_secs(60)`**, one period for all four
> phases, threaded through `Bootstrap` exactly as `ceiling_sweep_period` is.
>
> **`RETENTION_PHASE_BUDGET_MS = 600`.** Each phase must complete inside 600 ms of
> writer time, measured, not asserted — under a third of the 2 s reply bound. The
> per-phase **batch is a hypothesis, not the bound**; a phase stops on whichever
> comes first, its batch or its deadline.
>
> **The reduction rule.** If a phase's measured `elapsed_ms` exceeds
> `RETENTION_PHASE_BUDGET_MS`, that phase halves its batch for the next tick; floor
> 1. A phase at batch 1 still over its share is **split**: the child-delete tick and
> the parent-delete tick become two ticks, and the period is doubled (to at most one
> hour) until the phase meets its share at a batch ≥ 1. A phase never protects a row
> by failing its transaction, never retries in line, and never lengthens a caller's
> bound.
>
> **`elapsed_ms` is required.** Every phase logs, and every receipt row carries, its
> own wall-clock duration. Alongside it a phase logs `consumed` and `remaining`
> (`saturating(COUNT(*) − ceiling)` for that phase's own corpus). `remaining > 0` is
> the standing over-ceiling report.

### The receipt: one table, one name, one shape

```sql
CREATE TABLE IF NOT EXISTS retention_prune_receipts (
  sequence    INTEGER PRIMARY KEY AUTOINCREMENT,
  phase       TEXT    NOT NULL CHECK(phase IN
                ('messages','peer','execution','engagements','decisions')),
  pruned      INTEGER NOT NULL CHECK(pruned >= 0),
  oldest_ref  TEXT    NOT NULL CHECK(length(oldest_ref) <= 256),
  newest_ref  TEXT    NOT NULL CHECK(length(newest_ref) <= 256),
  remaining   INTEGER NOT NULL CHECK(remaining >= 0),
  elapsed_ms  INTEGER NOT NULL CHECK(elapsed_ms >= 0),
  payload     TEXT    CHECK(payload IS NULL OR json_valid(payload)),
  at_ms       INTEGER NOT NULL
) STRICT;
CREATE INDEX IF NOT EXISTS preceipt_at ON retention_prune_receipts(at_ms);
```

One row per phase per tick, written **inside** that phase's own `Immediate`
transaction — the receipt exists iff the phase committed, exactly as the
Decision section's receipt rule states — carrying the count, the oldest and
newest reference it removed, the phase name, the over-ceiling figure, the
cost and the clock. `elapsed_ms` is sampled immediately before the commit
and therefore **excludes** the commit's own cost and any SQLite lock wait;
the post-commit sample is what the batch-reduction rule consumes (the
Decision section's receipt rule is the single statement of this; this
clause defers to it). A phase writes its row when `pruned > 0` **or**
`remaining > 0`; a zero-work phase writes nothing. A writer inserts the
receipt and trims it **by this clause** in the same step:

```sql
DELETE FROM retention_prune_receipts
 WHERE sequence NOT IN (SELECT sequence FROM retention_prune_receipts
                        ORDER BY sequence DESC LIMIT :RETENTION_RECEIPT_LIMIT);
```

with **`RETENTION_RECEIPT_LIMIT = 100`**, written in both places a receipt is ever
written — the tick's phases **and** the in-write decision trim.

`oldest_ref`/`newest_ref` mean, per phase: `messages` → `admitted_messages.sequence`;
`peer` → `peer_messages.sequence`; `execution` → `runner_dispatches.id`;
`engagements` → `engagements.rowid`; `decisions` → `decisions.rowid`.

**The `payload` column is part of this one `CREATE TABLE IF NOT EXISTS`.** The
engagement phase must carry the per-id terminal state it erases
(`[{id,state,ended_at}]` plus the per-table child counts), which the two reference
columns cannot. It is defined here, in the create, rather than added later by an
`ALTER TABLE`: a later `ADD COLUMN` on a shared table fails the store's own rule
that no `ADD COLUMN` migration supports a fixture replay (the comment at
`025-alert-transitions.sql:1-12`). Every later retention migration writes the same
`CREATE TABLE IF NOT EXISTS` shape.

The table is created by the **first** retention migration to land on the
implementer's base; the `verify` probe array gains one
`SELECT … FROM retention_prune_receipts LIMIT 0` line when it first lands, and that
line must hold on the already-current branch too.

**Two content stores are NOT receipts.** A summary row cannot answer a lookup, so a
phase with a post-prune reader needs the bytes or the identity: Slice 1's
`retained_message_archive` (keyed `UNIQUE(engagement_id, source_key)`, bounded to the
same ceiling) and Slice 7's `retained_peer_index` (`(source_key PK, digest, sequence,
pruned_at_ms)`). Neither is a receipt and neither is this table.

### Ownership of the shared tables

A shared table has exactly **one pin owner** — the slice that states the release
proof and owns the bound — and a **named set of delete rights**: **bound delete**
(the owner's count/age prune) and **cascade delete** (another slice's transaction
removing the row because its parent is removed). No tier double-claims a table for
the same purpose, and two slices never delete the same row in the same tick —
structurally impossible, because there is one writer thread and phases run in
separate transactions.

| Table | Pin owner (owns the bound / states the release proof) | Delete rights |
|---|---|---|
| `runner_attempts` | **Slice 2** — pinned unconditionally until a clock bound exists that both late paths enforce | **Slice 6** — cascade, tier 1 |
| `task_outbox` | **Slice 2** — out of scope for the prune (`delivered` is never set; the pager `task_events` is unread in production) | **Slice 6** — cascade, tier 1, **scoped to rows whose owning task is deleted in the same transaction**, honouring the `canonical_tasks` FK |
| `task_inputs` | **Slice 1** — pinned while the canonical task is not `done` | **Slice 6** — cascade, tier 1 |
| `session_inputs` | **Slice 1** — unprocessed or claimed-but-unprocessed | **Slice 6** — cascade, tier 1 |
| `dispatch_inputs` | **Slice 1** — live or `outcome_unknown` dispatch | **Slice 6** — cascade, tier 1 |
| `matrix_ingress_events` | **Slice 1** — the provenance row moves with its message, so it pins nothing | **Slice 6** — cascade, tier 2 |
| `owned_task_completions` | **Slice 2** — the pin, stated both ways | **Slice 6** — cascade, **tier 1**, ordered **before** `task_operation_receipts` (composite FK) and **before** `final_replies` |
| `decisions` | **Slice 3** — in-write, never a phase | **Slice 3 only** — no other slice may name `decisions` in a phase or a cascade |
| `runner_outputs`, `task_operation_receipts`, `graph_commands`, `final_reply_calls`, `conversation_operations`, `usage_receipts` | **Slice 2** — the per-dispatch bound | **Slice 6** — cascade, tier 1 |
| `retained_message_archive` | **Slice 1** — owns the pin and the window | **Slice 6** — cascade, tier 2 |
| `peer_messages` | **Slice 7** — the bound, the pin rule and the identity-store insert | **Slice 6** — cascade, tier 1 |
| `peer_session_inputs` | **Slice 7** — unread or claimed-but-unprocessed | **Slice 6** — cascade, tier 1 |
| `peer_dispatch_inputs` | **Slice 7** — live or `outcome_unknown` dispatch | **Slice 6** — cascade, tier 1 |
| `retained_peer_index` | **Slice 7** — owns the `PEER_RECEIPT_CEILING` bound | **Slice 7 only** — bound delete, no cascade |

### The named product decisions

Every product decision the retention slices name, with the ADR that states it. A
decision named in an ADR is a decision; the same fact left in a predicate is a bug
waiting for a reviewer.

- **D-1 — Unknown-fate retention.** *"A row whose dispatch outcome is
  `outcome_unknown` is retained indefinitely, including through a dispatch recovery.
  The pinning pair is P4 and P5, not P5 alone; `unresolved_dispatches` (`009:23-26`)
  is narrower and is for reporting, not pinning."*
- **D-2 — Archive window.** *"Native's archive is bounded to the same ceiling, so
  content older than ~two ceilings is gone; 'archive, do not lose' holds only inside
  that window, and the archive's own prune runs in the same tick."*
- **D-3 — The idempotency window of `decisions`.** *"The newest
  `DECISION_RETENTION_LIMIT = 500` verdicts by `rowid` replay idempotently; outside
  it a command is refused — `NotFound` for `approve`/`reject`/`revoke` (their replay
  check precedes every mutation), and `NotFound` before the effect reset for
  `retry_cleanup`."* The `retry_cleanup` clause is superseded by D-3's own
  implementable form in ADR-095's decision amendment: the command is **excluded from
  the bound**, not refused.
- **D-4 — Engagements as the record cap.** *"Count-only, cap `ENDED_LIMIT = 500`,
  ordered `rowid ASC`; `ended_at` is advisory metadata, never `DEFAULT 0`; a pruned
  id can be re-admitted and starts from `pending`."*
- **D-5 — What the operator sees after a prune.** *"One read per slice, no page
  work: `retention_status` ({corpus_rows, ceiling, over_by}),
  `execution_retention_status`, `engagement_retention_status`, plus the peer phase's
  `remaining` and the shared receipt row. `remaining > 0` is the standing
  over-ceiling report."*
- **D-6 — `task_outbox` is out of scope.** *"Not pruned by Slice 2: `delivered` is
  never set — no writer ever sets it — and the pager `task_events` is unread in
  production (zero production callers; the cursor is caller-supplied and persisted
  nowhere), so a bound needs a real acknowledgement path first, named as a
  retained-product gap and not designed here."* The premise an earlier form of this
  decision carried — that `task_events` pages a production surface — is false and is
  corrected here; the conclusion (out of scope for the prune) survives it. The
  cascade delete of `task_outbox` rows whose owning `canonical_tasks` row is deleted in
  the same transaction is Slice 6's, **tier 1**, honouring the `canonical_tasks` FK.
- **D-7 — Every `runner_attempts` row is pinned.** *"Until a clock bound exists that
  both late paths enforce; named as an explicit non-goal, not an accident."*
- **D-8 — The 'newest 1 accepted output' residue.** *"One accepted `runner_outputs`
  row per `(dispatch_id, fence)` survives for every fence that ever completed —
  bounded (one row per fence), never expires; named as a deliberate residue."*
- **D-9 — Provenance moves with the message.** *"The `matrix_ingress_events` row is
  not a pin: it is archived with its message and deleted in the same transaction; the
  archive is keyed on the pair `(engagement_id, source_key)` and every archive read
  is engagement-scoped."*
- **D-10 — Task lifecycle is the canonical task's, not the intent's.** *"The gate is
  `json_extract(canonical_tasks.config,'$.status')='done'`, whose writer is
  `finish_task_clock` (`owned_completion.rs:83-131`) and which is terminal
  (`execution.rs:458-460`); `task_intents.state='closed'` has no production writer
  and is never a release."*
- **D-11 — Spend is forgiven at re-admission.** *"A pruned engagement's
  `usage_sources`/`usage_periods` do not survive, so a re-admitted (deterministic) id
  starts from zero; safe for admission, named as a consequence."*
- **D-12 — A retention failure is never a work refusal.** *"Log and retry next tick;
  the store's caps (`100 000`, `10 000`, `30 000`) refuse at a bound, and retention
  must not repeat that shape."*
- **D-13 — `matrix_ingress_events`' 100 000 stop is a backstop, not the bound.**
  *"With D-9 the provenance row drains with its message; the stop stays as the same
  all-pinned condition that leaves `remaining > 0` on the admitted corpus — one
  property, two stops."*


### The implementation shape

**The loop carries per-phase state.** The tick's batch is a vector, one hypothesis
per phase (the table above), not one scalar: a phase that reduces its batch never
reduces another's. The tick's observation enum is phase-tagged, so a receiver can
tell two phases apart. *(Depends on the builder's edits commit — see below.)*

**The tick is observable and abortable.** The receiver the loop publishes to is
threaded into the served app exactly as the ceiling sweep's is, so `/health`
reports liveness and a test can await a single tick; and the task's handle is read
on close, so the sweep is aborted at shutdown as `ceiling_sweep` is. *(Depends on
the builder's edits commit — see below.)*

**What the broken-out slice is held to, and what it does not yet meet.** The
brief-45 wiring review read the landed Slice 1 wiring and found it faithful to this
contract clause-for-clause **except** on the two clauses above and one diagnostic:
the loop's batch is one scalar capped by the `messages` const, so a phase could not
carry its own hypothesis; the tick's `watch` receiver is dropped (`let _ =
retention_tick;`), so the loop has no liveness or observation hook; and the
per-tick log names `pruned` where this contract's measurement is `consumed`. The
two sentences above hold **only once the builder's edits commit lands them**
(per-phase batch state, a phase-tagged observation enum, the receiver threaded into
the served app, and the handle read on close). Until that commit the loop is
single-phase, its batch is one scalar, its receiver is dropped and its handle is
never read — this ADR does not claim a shape the tree does not have.

**The split is a named residue, not a claimed conformance.** The reduction rule's
**split** response (a phase at batch 1 still over its share) is **not implemented
by Slice 1**; this contract names it as an unimplemented residue so a later reader
does not mistake it for wired behaviour. Nothing else in the reduction rule is
affected.

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
status method; because the peer slice's schema-head move — fifteen test files'
`26`→`27` pins plus the `domain.rs` registry literal — lands in the same
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

**Phase-2 alternatives (peer corpus).**

- **Full content archive (phase 1's shape).** Rejected — bytes no post-prune
  reader can reach; the only reader (`admit`'s idempotency lookup) needs
  identity, not content.
- **Delete with no identity store.** Rejected — loses the
  `Conflict`/`replayed` answer for a pruned key.
- **Pin while any child row exists.** Rejected — the child rows are never
  deleted, so it pins everything.
- **A hard cap and refusal at the ceiling.** Rejected — stops coordination
  rather than reclaiming.
- **A plain pin on `graph_nodes.message_sequence`.** Rejected — no writer
  clears it, and the graph-pinned set can reach 128 000 against a 5000
  ceiling.
- **`unresolved_dispatches` as the pin predicate.** Rejected — it overturns
  the tick contract's D-1 without amending it, and would leave the admitted
  and peer corpora encoding two different pin rules in one store.
