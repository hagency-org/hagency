---
kind: decision
id: ADR-052
title: Scope redacted progress policy and coalescing to one immutable host run
status: Accepted
---

## Context

Progress filtering and coalescing need a finite per-run policy that keeps optional activity reporting separate from canonical task truth.

## Decision

`hagency-progress` ports the pure policy in
[progress-filter.js](../../lib/progress-filter.js), the attempt/throttle/count
behavior of [hagency-progress](../../bin/hagency-progress), and the ACP progress
collection/finish semantics in
[hagency-acp-agent.mjs](../../scripts/hagency-acp-agent.mjs). It does not replace
those files or install/run hooks. It is a separate pure crate, with no network,
filesystem, timers, runtime adapter, Matrix client or canonical task mutation.
This bounded M6 slice does not complete ADR026's editable, durable Matrix status.

The policy remains absent-by-default and malformed-closed. Absent/null config
uses start/step/done, every tool and a 60-second interval. Whole-object perGroup
replacement remains; unmentioned fields in that selected object receive the
built-in defaults, not unrelated top-level overrides. Tool includes/excludes,
trimmed lists, unknown event names, nullable defaults and the five-second minimum
interval retain the pure JavaScript behavior. Filter diagnostics are static
errors, not interpolated tool or group names in outgoing text. Future config IO
must distinguish missing from unreadable/malformed; it cannot turn an IO failure
into an absent/null document.

Only seven fixed verbs can enter `Counts`: read, searched, ran commands, edited,
wrote, fetched and worked. The type cannot hold an arbitrary string key. Tool
names are used only for allow/exclude comparison, then mapped to a fixed verb;
unknown tools remain generic. ACP think/other starts remain silent, known kinds
map to the same tool names, and updates do not add a new call start. Raw titles,
arguments, errors, paths and credentials never enter the summary. Observation
receipts retain bounded hashes; call tracking retains opaque call IDs and status,
not raw payloads or tool titles. No Debug/Serialize surface exposes those records.

The exported summary function preserves exact valid JavaScript wording and
first-observed verb order, including singular/plural failures and zero-delivery
wording. Counts reject zero additions and excessive totals. A runtime `done` or
Stop means only that the host observed that runtime finish. It cannot mark a
canonical task Done or prove successful execution. PostToolUse hook events remain
reported activity and cannot supply independent execution success/failure proof.
ACP calls count in fixed work totals only after a completed status is observed.
Failed status contributes one failure; unconfirmed attempts instead show as
"N attempts pending" while active and "N attempts unresolved" after finish.
Mixed unresolved/failed outcomes never claim that nothing succeeded. A completed
observation arriving after a pending notice was claimed or accepted remains
eligible for a new snapshot, while the lifetime call count remains one.

[ACP v1 tool calls](https://agentclientprotocol.com/protocol/v1/tool-calls)
defines the initial status as the current ToolCallStatus, defaulting to pending;
completed and failed are valid initial states too. Both initial and update
notifications therefore establish terminal state. Missing, future or malformed
status cannot establish completion; case-insensitive string failed retains the
existing helper's compatibility behavior. Later contradictory terminal evidence
is an error rather than a state reversal. Runtime status remains a bound upstream
observation, never native execution, canonical task or Matrix delivery authority.

`Accumulator` owns one opaque, immutable host-created `RunId`. The eventual host
must bind it to its exact current dispatch, Agent incarnation, capability,
connection and upstream thread/turn; this crate cannot establish that authority
by inspecting runtime data. RunId, attempts and delivery evidence have no
Deserialize implementation. Runtime payloads contain no accepted room/thread
routing setters. Their extra fields are merely part of a content receipt.

There is no in-place reset, import or serialized snapshot. A new run requires a
new host identity and a fresh accumulator. Retirement rejects later observations
and claims and never retargets old context. The caller must not recreate an old
RunId after losing state; persistent recovery/host ownership is still an
integration gate. Multiple accumulators are independently owned by the future
host, never keyed through a global mutable Agent anchor file.

Every observation uses an exact run and contiguous positive host sequence. A
bounded canonical JSON hash includes the transport kind. Identical replay returns
Duplicate without modifying the clock or counters; changed replay, a gap,
foreign run or new input after finish is rejected. New observations and attempts
use monotonic host milliseconds. Time reversal and capacity failures leave state
unchanged; successful quiet polls advance the clock. Host times are scheduling
observations, not authenticated timestamps from model input. A repeated ACP start
for one call must have identical content and never increments twice. Contradictory
terminal statuses and updates before a known call are rejected. No ignored error
may be reinterpreted as a successful tool observation by the future adapter.

Counts are retained as bounded scoped history with an accepted-window watermark.
`claim` returns one redacted text emission and a private attempt token, and opens
the throttle immediately. The first transition can emit immediately; ordinary
steps wait at least the selected interval. Finish can bypass a preceding step
window once, while failed finish retries remain throttled. There is at most one
outstanding attempt. `ObservedAccepted` clears only the frozen snapshot watermark,
leaving newer work/failures intact; this is host evidence that the progress
submission was accepted, not that a Matrix recipient saw it. ObservedNotAccepted
retains counts for a later bounded attempt. Unknown retains the outstanding token
and blocks further claims until explicit host inspection settles it. Recovery
can read that token without obtaining another send. Retirement makes any pending
attempt uncertain; later settlement may preserve historical acceptance but cannot
reactivate the retired run. This in-memory projection is not a durable outbox.

Answer delivery is a separate typed host observation supplied when finishing:
`AnswerDelivery` retains an exact count plus either FinalReplyJournalInspection
or MatrixTimelineInspection. These names require actual complete-run inspection
by the host; merely writing bytes, receiving model text, counting tools or
accepting a progress line cannot construct that evidence. None means unknown.
Only an explicitly inspected zero can produce "but sent nothing" after activity.
The crate neither performs that inspection nor certifies Matrix authenticity.

The retained source has several defects that are deliberately **not** reproduced.
`native/fixtures/progress.json` keeps these correction cases separate from exact
parity cases, with measured legacy/native outcomes and rationale:

- A malformed selected perGroup value may fall back to a broader policy. Native
  rejects it. Inherited JavaScript object properties cannot create a group rule.
- Inherited properties of the verb object are not fixed verbs. Native maps them
  to worked; non-string ACP failure status is not evidence of failure.
- ACP failed updates previously counted before tool exclusion and counted every
  repeated failure, leaving the call's initial activity counted too. Native
  applies the same filter, deduplicates status and removes the failed activity.
- Initial pending/unknown calls previously counted as completed-looking work and
  initial failed status was ignored. Native shows unresolved attempts separately,
  accepts initial terminal evidence and applies exclusions to every state.
- The ACP finish path bypassed event filters. Native does not emit a disabled
  finish. Finish summaries use lifetime counts even after earlier progress was
  accepted, rather than only the last unflushed window.
- Possible-send uncertainty blocks automatic retry; no optimistic reset occurs
  merely because a timer expired. Inputs after retirement cannot adopt an old
  anchor or be remapped into a fresh run.

Bounds are explicit: 64 KiB config JSON, depth8, 128 entries per filter list,
256 bytes per tool/group/run/call identifier, 16 KiB observation JSON, 1024
observations, 256 call identities and 256 attempts per accumulator. Fixed counts
allow at most one million total additions. Full capacities return errors rather
than evicting replay receipts or silently dropping activity. Failures of this
optional projection must remain observable to the host without breaking the
actual Agent task. There is no unbounded background worker or hidden retry.

The reproducible JavaScript oracle records all three source hashes, executes
pure policy functions and the actual CLI/ACP emitter bodies with explicit fake
IO, environment, clock, timers and fetch, and never contacts a service. It holds
275 unchanged cases and 20 explicit corrections. Native CI runs its check mode.
Remaining work is current-runtime attachment, persistent crash recovery, host
config/proof qualification, one editable status per dispatch, route/privacy/E2EE
handling, approval/settlement-derived status and actual Matrix delivery. None is
claimed by the offline policy/accumulator tests.

## Consequences

The pure accumulator retains explicit bounds and replay identities, with observable refusal at capacity. Durable editable Matrix status and actual runtime attachment remain later integration work.

## Alternatives Considered

Inferring Done from tool activity or silently evicting progress receipts would change the documented semantics. Hidden timers or retry workers would introduce IO and lifetime behavior outside this pure policy boundary.
