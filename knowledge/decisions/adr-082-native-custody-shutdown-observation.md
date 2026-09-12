---
kind: decision
id: ADR-082
title: "Observe one custody worker shutdown without changing its result"
status: Accepted
requirements: [REQ-RUST-MIGRATION-EXECUTION]
---

## Context

A failed custody-worker shutdown needs phase observations from its original call, independently of later closure and unrelated domain-store diagnostics.

## Decision

At25c01ee the original Windows Palpo transport target passed13/13. A later
failure-only serial diagnostic passed12/13 and failed1/13: native_outbound_http_publication_
frozen_restart_and_rotation panicked at transport.rs620 with OutcomeUnknown.
The call was the first shutdown before reopen, after the expected publication
Transport error and subsequent freeze Conflict. No phase evidence was recorded.
The original log remains windows-25c01ee-original.log in the external migration
cache; the read-only audit is palpo-25c01ee-shutdown-review.md. Neither the later
failure nor a new success explains the historical cause or replaces another
run's verdict.

This fixture uses Store/Repository/custody.sqlite3, not DomainStore. Its original
shutdown uses a bounded queue and two two-second waits, returning OutcomeUnknown
for either timeout. The worker drops Repository before sending acknowledgement.
SQLite connection drop/final checkpoint retains the owner lock until completed.
These operations and ordering remain unchanged.

Add Store::shutdown_observed with the same optional per-job fixed Probe used by
DomainStore. Ordinary shutdown uses None and allocates no probe. Only observed
calls record relative monotonic microsecond timestamps for enqueue start and
observation, worker pickup, repository Drop start/finish, acknowledgement
start/sent, and caller finish. The original Result is returned alongside an
existing ShutdownSnapshot. No payload, identity, token, path, variable-size
message, new retry or runtime authority enters the probe.

The existing timestamp-before-Release-bit implementation is reused unchanged.
Independent caller/worker phases permit pickup before the caller observes
successful enqueue, or receipt before the sender publishes acknowledgement-sent.
Missing timestamps mean unobserved, not rollback, release or no execution. A
completed result is still the original acknowledgement, not proof of OS-thread
exit. An already timed-out caller never becomes successful after late cleanup.

The Palpo publication-before-reopen call site uses the observed form. Success
continues silently through the original reopen/replay/generation assertions.
Failure still panics, adding only a static stage label, the unchanged shutdown
error and fixed snapshot. The remaining fixture waits, subsequent shutdown,
network assertions, production deadlines and workflow result handling stay as
before. This is measurement for a future incident, not a speculative timeout fix.

Actual worker tests cover ordinary/observed successful release, a worker paused
before shutdown pickup, Drop and acknowledgement boundaries, and an unconsumed
full queue. They await the real unchanged timeout and use explicit private gates,
not timing-sensitive arbitrary sleeps. Resume only lets the original owner finish;
it is never a second shutdown call that converts failure to success. A retained
lock is tested before Drop and real ownership reacquisition after release.

Focused Cargo execution covers store phase/custody tests and the actual local
Palpo HTTP publication fixture. Complete strict cross-crate lifecycle must run
once after parent integration using the populated workspace target; crate-only
lifecycle would not discover both stores' and Palpo's selectors and must not be
reported as complete. Native/Windows GNU compilation is separate from actual
hosted Windows runtime qualification. No live server, service or credential is
accessed.

## Consequences

Fixed snapshots preserve the original result, queue and acknowledgement waits. Local phase tests and cross-compilation remain separate from actual hosted Windows runtime evidence.

## Alternatives Considered

Issuing a second shutdown or treating eventual lock release as the first call's success would erase uncertainty. Reusing domain-store evidence for a custody-store failure would observe the wrong owner.
