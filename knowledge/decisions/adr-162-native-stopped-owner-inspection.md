---
kind: decision
id: ADR-162
title: Retain exact original stopped-owner workspace evidence before shutdown
status: Accepted
requirements: [REQ-RUST-MIGRATION-EXECUTION, REQ-LOCAL-THREE-RUNNER-QUALIFICATION, REQ-THREE-LAYER-COMPLETION]
---

The local Codex file failure exposed a missing recovery prerequisite: its real
whole-tree stop observation survived only in the execution Report. Shutdown
dropped that original owner; the remaining stop row cannot prove physical cleanup
or workspace inspection. Neither the generic orphan recovery endpoint nor a
formatted status record may invent the missing proof.

The original owned worker now records a bounded content inventory only after
actual full stop and an acknowledged negative fence. It uses the original
Started scope and retained root, not a reopened configured path as authority.
No-follow traversal, finite limits and two matching passes detect inconsistent
observations under the existing host-exclusive stable-path premise. Symlinks are
recorded but never followed. This does not claim hostile same-user isolation or
semantic safety of repeating effects outside the workspace.

Schema34 stores one immutable, host-only stop inspection per dispatch/fence.
The original capability and private Started marker authenticate the historical
attempt, including after its execution authority expired. Exact replay checks
the original committed digest; conflicting evidence refuses. Runtime output,
public status JSON and caller edits to a returned Report cannot mint a receipt.
An unknown recording response retains the same pending evidence for explicit
retry, not a fresh scan or new execution.

The receipt is evidence for a later explicit operator recovery flow. Recording
it never settles the stop or frees any lease, task input or quarantine; it does
not make G8 or full recovery complete. Existing failure and completion outcomes
remain unchanged. Old attempts without this original observation remain without
proof; no live database is patched to manufacture one.

## Verification

Actual offline owned subprocess EOF records a content-hash receipt, preserved on
reopen, while its original task remains InProgress and its lease/dirty/quarantine
remain held. Missing-process and oversized-workspace cases cannot mint a receipt
through mutated Report fields. Store tests cover pre-Started/foreign capability
refusal, conflicting content, historical replay, and real writer reply loss both
before and after commit. Filesystem tests cover exact hashes, outside symlinks,
unsupported sockets, root replacement and entry/byte/deadline bounds. Schema33
upgrade preserves existing resource configuration and does not create evidence
for old attempts. Full store352/execution83 pass; strict all-target Clippy passes.
This verifies evidence custody, not stopped-task resume or complete E2E.

ADR163 consumes the exact receipt only to distinguish stopped physical occupancy
from an unknown physical owner at claim time. It does not consume or alter the
receipt, settle the stop, clear the lease/quarantine or authorize a retry.

## Saying it, and being seen to wait (2026-09-21)

The recovery flow this ADR records was complete and silent. Live, one turn the
provider failed ended a soak: the dispatch was fenced with an unknown outcome,
the tree was proven stopped, the inspection was recorded and the worker waited
for the operator exactly as designed. The room saw an agent that stopped
answering, and the fleet reported it as lost, because any status carrying an
error counted as lost. The retained product does the same recovery out loud: it
posts "Result uncertain: the runner stopped after work may have started. Inspect
the workspace before retrying; this dispatch will not be run again
automatically." in the thread and keeps the agent up (`settleUnknownInternal`).

Recording the failure of a started run now queues that notice, in those words:
once per task, for a verified session, rooted at the request the dispatch was
answering. The attempt is its own savepoint, so recording a failure never fails
for a notice it could not address. It is queued there and not in the fence,
because a successful completion retires its dispatch through the same fence: the
first draft queued it in the fence and the two-agent fixtures caught finished
tasks being announced as uncertain. The agent's own driver posts it, best effort,
before it waits or gives up; ADR045 admits the ordinary task it belongs to. A
worker that waits carries `awaiting_operator` beside the failure it keeps
reporting, and the fleet counts an agent as lost only when it failed and is not
waiting.

No recovery authority changed: the operator's resolution, the inspection receipt
and the quarantine are as recorded above. Not ported in this slice: the retained
product also answers later requests in a quarantined session with "Waiting: …an
operator must inspect and resolve that outcome…"; here the waiting worker does
not take in new requests until it is resolved.


### Said after a restart too (2026-09-22)

The reopened repository settles every started or parked run as unknown
(`recover_all`), and the sweep does the same for an expired capability. Both
now queue the same thread notice the reported failure queues, best effort in
their own savepoint, once per task; the retained product's `reconcileOnStart`
goes through `settleUnknownInternal`, notice included. The agent that comes
back after the restart (ADR147 re-attach) posts it on its first turn. Pinned by
`native_outcome_unknown_is_said_in_the_thread_after_a_restart`.

### Waiting is said too (2026-09-22)

A request into a session whose previous run ended unknown now gets the
retained product's answer (`claimDispatch`): "Waiting: a previous runner in
this session stopped after work may have started. An operator must inspect and
resolve that outcome before another turn can run.", once per unresolved task,
rooted at the request, best effort in its own savepoint. Nothing else changes:
no dispatch is minted, the request stays unread, and the selection that follows
the operator's resolution takes it. Both the agent inbox and the delegated
intent selector say it. Pinned by
`native_request_into_a_quarantined_session_is_answered_with_waiting`.
