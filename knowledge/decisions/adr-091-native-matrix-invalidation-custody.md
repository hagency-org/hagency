---
kind: decision
id: ADR-091
title: "Retain enqueued negative Matrix observations through caller loss"
status: Accepted
requirements: [REQ-RUST-MIGRATION-EXECUTION]
---

## Context

Dropping a caller after a negative Matrix observation entered the writer queue could discard required retirement and leave cached authority available.

## Decision

DomainStore::call skips an operation if its result receiver is closed before
writer pickup. This protects ordinary work from executing after its caller
abandons it. Applied to an already observed negative Matrix identity or room
state, however, dropping the caller can discard the queued retirement and leave
cached authority available. The source trace arose while reviewing ADR089's
current whoami failure path; it is independent of any historical CI failure.

A private call execution mode preserves the existing cancellation check by
default. Only invalidate_matrix_transport and invalidate_matrix_room request
execution after successful queue admission even if their reply receiver closes.
The original byte permit and finite queue retain the original closure until
writer execution. No additional task, queue, map, retry or receiver is retained.
The original two-second reply timeout still returns OutcomeUnknown; subsequent
execution does not replace that missing acknowledgement with caller success.

The original host supplied invalidation data and domain transaction are unchanged.
Transport invalidation compares the complete expected incarnation; room
invalidation compares registration, transport and room generation. A delayed
negative observation cannot target a newer incarnation. Existing shared route,
session, approval and uncertain-send retirement stays inside the same original
transaction. Positive observations and work producing calls retain the ordinary
receiver cancellation check. The private mode is not new positive authority.

The guarantee begins only after successful enqueue. Capacity, byte admission or
closed-worker errors cannot promise retirement. A host must retain an observed
negative result before admission and stop relying on that scope if persistence
fails. ADR089's finite observation owner handles its own earlier lifetime; this
domain change does not retain a caller's unpolled future or invent a write.

Tests use an actual domain repository and its real writer, held by a finite
private fixture gate. They poll the real public invalidation until its original
queue slot is occupied, drop its caller, release the same writer, and query the
result separately. Replacement observations execute on that same writer before
the old queued invalidation. Another fixture awaits the unchanged real timeout,
keeps the original OutcomeUnknown result, then separately observes eventual
retirement. Ordinary positive observation and task creation are still skipped
after receiver loss. These host fixture observations prove domain custody and
generation guards, not authenticated HTTP provenance, Windows execution or a
fix for earlier CI failures.

## Consequences

Only the two admitted negative invalidations retain execution through receiver loss, under the original finite queue and generation guards. Ordinary abandoned positive work remains skipped.

## Alternatives Considered

Retaining all cancelled commands would change ordinary mutation semantics. Treating an unpolled future as queued work or replaying an old negative against a replacement generation would create authority the original observation did not have.
