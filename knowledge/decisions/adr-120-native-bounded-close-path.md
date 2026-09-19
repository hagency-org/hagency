---
kind: decision
id: ADR-120
title: Bounded close path for the private SQLite stores
status: Accepted
---

## Context

Hosted Windows stalls the native domain shutdown inside `sqlite3_close`.
Every failing `ShutdownSnapshot` shows `sqlite_close_entered_us` published,
nothing after it, and zero writer CPU across the two-second reply budget, so
the writer waits rather than computes. A whole-package probe reproduced the
stall in one of four iterations with four stalls on four threads of one
process; the same selectors pass in isolation. At the moment of timeout the
domain `-wal` held 91 to 101 un-checkpointed frames and the `-shm` was still
present. SQLite unlinks `-shm` before `-wal` at close, so the stall is at or
before the SHM unlink: inside the close-time checkpoint, its sync, the
refused-delete retry loop of `winDelete`, or the process-global Windows SHM
mutex that serializes every close in the process. No single blocked call is
proven; all of them live in the optional close-time checkpoint and unlink.

The store opens with `journal_mode = WAL`, `synchronous = FULL`, a 100 ms busy
timeout and nothing else. Committed transactions are made durable by
`synchronous = FULL` at commit. The close-time checkpoint only bounds WAL
replay cost on the next open, and the open path already admits leftover
`-wal`, `-shm` and `-journal` files.

## Decision

Both private stores set `SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE` on their
connection immediately after open, through rusqlite's safe configuration API.
On close SQLite takes no EXCLUSIVE lock, runs no PASSIVE checkpoint, purges
no `-shm` and unlinks no `-wal`; the auxiliary files remain and the next open
replays them. The two-second enqueue and reply waits, every phase meaning, the
`ShutdownOutcome` values, the connection-before-ownership destruction order
and the `OutcomeUnknown` verdict are unchanged. A close that does not finish
is still reported exactly as it is today, and missing phases remain
unobserved rather than reinterpreted.

This is not a durability claim. The change removes optional work from the
close path; it widens no bound, adds no retry and introduces no fallback.

## Consequences

A clean close and a crash now leave the same on-disk shape, a database plus
a WAL to replay, so recovery is the ordinary open path rather than an
exception. An integration test proves that a completed close leaves a
non-empty WAL and SHM behind and that the reopened store reports the same
canonical state. Hosted Windows whole-package probes are the reproduction
vehicle; if the stall survives with the checkpoint and both unlinks removed,
the remaining candidate is the SHM unmap and its process-global mutex, which
must be separately evidenced before any further change.

## Alternatives Considered

`SQLITE_FCNTL_PERSIST_WAL` also removes both unlinks but keeps the checkpoint
and is reachable only through unsafe file-control FFI, which the crate's
deny-unsafe-code policy excludes. A longer reply budget would trade a truthful
unknown for a slower truthful unknown with the mechanism intact. Retrying the
close, serializing the test binary or ignoring the tests would each hide an
honest failure.
