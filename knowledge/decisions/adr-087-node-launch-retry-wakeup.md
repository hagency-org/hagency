---
kind: decision
id: ADR-087
title: Preserve the actual claim cutoff in Node launch retry scheduling
status: Accepted
---

## Context

Separate clock reads in claim and retry-wake selection can exclude a dispatch that becomes due between those reads, leaving no future wake.

## Decision

The original Node CI34556196694 at7cf0dc0 failed the wrapper-launch recovery
integration at its thirty-second Vitest deadline. Its log and uploaded JSON
report provide no inner phase or dispatch row; the historical cause is unknown.
The prior25c01ee Node run passed, and no Node runtime source changed between
those revisions. Neither fact establishes the failed run's cause.

Read-only tracing found a concrete race in the backend pump: claimDispatch
selects available_at at or before its clock reading, then nextQueuedDispatchAt
selects available_at strictly after a later reading. A retry becoming due in
that gap is absent from both results, leaving no future wake. A disposable
SQLite reproduction using the actual store and an injected clock confirmed
null claim at1800000000099, null wake at1800000000100, a queued retry due at
1800000000100 with its input intact, and successful explicit subsequent claim.
This proves the controlled code defect, not the uninstrumented hosted cause.
External original evidence and reproduction use the node-7cf0dc0 and
node-launch-retry-clock-repro prefixes under the2026-09-10 migration cache.

Add a combined claimDispatchWithWake operation that returns the ordinary claim
result and the next availability strictly after the actual eligibility cutoff
sampled inside that claim transaction. Keep existing standalone APIs and all
claim predicates, lease/capability mutations and transaction boundaries. A
caller never provides the authority clock. The pump schedules the returned
availability with its existing bounded timer conversion. Already-due blocked
rows remain excluded, avoiding an immediate retry loop when no state changed.

Deterministic tests must advance the clock across the gap, verify retained
input and subsequent claim, and distinguish blocked due rows from future work.
The original real-guardian recovery test retains its policy, assertions and
thirty-second timeout; only fixed-stage and bounded row evidence is added on
failure. Node scenarios run through exact Vitest selectors. Agent-spec parsing,
lint and explicit parsed boundary checks are separate evidence; its Cargo-only
lifecycle cannot prove Node tests and is not reported as passing.

The first complete local suite at5e7e31f caught an omitted ADR035 inventory
refresh. Read-only reconstruction found exactly397 changed leaves: the same
backend-v2.js hash in its source and helper records, and395 line/end_line offsets
each moving up one line. Classifications, counts, paths, methods and parity gates
were unchanged. Extend this contract to regenerate the reviewed inventory and
update the exact SSE installer assertion from7938 to7937. Preserve that original
suite's five failures separately: the inventory mismatch, one framework version
probe timeout and three fake Codex initialize timeouts. The unrelated subprocess
timeout causes are not established by the inventory correction.

## Consequences

The combined result preserves the actual claim cutoff without scheduling every blocked due row. Inventory offsets follow the real backend change, while historical CI and unrelated subprocess failures remain separately recorded.

## Alternatives Considered

Widening waits or waking for every blocked due row would hide the missing cutoff or introduce immediate spin. Calling the historical timeout a reproduced flake would exceed the evidence from the deterministic clock-crossing regression.
