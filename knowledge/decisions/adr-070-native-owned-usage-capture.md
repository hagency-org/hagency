---
kind: decision
id: ADR-070
title: Capture native usage under the exact owned dispatch before ledger attribution
status: Accepted
---

## Context

Runtime usage evidence and durable ledger sources must meet inside the same owned dispatch rather than through a restorable textual session identity.

## Decision

ADR069 supplies actual scoped runtime observations; ADR063 persists historical
usage sources. Connect them only inside the private owned-execution operation.
The acknowledged Started scope binds its ledger source before child creation.
Failure or a lost binding response launches no child. Only that operation's
OwnedSession is attached, after its fresh thread/start and turn/start. This path
exposes no resume or restored-source constructor. Historical ledger restoration
never becomes permission to attach another process, even with identical textual
thread and turn IDs.

Keep the exact opaque driver source and consume every observed update in order.
A foreign source, missing sequence, retired source or invalidated observation
permanently closes new capture. Completion closes capture too; a historically
valid source is not a live process. Usage is projected into ADR071's fixed
untrusted counter input without fabricating transcript JSON. Attribution comes
only from Started, never counter values, model metadata or transcript paths.

Each admitted usage observation has a host-generated runtime_v1_<sequence> call ID.
Retain one exact source/call/content tuple before awaiting the existing bounded
writer. A lost response or cancelled wait leaves that tuple intact. An explicit
report retry submits only this same tuple; it never retargets the source, changes
the call ID, reads a replacement process or resumes capture. A pending or refused
write stops further capture with an explicit failure, while the existing owned
runner cancellation, cleanup, canonical completion and delivery rules still run.
No retry changes execution authority, task state, leases or quota eligibility.

The report retains fixed counters and at most one normalized pending observation
or one fixed original projection rejected by normalization. Arithmetic overflow
retains that rejected evidence and static failure; retry never turns it into an
admitted ledger record.
There is no detached writer, unbounded receipt cache or per-event retry loop.
Dropping a report can lose uncommitted observation evidence; it cannot undo a
committed ledger receipt. Process restart permits inspection of durable history,
not reconstruction of missing bytes. Every runtime observation is labelled stream
incomplete: current terminal handling cannot prove that every usage event arrived.
Absence is unknown, never measured zero. Aggregate evidence is labelled untrusted
usage rather than untrusted transcript because it now covers both input forms.

This slice changes no schema, transcript arithmetic or source capacity. It does
not authenticate provider billing, implement quotas, discover transcripts, expose
source controls over HTTP, enable runtime services or qualify production sandbox
and workspace custody. Offline actual native pipes, exact source/sequence guards,
real writer replay and restart evidence are separate acceptance boundaries.

## Consequences

Capture begins only after acknowledged source binding and consumes exact ordered observations. Report retries preserve historical evidence without reattaching another process or hiding stream incompleteness.

## Alternatives Considered

Launching before source-binding acknowledgement or restoring a source into a new process would lose attribution. Detached per-event retries and unbounded receipt caches would split the operation's finite custody.

## Cooperative control preserves usage provenance

The runtime-only approval control pump exposes the same ordered `(Update,
Observation)` values as ordinary session reads. Private control output is a
separate enum variant without a fabricated observation or sequence increment.
Updates returned while an original prepared response waits for its first byte
also advance the same opaque sequence exactly once. The runtime retains partial
frames and buffered suffixes across successful control returns. The host remains
responsible for handing every observation to the original usage capture before
continuing; neither an in-flight approval-grant future nor a new callback permits
skipping a usage receipt. This slice changes no ledger, usage retry, attribution,
execution operation or native application proof.
