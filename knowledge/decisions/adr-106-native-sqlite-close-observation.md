---
kind: decision
id: ADR-106
title: "Observe entry into the original SQLite connection close"
status: Accepted
requirements: [REQ-RUST-MIGRATION-EXECUTION]
---

## Context

The original fed7557 Windows Cargo suite failed five approval cases. Three
original DomainStore shutdowns reached the writer within 54 microseconds and
entered Connection destruction but did not observe its completion before the
unchanged two-second reply deadline. Two separate original SDK opens exceeded
ten seconds during fresh fixture setup. The exact failures and source review
are preserved outside the repository; none has a proven backend cause.

ADR099 distinguishes connection and ownership-file destruction. Its connection
interval still includes rusqlite cache finalization and entry into SQLite as well
as SQLite's actual close work. Pinned rusqlite 0.37 exposes the safe trace_v2 API
with a CLOSE-only event. Bundled SQLite 3.50.2 emits that event after acquiring its
connection mutex and before checking outstanding statements or cleaning up WAL
and shared-memory resources. Consequently the event proves entry, not successful
close, transaction completion, lock release or acknowledgement.

## Decision

Add exactly one optional sqlite_close_entered_us timestamp to the existing
per-original ShutdownSnapshot and one corresponding SqliteCloseEntered phase.
An observed DomainRepository drop registers only SQLITE_TRACE_CLOSE on its
original Connection. The safe callback ignores its ConnRef entirely and records
only this fixed phase into that operation's existing finite Probe. No SQL trace,
row/profile event, SQL text, filename, identity, backend message or connection
address is read or logged.

Associate the callback using a scoped worker-thread-local slot holding at most
one Arc to the original Probe. An RAII guard is confined to that thread and
clears only its own association on normal return or unwind. A nested or
unavailable slot must leave the new phase unobserved rather than overwrite an
active association, add an error or change destruction. The callback publishes
through the existing atomics without holding a RefCell borrow during the mark.
The guard owns no repository, connection, sender or task and cannot form a cycle.
Normal unobserved shutdown installs no callback, accesses no observation slot and
allocates no Probe. Custody Store snapshots keep the new domain phase absent.

Enable only rusqlite's empty trace feature in native/hagency-store/Cargo.toml.
Cargo unifies that Rust API feature for the existing pinned package; it adds no
package, version, lockfile entry, SQLite C build flag or runtime logger. This
feature availability is distinct from installing an actual per-connection hook.
The existing deny-unsafe-code policy remains unchanged.

Retain the exact connection-before-ownership-file destruction sequence and
original cleanup on unwind. Preserve both existing two-second shutdown waits,
queue limits, every original result and acknowledgement only after destruction.
The callback introduces no database query, checkpoint, retry, wait or worker.
Any deterministic pause is test-only at the real callback in the actual writer.

## Consequences

The new marker distinguishes waiting before SQLite's close entry from waiting
inside its cleanup. It does not distinguish a global Windows VFS mutex from
file I/O or scheduler delay, and does not explain the two SDK initialization
failures. Missing entry or finish remains unobserved. Later successful cleanup
must never replace the original timeout result or fabricate a sent ACK.

The finite snapshot ceiling increases from ADR099's 208 bytes to 224 bytes for one
Option<u64>; this decision replaces only that numeric ceiling and preserves all
prior phase meanings. The Probe has 13 timestamp cells within its existing
AtomicU16 publication mask. There is no global latest-operation map or expanding
log. Actual concurrent and sequential repository closes must prove association
and guard cleanup, including unchanged normal shutdown.

Acceptance retains all five original failing Matrix selectors and the existing
shutdown boundary tests. Local passes, Windows GNU compilation and later
hosted diagnostics qualify only their actual execution; they do not replace the
original failed verdict or qualify positive Windows file staging. Actual callback
tests establish observation behavior only; they do not establish the original
Windows cause.

A separate open dependency review concerns SQLite's documented WAL-reset race:
the original bundled 3.50.2 predates fixes in 3.51.3 and the 3.50.7 backport. That
race requires overlapping writes/checkpoints on the same WAL database and is
not established in these original failures. Its dependency assessment and any
upgrade remain outside this observation slice. Primary scope:
https://www.sqlite.org/wal.html#the_wal_reset_bug .

## Alternatives Considered

A raw SQLite FFI hook or process-global logger would add unnecessary safety and
association concerns when the pinned safe CLOSE-only API already exists.
Explicitly closing or checkpointing the database would change cleanup policy.
Changing timeouts or serializing tests would alter the incident without tracing
its cause. Adding further SDK internal traces is deferred until separately
justified; this slice adds only the smallest meaningful connection boundary.

## Accepted extension: native original writer accounting

Original 22c4993 Windows Cargo records four same-connection CLOSE entries within
68 microseconds but no connection-drop completion by the unchanged two-second
reply deadline. The pinned source places CLOSE after cache flush, rusqlite's
interrupt mutex and SQLite's connection mutex. Later pager/WAL/VFS work remains
unobserved. A global SHM cleanup mutex and native IO/retry paths are source-derived
possibilities only. No blocked operation or production repair is established.

An already-observed domain drop may now retain one real Windows thread handle
duplicated from its actual current writer with THREAD_QUERY_LIMITED_INFORMATION
only. It is non-inheritable, is owned by that same finite Probe, and is never
reopened by numeric thread ID. A baseline GetThreadTimes reading is taken
immediately before ConnectionDropStarted. Its interval begins at this observed
domain-drop entry, not necessarily at the later SQLITE_TRACE_CLOSE marker.

The existing snapshot path performs at most one further native time query and
freezes either its checked CPU deltas and native PID/TID or an explicit unavailable
stage. Later snapshots cannot retry a failed query or charge later work on that
thread to the original measurement. This fixed Copy value has unobserved and
unsupported states as well. The owned handle persists through the query, including
writer exit, and closes through RAII with its Probe. It retains no repository,
job, sender or callback association and forms no ownership cycle.

The in-memory snapshot ceiling becomes 256 bytes, replacing only the previous
224-byte numeric bound. Its complete Debug projection must fit 2048 bytes for
maximum-width scalar values and every state label. No handle value, thread-object
name, filesystem path, SQL, payload or native error string is exposed. Native
CPU deltas cannot distinguish IO, mutex wait, sleep or descheduling and carry no
completion, retry or authority meaning. Original caller timestamps and verdicts
are frozen before the normal observed-shutdown snapshot; query availability does
not influence either.

The crate's deny-unsafe-code policy remains the default. A single isolated
shutdown/native_writer/windows.rs boundary permits only audited Win32 native
handle duplication and CPU queries (plus actual test fixtures). Existing
windows-sys features suffice; no dependency, SQLite hook/configuration, process
privilege, thread suspension, new worker, retry or deadline is added. Ordinary
unobserved shutdown does not capture or query a native writer. Generic custody
snapshots leave the domain writer unobserved. Non-Windows domain drops report
unsupported; compilation cannot substitute for native Windows execution.

Acceptance keeps original held-close, ownership, queue and fixed-snapshot tests,
and adds actual cross-thread Windows handle lifetime and unavailable-query
coverage. Earlier original failures remain failures. Actual hosted Windows
measurements are still required and cannot alone name the blocked backend call.
