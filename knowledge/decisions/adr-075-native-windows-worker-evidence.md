---
kind: decision
id: ADR-075
title: "Separate custody lease expiry from concurrent claims and retain shutdown evidence"
status: Accepted
tags: [rust, windows, fixtures, custody]
---

## Context

The original Windows failures mixed a short lease's expiry with concurrency assertions and provided no phase evidence for an unrelated shutdown error.

## Decision

Original Windows job103108377728 at a856aa5 failed two store library tests:
queued completion cancellation passed its behavior assertions but shutdown returned
OutcomeUnknown; the outbound concurrency fixture returned two claims rather than
one. Matrix outgoing and transport tests passed in the original run and both later
diagnostic runs. The failed full-suite result remains failed.

The concurrency fixture requested a 100ms lease. The writer adds actual elapsed
submission time and the repository expires unstarted claims before considering
new attempts. Two sequential claims separated by expiry are valid and do not mean
two simultaneous execution permissions. Use the existing allowed120s lease for
this concurrency-only fixture, assert completion within that lease, and retain the
exact one-claim assertion. A separate real-writer regression injects elapsed host
submission time, reclaims an expired100ms attempt and proves the old ticket and
repeated Start cannot execute. No production timeout or lease limit changes.
The original run did not record its execution times, so its historical timing
cannot be established solely from the count. The new regression demonstrates the
fixture's invalid assumption with a controlled actual writer.

The shutdown cause is unproven. Use the existing fixed phase snapshot on the
original shutdown call, print it only upon error, and preserve both two-second
waits and the failing verdict. Do not retry shutdown or count later closure as the
original success. Subsequent Windows CI remains qualification, not local GNU
cross-compilation or an excuse to widen deadlines.

## Consequences

Separate fixtures preserve one-claim concurrency and real expired-ticket denial without changing production leases. Original shutdown uncertainty remains failed evidence pending an observed phase.

## Alternatives Considered

Calling sequential claims after lease expiry simultaneous execution would misstate the observed authority. Retrying shutdown or widening production waits would replace the original failure instead of explaining it.
