---
kind: decision
id: ADR-125
title: Admitted-corpus retention, phases one and two
status: Proposed
requirements: [REQ-RUST-MIGRATION-EXECUTION]
tags: [retention, store, peer-corpus, migration-027]
---

## Context

ADR-125's landed record (phase 1, `messages`, migration 026) fixed the tick
contract the whole retention family obeys: one `Immediate` transaction per
phase, the shared receipt table `retention_prune_receipts` (one row per phase
per working tick, written **inside** the phase's own transaction, trimmed to
100 by the same writer), a 600 ms per-phase budget, per-phase batch state
(one slot per phase, initialized from that phase's hypothesis, halved on
*that phase's own* breach, floor 1, the split at batch 1 a named visible
residue). This amendment adds the second phase the contract's fixed order
names — **`peer`**, after `messages` and before `execution` — because the
peer corpus is the second-fastest-growing surface and its deletion is a
leaf/mid phase that may not presuppose later phases' work.

The peer corpus is three tables (`007-peer-inputs.sql`): `peer_messages`
(the event: `sequence AUTOINCREMENT`, `source_key` idempotency key, digest,
conversation, config) and its two children `peer_session_inputs`
(per-recipient delivery rows: `wake`, `dispatch_id`, `processed_at`) and
`peer_dispatch_inputs` (per-dispatch claims). A fourth reference site links
graph custody in: `graph_nodes.message_sequence`, canonical (equality-checked
on every workflow read), not a projection. Retained's counterpart bound its
`messages` array by unread-inbox exemptions; native's table is per session
(one agent may hold several), so per-session pinning is at least as strong.

## Decision

**The bound.** A `peer_messages` row is a prune candidate only when **no**
pin holds: P1 recency (outside the newest `CEILING` by sequence); P2′
unprocessed-and-live conversation input; P3′ claimed-not-processed
(`dispatch_id` set, `processed_at` null — `dispatch_id` alone would pin
forever); **P4** any live dispatch (`queued/leased/started/parked`) claims
it; **P5** an `outcome_unknown` dispatch claims it (the tick contract's D-1:
the raw state pins; `unresolved_dispatches` is for reporting, never
releasing); P6 a graph node holds it inside its release boundary. `wake`
never pins (set once, never cleared — it would pin every Request/Response
forever).

**The archive: delete the row, keep a bounded identity store.** Pruning
deletes the row **with its children and the paired graph move**, and records
**identity, not content**, in `retained_peer_index(source_key PK, digest,
sequence, pruned_at_ms)` — migration **027** (the ledger's RT-7 number),
created `IF NOT EXISTS`, its prune oldest-first **in the same tick**, bounded
by a DDL-level constant (`PEER_RECEIPT_CEILING`), not a prose claim. The one
post-prune reader — `admit`'s idempotency lookup (`peers.rs:166-182`), which
answers `Conflict` on digest mismatch and `replayed:true` on match — consults
the index on a live miss, so a redelivered send keeps its answer after the
corpus row is gone.

**The delete is children-first, in one transaction.** Per candidate, inside
the phase's single `Immediate` transaction: the paired graph move (when the
node is releasable), then `peer_dispatch_inputs`, then `peer_session_inputs`,
then `peer_messages`, then the identity insert. The invariant — a parent row
is deleted only in the same transaction as all of its children — is what
keeps `peer_inbox` and `validate_dispatch` from hard `NotFound`s; the
`RESTRICT` FKs are the backstop, not the mechanism.

**No consumer observes the prune.** The safety lives in the consumers, not
the views: three of the six shared views do lose the pruned pair, and every
consumer that could see it carries its own unprocessed/live conjunct
(`peers.rs:115/:277`, `execution.rs:743`, `task_intents.rs:366`,
`graphs.rs:356`). The tests assert over the consumer queries, with a
non-empty precondition so a vacuous view-equality cannot pass.

**The phase receipt and batch state.** The `peer` phase writes its
`phase='peer'` receipt row **inside its own transaction** — the CHECK's
vocabulary already admits the word — carrying pruned/archived/remaining and
its own measured `elapsed_ms`; the same writer trims to the shared limit.
Its batch is **its own slot** in the per-phase state (never the `messages`
scalar), initialized from its own hypothesis, halved on its own breach,
floor 1, the split a named visible residue. The bootstrap wiring appends the
phase to the one retention loop's submission order, logging with the
`[peer]` prefix; `Busy`/`OutcomeUnknown` wait for the next tick, never an
in-line retry.

**Ownership of the shared tables** (the tick contract's §4, cited not
restated): this phase is the pin owner of the three corpus tables; cascade
delete rights remain the later execution phase's — invisible to a grep
today and exactly why the contract records it.

**The graph-move's two builder-facing consequences, named (the reconciliation
map's C12/C13/C16).** (C16) The graph move is a **release of custody**: a
moved binding is NULL, so a terminal-node read that no longer matches
returns the arm the design chose — the message is gone and the recovery must
be re-driven from identity, never silently re-admitted. (C12) Concretely,
`admit_recovery` (`graphs.rs:374`) admits a `complete` node *as well as*
live states, so **a moved binding refuses there with `RunnerAuthority` — and
that is the intended direction**, not a defect: the recovery a pruned
message would authorize is exactly the replay the identity store covers.
(C13) The P2′ release is **three-triggered, not one**: besides a closed
conversation (`close()` fences and sets `state='closed'`), the pair also
leaves `conversation_peer_inputs` when its **engagement deactivates**
(`write_engagement`'s state write) or its **session binding is lost** (an
`internal_participants` row deleted on the session-retire path). All three
are the same direction — release, never re-pin — so P2′ stays monotone, and
the acceptance line is: *if a reopen, engagement reactivation, or
participant re-insert appears on the base, P2′ must be revisited.*

## Consequences

Good, because the second-fastest corpus gains the same bound, the same
receipt discipline and the same one-transaction tick as phase 1, the
identity store keeps the idempotency answer alive past the prune, and the
graph move keeps workflow reads coherent.
Bad, because `outcome_unknown` pins are unbounded until a D-1 settlement
surface exists (named as that class, not wired), and the phase adds a
second migration's head-pin churn to every test that pins the head.

## Alternatives Considered

- Archive full content, phase 1's shape — rejected: no post-prune reader
  needs content, only `sequence,digest` identity; the corpus's volume makes
  a content archive the unbounded surface this family exists to remove.
- Pin on `wake` — rejected: set once, never cleared; every wake-carrying
  message would pin forever.
- Release `outcome_unknown` rows via `unresolved_dispatches` — rejected per
  D-1: the reporting view must never gate retention; the raw state pins.
- Prune without the graph move — rejected: a moved binding must be NULL or
  the workflow read returns `Error::State`; the move is part of the delete.
