---
kind: context
id: CTX-NATIVE-OWNED-CONTROL-PUMP
title: "Original session custody through bounded private approval control"
status: Accepted
---

The bounded task is `specs/task-rust-owned-control-pump.spec.md`, based on
b856b47. The runtime implementation follows accepted ADR034/036/040/070 mechanics;
domain approval authorization/transmission/application semantics remain separate.

The low-level operation borrows a pinned host future and selects only cancel-safe
single reads. Successful control returns preserve the original decoder, partial
frame timer, streams, callback reservations and opaque observation order. Started
future cancellation keeps the existing typed failure and owned stop guards.
There is no extra task, channel, writer, reconstructed session or keepalive event.

Policy is explicit and finite: owner_wait_ms > 0; response_reserve_ms at least the
existing write timeout; total <= 1,200,000ms and within the original lifetime.
A callback arriving too late is refused even if initial opt-in fitted. Each of at
most 16 callbacks keeps its original admission-based owner/response deadlines.
Other callbacks and control wakes cannot renew it. A callback must be prepared
before its owner bound; only then does its fixed response reserve cover awaiting
response-begin and writing. An unprepared sibling retains its owner bound.
Ordinary event operations
retain one deadline across control results; real typed read completion ends that
operation. Prepared-send update returns retain the original read/write clocks.

Preparation reserves and serializes the exact callback synchronously. The
non-cloneable PreparedApproval binds the original driver Arc identity and exact
numeric/string request ID; no foreign driver or reconstructed request can rearm
it. Before first byte, all already received and currently ready stdout is parsed,
including an existing partial frame. Each new typed observation is returned to
the host. The same original frame can continue only under its original fixed
response/write deadlines and after the host rechecks its admitted durable grant.
After any accepted byte it must finish and flush or fail with uncertain progress;
there is no partial-write continuation API after a dropped/failed operation.

A host grant batch must be retained separately from its pending map and original
prepared frames. Pinning a begin future that borrows only batch.grants permits
processing new callbacks/usage against the retained session and map. Positive
begin output is not itself a runtime observation. New barriers require their own
authorization and rechecking earlier admitted grants, without rearming begin.
This library cannot supply those domain checks. Actual native permission
application stays independently unconfirmed after write acceptance or resolution.

The runtime and execution host integration gates remain distinct. This opt-in
changes neither the existing 30s host operation nor its 2s response policy, and
it does not admit a longer launch, release a lease, apply a verdict, or enable a
live model. Local duplex and real offline owned-child results are mechanics
qualification only; Windows execution and full owned interactive workflow remain
separate qualification.

Final local qualification: all 56 runtime package tests pass (5 library, 9 codec,
8 owned, 26 session, 8 transport), Clippy with warnings denied passes, and strict
lifecycle passes all six bounded checks. The five bound selectors run six actual
tests because the original-deadline selector includes the independent reserve
regression. The lifecycle's wider requirement-trace diagnostic still lists other
mapped project scenarios lacking results in this task; bounded success is not
full-project trace closure. Windows GNU all-target compilation passes without
executing Windows code. The original local oversized async-fixture stack failure
remains in the cache; splitting independent cases corrected fixture storage only.

Domain approval expiry is independently authoritative. Runtime response reserve
cannot revive an expired/revoked domain decision. The future host must align its
owner cutoff, original grant expiry and immutable response deadline at admission,
with the response margin contained in the authorized lifetime. The duplex
positive-control-after-owner fixture proves runtime scheduling mechanics only;
it supplies no independent database grant or native application qualification.
