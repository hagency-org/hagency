---
kind: decision
id: ADR-031
title: Bind finite graph progress to canonical tasks and durable peer assignments
status: Accepted
requirements: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREE-LAYER-COMPLETION]
---

## Context

Finite graph progress must survive its creator process while remaining tied to canonical tasks, current membership and durable peer assignments.

## Decision

Schema 10 connects the pure graph planner to the domain writer. A current started
creator submits a bounded definition and exact internal participant session IDs.
The writer validates conversation membership, project and allocation generation,
then creates the graph and every canonical node task in one transaction. Tasks
retain the creator and optional parent task, and begin as created. The definition
has at most 128 nodes and its request is at most 64 KiB. A bound graph-node task
cannot submit a nested graph through this command.

The finite definition is durable authority to activate these existing tasks when
their dependencies are satisfied. Its creator process need not stay running.
Allocation and exact conversation membership must remain current; process
quarantine is not membership retirement and is not a new runtime capability.
Generic task or peer delegation remains a separately authorized operation.

Planning progress, assignment messages, recipient input, pinned dependencies and
command receipts commit together. Each node has one stable task and assignment
identity. Host dispatch must bind both that task and its admitted peer input;
generic enqueue cannot bypass readiness by supplying a task ID or instruction.
The host chooses workspace resources using the existing lease mechanism. Runtime
graph JSON cannot supply dispatch identity, process evidence or resource grants.

A successful node result requires a current capability for that exact node task
and explicit canonical done state. The result is bound to its completed execution
epoch. The result transaction never marks a task or parent done. A failed node
requires explicit canonical blocked state; dependent failure and conditional skip
leave their undispatched canonical tasks created. Failure fences the failed
node's process just like cancellation. Uncertainty never becomes automatic retry.
This immediately invalidates that failed runner's capability. A repeated failed
result after fencing is refused; the creator reads the committed failure from
the graph if the response was lost. Successful-result replay applies only to a
current work capability or the exact inspected completed-result grant.

Result values are immutable, canonicalized, digest-bound JSON, at most 64 KiB and
depth 64 per node. The stored graph and its public view contain progress metadata
without copying results. Assignment messages carry graph/node/task IDs and a
dependency count. A worker pages at most 32 pinned dependency references and reads
only those values. The exact creator may read any completed graph result. Values
needed for pending conditions are hydrated transiently; reference-only planning
avoids copying large values into every downstream assignment. Dependency order,
fractional conditions, skips and failure propagation use the existing planner.
Large failure summaries are UTF-8 bounded to 4000 bytes; the definition preserves
the complete dependency IDs.

Command IDs are content-bound within a dispatch. Repeating a successful result
after host-inspected recovery is also checked against the node's immutable receipt
and completed epoch. A report-only replacement can supply that exact result and
settle its frozen input, including after the graph becomes terminal. It cannot
create a graph or obtain the creator's graph view. A lost report response never
creates another dependent assignment. Execution settlement requires both explicit
canonical completion and a committed node result; final model text is insufficient.

The exact live creator can cancel the graph. Membership retirement, allocation
revocation and registration rotation reconcile obsolete graph scope atomically.
Queued or leased work is superseded; started, parked and unknown work retains
leases and durable stop intents until host inspection. Rejoining has a fresh
session and cannot revive the old graph. Terminal graph history is preserved when
scope retires, while pending terminal report work is still fenced. Canonical tasks
and input history are retained. This reuses ADR-030 custody settlement and exposes
no runtime inspection endpoint.
Retired unprocessed input does not consume the live pending-input quota. Input
frozen into queued, live or unresolved dispatch custody still counts until that
custody is settled. Quota release never fabricates a processed acknowledgement.
The quota snapshot borrows the writer transaction and is reused only within one
admission batch, incrementing for every new recipient. Graph scope validates the
distinct assignee sessions. Rechecking an entire graph per message per assignment
made a realistic queue-boundary fixture take minutes before these changes.

Expired or restarted started/parked attempts retain shared as well as exclusive
resource leases. All unresolved attempts count against execution concurrency,
including those without a cancellation stop intent. Host-inspected recovery
releases only the original attempt's leases inside the recovery transaction.
Another unknown reader still excludes an exclusive writer. The schema upgrade
restores missing legacy unknown leases from host-owned resource declarations;
already inspected recoveries and settled stops are excluded.

Schema validation uses independently prepared queries. Combining all expanding
views into one cross join exceeded SQLite's 64-table limit during testing.
Every query must validate inside the migration transaction before advancing the
schema version, and again on reopening an existing database.

This is the native domain and private runner API. Actual model adapters, graph
tool exposure, final replies with DM/promotion privacy, scheduling integration,
Matrix/Palpo transport, effective sandbox policy and cutover remain migration
gates. Repository inspection fixtures do not prove termination of live processes.

## Consequences

The domain writer commits bounded graph definitions and node tasks together. Graph readiness does not itself qualify model execution, Matrix delivery or process cleanup.

## Alternatives Considered

Keeping graph progress only in the creator's runtime would lose durable scheduling intent. Allowing nested graph creation or unbounded definitions through this command would bypass the recorded finite authority boundary.

## Amendment 2026-09-13 — the graph-side receipt family in the execution bound (retention Slice 2)

Schema 010's `graph_commands` is this ADR's object: `(dispatch_id, call_id)` →
`digest`/`response`, the graph tool call's replay receipt, written under the same
per-dispatch cap as the dispatch receipt family. Nothing deletes a row of it
(`grep -rn "DELETE FROM graph_commands" native/` returns nothing on `9ef8e684`).
This amendment is the graph half of the retention work whose dispatch half is
ADR-053's 2026-09-13 amendment; the tick it cites is **ADR-125's "Retention sweep
tick" section**, and the phase is the same phase 3, `execution`.

**1. The prunable set from this schema.** `graph_commands` rows for a **candidate
dispatch** — the same candidate ADR-053's amendment defines:
`runner_dispatches.state IN ('completed','superseded')` **and**
`capability_hash IS NULL` **and** the dispatch not listed by `unresolved_dispatches`
— pruned oldest-first, keeping the newest `EXECUTION_RETENTION_DISPATCHES = 500`
settled dispatches, under `EXECUTION_RETENTION_ROWS = 100_000` per-table backstop
and `EXECUTION_RETENTION_BATCH = 64` dispatches per tick. `graph_commands` is keyed
by `(dispatch_id, call_id)` and has no child, so it drains with its dispatch and in
no other way.

**2. What this amendment does *not* prune, and why.** `task_graphs`, `graph_nodes`
and `graph_dependencies` are **not** execution evidence: they are the durable graph
definition and its pinned dependency edges, and a `graph_nodes` row carries the
`message_sequence` that pins a peer message (ADR-127's object). None of them is a
candidate here. `graph_nodes` is also Slice 7's release surface, and this slice must
not touch it. So this phase deletes **no** row of this ADR's three definition tables.

**3. `canonical_tasks` is never pruned by this slice.** A canonical task is durable
completion authority, and its lifecycle gate is a named product decision of the tick
contract (D-10), quoted: *"The gate is
`json_extract(canonical_tasks.config,'$.status')='done'`, whose writer is
`finish_task_clock` (`owned_completion.rs:83-131`) and which is **terminal**
(`execution.rs:458-460`); `task_intents.state='closed'` has **no production writer**
and is never a release."* That gate pins message and task inputs in Slice 1's phase;
it is not a release for a task row, and no phase in this sweep deletes one. A
`canonical_tasks` row leaves only inside Slice 6's engagement cascade (ADR-095's
amendment), where its owning engagement is the parent being removed.

**4. `task_outbox` is out of scope for the bound (D-6).** The tick contract's D-6
reads, quoted: *"Not pruned by Slice 2: `delivered` is never set, the pager is
production, a bound needs a real acknowledgement path first."* The table is this
ADR's (schema 003 admits the outbox for graph-node task delivery), the pager
`task_events(after,limit)` is production (`execution.rs:1079-1083`), and no writer
sets `delivered`. Slice 2 holds **pin only**; the cascade delete inside a candidate
engagement is Slice 6's, and the scoping sentence that reconciles the two is in
ADR-095's amendment. A bound needs a real acknowledgement path first — stated here so
the deferral is this ADR's decision, not an omission.

**5. Receipt, not archive.** The phase writes one `retention_prune_receipts` row with
`phase='execution'` and `oldest_ref`/`newest_ref` = `runner_dispatches.id` (the
dispatch is the unit, not the call). No supported surface reads a `graph_commands`
row after its dispatch is settled — the table exists so a replayed graph tool call
answer its own `call_id` — so the loss is direct-SQLite inspection only, bounded at
500 settled dispatches. The contract's D-5, quoted: *"One read per slice, no page
work: `retention_status` ({corpus_rows, ceiling, over_by}),
`execution_retention_status`, `engagement_retention_status`, plus the peer phase's
`remaining` and the shared receipt row. `remaining > 0` is the standing over-ceiling
report."*

**6. Never a work refusal (D-12).** The tick contract's D-12 reads, quoted: *"Log and
retry next tick; the store's caps (`100 000`, `10 000`, `30 000`) refuse at a bound,
and retention must not repeat that shape."* An over-window corpus reports
`remaining > 0`; it never refuses a graph command, a node or a task.
