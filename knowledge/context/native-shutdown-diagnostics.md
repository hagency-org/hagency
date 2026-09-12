# Native domain shutdown evidence

## Observed failure, 2026-09-10

Windows native run 34533485134, job 103059541682, at `79b036c` failed three MCP
coordination tests at the fixture's `domain.shutdown().await.unwrap()` and one
runner HTTP test at the same shutdown call. The original error was
`OutcomeUnknown`. The complete unmodified log is preserved externally as
`ci-79b036c-windows.log`; do not replace it with a later passing result.

The MCP file reported 3 passed / 3 failed in 20.28 seconds; runner HTTP reported
9 passed / 1 failed in 22.19 seconds. Grouped panic output and test-file duration
are not individual shutdown timings. Default Matrix (9 tests) and Palpo (13)
transport suites passed, as did the failure-only serial transport diagnostic;
those runs do not explain or close these independent teardown failures.

Source inspection establishes that the existing writer first queues Shutdown,
then destroys its owned DomainRepository, then sends an acknowledgement. Each
caller wait is two seconds. The repository has a SQLite Connection and an
ownership File without its own Drop implementation. Pinned rusqlite 0.37 calls
SQLite close during destruction, which can perform synchronous IO. Neither the
historical log nor source proves whether queue delay, that destruction, or
scheduling before acknowledgement caused the observed deadline expiration.
No historical disk contention, pending mutation, rollback or closure is inferred.

## Minimal observation contract

`DomainStore::shutdown_observed` returns the original `Result<(), Error>` and a
fixed-size `ShutdownSnapshot`. Ordinary `shutdown` uses the same internal path
without allocating a probe or reading diagnostic time. The queue, two-second
enqueue wait, two-second reply wait, Drop-before-ack order and existing unknown
or unavailable verdicts are unchanged. No retries, early acknowledgements,
deadline widening or control-plane endpoints are added.

One optional per-job Arc owns eight timestamp slots and their atomic publication
bits. Time is relative monotonic microseconds; conversion saturates only beyond
u64 microseconds. There are no identifiers, paths, SQL, errors, credentials,
caller strings, serialization, global state or production callbacks. An elapsed
zero is `Some(0)`, distinct from an unobserved `None`. Each phase's timestamp is
stored before its Release publication; a snapshot Acquire-loads completed bits
before reading their timestamps. Each phase has exactly one publisher.

Worker phases and caller phases have a partial order. The worker can pick up and
destroy the repository before the caller observes enqueue completion. A caller
can receive a successful acknowledgement before the worker publishes its
post-send marker. Therefore no single last-phase enum overwrites progress, and
missing `acknowledgement_sent_us` does not contradict an original successful
Result. Acknowledgement-start distinguishes that final synchronous send boundary
from repository destruction. The verdict distinguishes enqueue/reply timeout or
closed channel independently of these observations.

At caller timeout a queued job or destructor may still own the repository. Its
probe continues to live with the worker even though the returned snapshot is
fixed. Missing phases mean unobserved, never rollback or safe retry. Drop-finished
only means that the repository destructor returned, not a new integrity, flush
or OS-thread-exit proof. No diagnostic observation authorizes resource release.

## Verification scope

Four deterministic selectors cover actual ordinary and observed repository
ownership release, enqueue timeout versus accepted-but-unpicked reply timeout,
controlled pauses at the recorded Drop-start and acknowledgement-start boundaries,
and cross-thread phase publication with independent scopes, partial order and
zero-duration representation. Test-only gates are bounded and release on sender
drop; they are not part of production and do not claim SQLite itself was stalled.
The original timeout result remains an asserted error after cleanup of its exact
accepted job; shutdown is not retried to turn that result into success.

MCP and runner HTTP fixture teardown now logs only the original error plus the
static snapshot on failure and still panics. Successful teardown stays silent.
No network, services, runtime authority or live deployments are changed. Focused
local validation is not Windows qualification or proof of the historical cause;
the integrated CI run must supply that platform evidence. Full workspace testing
is delegated to integration to avoid another broad build in this isolated tree.
