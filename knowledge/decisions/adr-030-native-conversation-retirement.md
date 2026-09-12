---
kind: decision
id: ADR-030
title: Retire internal conversation authority separately from process custody
status: Accepted
requirements: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREE-LAYER-COMPLETION]
---

## Context

Removing internal conversation membership must revoke execution authority without erasing tasks, messages or unresolved process effects.

## Decision

Native internal groups use exact creator-session authority. Their membership
operations carry an expected revision and a dispatch-scoped call ID. The domain
transaction records the normalized request digest and exact response. Retrying a
receipt returns that response; a separate read obtains current state. Changed
payload or stale revision fails. Matrix room membership is a separate adapter
responsibility and is not affected by this API.

Removing a participant retains its session, tasks and immutable message records.
Rejoining allocates a new session derived from the committed membership revision;
the previous session never regains membership authority. Current membership is
unique by conversation/engagement. Matrix session uniqueness remains unchanged.
The creator's own engagement remains an internal participant until closure.

Closing a group removes current membership and closes child groups created by
retired internal sessions. Queued and leased-but-unstarted dispatches become
superseded. Started, parked and previously unknown work receives a durable stop
intent, loses its execution capability and remains outcome_unknown. This includes
a creator's frozen peer batch containing input from the closing group. The batch
cannot be silently edited after start. No task becomes done during retirement.

Uncertain retired work retains logical leases, workspace dirtiness and session
quarantine until the host proves its process scope stopped and inspects effects.
Pending stop intents survive restart and count against execution concurrency.
Settlement is a host-only transaction with an exact dispatch fence and an
idempotent evidence record. A string in that record is not process-stop proof;
the actual runner adapter must establish custody and workspace observations first.
No runtime command or HTTP endpoint accepts such an inspection assertion.

Settlement releases only the inspected dispatch's leases and clears dirtiness or
quarantine only where no other unresolved attempt remains. It releases unprocessed
input assignments without acknowledging delivery, preserves frozen inspection
records, and never recovers retired instructions into another execution attempt.
Current input from unrelated live groups may then be scheduled independently.

The process adapter, real stop observation, Matrix membership, final reply delivery
and production cutover remain migration gates. Deterministic repository fixtures
prove transactions and authority, not termination of a real Agent process.

## Consequences

Retirement preserves immutable history and dirty leases until exact host inspection settles the original attempt. Rejoining creates a new session incarnation rather than restoring old authority.

## Alternatives Considered

Deleting retired sessions or immediately releasing their resource leases would hide unresolved execution. Reusing the old session on rejoin would let historical capabilities regain membership authority.
