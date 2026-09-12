---
kind: decision
id: ADR-099
title: "Observe original approval setup and repository field destruction"
status: Accepted
requirements: [REQ-RUST-MIGRATION-EXECUTION]
---

## Context

The original Windows 1da8f1b suite failed six approval library tests. Four fail
inside the fixture's direct SDK open after authenticated bootstrap; one fails
inside direct SDK close before corrupt-journal restoration. These five original
ten-second acknowledgement timeouts have no attached approval fixture trace.
The wrong-device case fails during domain cleanup: the writer picked shutdown
up at 47 microseconds and entered repository drop at 48 microseconds, but had not
published drop completion at the original two-second reply timeout. The
connection destructor, ownership-file release and OS scheduling inside that
interval are not distinguished. No backend cause is established.

All original job logs, metadata, exact Cargo boundaries and hashes are retained
externally in 1da8f1b-original-ci-evidence.md and its hash manifest. The original
Windows verdict is failure. The separate outgoing diagnostic passes and the
separate transport diagnostic fails one originally passing case; neither result
replaces the original suite. Positive Windows staging remains unqualified.

## Decision

Use the existing finite per-operation trace around original approval fixture
bootstrap, direct SDK open, direct SDK close and Collector teardown. Fixed
callsite and variant labels contain no payload, identity or backend error text.
The existing observe and close tasks capture the same trace only in test builds;
no additional task, permit, owner, retry or production behavior is introduced.
Original errors and request scripts remain unchanged.

Extend the existing optional shutdown observer with connection-drop start and
finish and ownership-file-drop start and finish timestamps. The original
repository owns exactly one Connection followed by its ownership File. Explicit
destruction retains this same order, including ownership cleanup on unwind,
without a new query, checkpoint, wait or explicit SQLite close protocol. The
worker still acknowledges only after both resources drop. Unobserved shutdown
allocates no Probe. Fixed timestamps remain available when the store is linked
as a dependency; deterministic pause injection remains test-only.
The finite snapshot cap increases by 64 bytes for exactly four Option<u64>
fields, from 144 to 208 bytes; independent probe publication remains unchanged.

## Consequences

A future original failure can identify the actual field destruction interval
or SDK phase reached before its unchanged deadline. Missing timestamps remain
unobserved, never rollback, retry authority or proof of released ownership. The
shared snapshot's new domain-specific fields remain absent for custody-store
shutdown, which is outside this change. Existing snapshot phase meanings and
both original shutdown deadlines remain unchanged.

Actual writer tests hold connection and ownership-file boundaries, retain the
original timed-out result, check the real private lock, then release the same
writer and perform a separate real reopen. An actual approval SDK lifecycle
test holds its original thread while checking lock custody and the original
spawned close trace. These checks qualify observation behavior only; neither
local tests nor Windows compilation fixes the historical hosted failures.

## Alternatives Considered

Changing timeouts, WAL checkpoint policy or test concurrency would alter the
incident without establishing its cause. A second operation or global latest
trace could attribute a new result to the failed owner. Keeping all destruction
inside one marker would leave the currently observed interval unresolved.
