---
kind: decision
id: ADR-088
title: "Observe original Windows Matrix failure phases without changing outcomes"
status: Accepted
requirements: [REQ-RUST-MIGRATION-EXECUTION]
---

## Context

Original Windows Matrix failures lacked enough bounded phase evidence to distinguish domain shutdown waits from early collector or attachment failures.

## Decision

Original Windows7cf0dc0 printed475 passing tests and six Matrix library failures;
that library reported70/76 passing. Four errors were DomainStore shutdown after
successful SDK/Collector close: approval_intake/mod.rs274 and intake/mod.rs307,
495,1063. They were not SDK-close failures. Existing shutdown returns OutcomeUnknown
for either its enqueue or reply timeout; the original log contains no phase
snapshot distinguishing those waits or repository Drop. ADR075/082 already provide
DomainStore::shutdown_observed without altering queue, waits, Result or release
ordering. Use it at these original fixture sites and print its existing fixed
snapshot only on failure. No second shutdown or later success repairs that result.

Attachment manifest bounds failed at attachments.rs275 in one of two64-event
batches. Its joined whoami/sync/state script completed, which places this actual
failure after intake_start returned and the final room-state request was issued.
The original error does not identify the batch index or later room/domain/SDK
handoff operation. Add bounded batch index and optional fixed HTTP-request phase
observations around the same existing fixture. Script completion proves only
that responses were supplied; it does not prove remote body validation or the
backend stage that later failed. No raw responses, event values, private routes
or journal contents are printed.

The old-inspector test failed at shared Fake::next's existing10.9s wait. Its prime
helper still joined collection and the full HTTP script, hiding an early collector
error until an impossible next request timed out. The later inspection script
already uses common::scripted. Change the original prime pair to that existing
script-first driver, retain its three requests and session assertions, and add
fixed optional HTTP phase labels. The original log cannot prove which of these
sites timed out; this is diagnostic visibility, not its historical root cause.
Real wrong-device local HTTP bootstrap and observed-intake regressions prove early
Identity propagation without artificial clocks or changed deadlines. The explicitly
observed manifest path also uses common::scripted so its own early error cannot
be masked by a later missing request. Its last fixed HTTP phase is printed on
error; the unobserved run helper retains its existing join semantics.

All changes are in fixture files. Every production/SDK/HTTP/shutdown/fake deadline,
CI parallelism setting and original Result remains unchanged. The new helper never
retries shutdown, collection or intake. Original Windows failure remains failed;
its later serial19 outgoing and22 transport test passes were different selectors
and do not explain these six failures. Local fixture passes or Windows GNU checks
cannot establish native Windows timing or repair a prior failed suite. A future
original instrumented Windows run is needed before any production fix is justified.

## Consequences

Fixture observations preserve original results, fixed phases and every deadline. Instrumentation and later serial passes do not establish a production defect or repair the historical failed suite.

## Alternatives Considered

Retrying shutdown or delaying shared clocks would replace the original operation being observed. Inferring the cause from different passing diagnostic selectors would conflate separate evidence.
