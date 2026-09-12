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
