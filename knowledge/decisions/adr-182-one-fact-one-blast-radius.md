---
kind: decision
id: ADR-182
title: "One fact, one blast radius: the worker outlives its attempt and the process always exits"
status: Proposed
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [native, execution, fleet, custody, shutdown, containment]
---

## Context

The 2026-09-22 architectural review (gaps G1 and G2) and the 2026-09-23 ADR
consistency review (F1, F2, F9, F10, F16, F17 and Part 4's table) found that
the port keeps two failure models. The store's is the retained product's: a
failed attempt settles the dispatch `outcome_unknown`, quarantines the
session, dirties the workspace and posts the notice (ADR-053, ADR-162,
ADR-169). The host's is the port's own: the same fact ends the agent's worker
for good (`bootstrap/driver.rs`, ADR-036 "which ends the agent's worker",
deferred as "recovery work"), marks the fleet `outcome_unknown`, and — when
the guardian could not prove the tree gone — keeps an in-memory owner that
makes SIGTERM ineffective (ADR-096 "shutdown failure cannot consume the last
owner", ADR-127/133/135 "parks rather than exiting"). The in-memory owner is
weaker than what it replaces: `kill -9` clears it, re-attach ignores it, and
nothing durable says the agent was fenced (F16).

The retained product: a failed or unknown dispatch never touches the agent
(`router/src/store.ts` `settleUnknownInternal`; `backend-v2.js:2735-2756`
requeues or settles that dispatch and the pump continues). Only an
unconfirmed cleanup fences the agent, durably, on its record — `manualDown`,
`offlineReason: 'runner-cleanup-unconfirmed'`, `stopUnconfirmedDispatches`
(`backend-v2.js:2717-2733`) — the claim path refuses it while the fence stands
(`:1940` `agent_stopped`), the fence clears when a later close confirms
(`:2724`) or the operator starts the agent (`:2982`), and shutdown waits a
bounded receipt per runner (8 s, `router/src/runner.ts:368-377`) and exits
(`backend-v2.js:17386-17416`, `process.exit(0)` in `finally`).

The operator decided on 2026-09-22: reverse ADR-096's retained-owner rule;
follow the retained product's model (Part 4) where it has one.

## Decision

1. **A failed attempt ends its dispatch, not its worker.** When an attempt
   completes with a failure, an unsettled or a negative settlement, the
   driver records it (ADR-181), posts the queued notice, releases the
   workspace lease and continues to its next claim. The store's session
   quarantine is what keeps the failed session from being selected again;
   the agent's other sessions proceed. `awaiting_operator` stops being a
   blocking state: the worker no longer parks on `wait_for_resolution`; the
   operator's resolution simply makes the session claimable again. The
   store's occupancy rule is unchanged (ADR-163, the retained product's):
   a proven stop leaves the host's own inventory receipt (ADR-162), which
   frees the runner slot at once, so the agent's other sessions proceed
   without any operator act; only a stop without that receipt — an unproven
   one, the fenced case — keeps its slot charged.
2. **A refused handoff is that attempt's failure.** An error before an
   Operation exists (a lost account, a local provider directory gone, a
   claim the host cannot bind) records the failure with its category and
   site, and the worker continues after the retained product's flat launch
   backoff (5 s, `RUNNER_LAUNCH_RETRY_MS`); it does not end. Nothing
   started, so the dispatch goes back to the queue with the same backoff
   (`fail_before_start`, the attempt's outcome `spawn_failed`) instead of
   holding its lease for the full minute. For a factory agent the refused
   handoff consumed the warm child (the warm runtime is one-shot), so the
   factory keeps the follow-up root that child was started from and the
   next attempt is a follow-up launch — the same launch a restart uses
   (`Phase::Reattached`) — rather than a spent runtime that refuses every
   retry with `admission`, which is what the pin first found.
3. **Unproven cleanup fences the agent, durably.** When the report's custody
   is not proven stopped (`Cleanup::Unknown`, or `Observed` without all three
   facts), the driver writes an **agent fence**: `agent_fences(engagement_id,
   dispatch_id, fence, reason, created_at, cleared_at)` with `reason` one of
   `cleanup_unproven`, `cleanup_unknown`. While an uncleared fence exists for
   an engagement, `claim_owned_dispatch_for_host` and the intent and inbox
   selectors return no work for it, and the fleet status shows the agent as
   `fenced` with the dispatch. The fence is cleared by the operator's
   resolution of that dispatch (`recover-dispatch`,
   `resolve-stopped-dispatch`, `continue-stopped-dispatch`), which is the
   retained product's operator start; nothing automatic clears it, because
   the port's guardian reports once (ADR-029, F9). Re-attach after a restart
   reads and honours open fences.

   **How the operator reaches a fenced dispatch (decided 2026-09-23, option
   3 of the three offered).** Found while pinning this slice: an unproven
   stop fails its attempt (`cleanup_unknown`), which settles the dispatch
   `outcome_unknown` with an open `owned_runner_failure` stop row and — by
   ADR-162's own rule — no host inventory receipt; the stopped-dispatch
   inspection required that receipt and the orphan recovery refuses an open
   stop row, so no console route reached a fenced dispatch and the kernel's
   clearing was wired but unreachable. The retained product has no such
   gate (`beginOutcomeInspection` / `resolveOutcomeUnknown` take the
   operator's judgement as the evidence; `startAgent` clears `manualDown`,
   `backend-v2.js:2982`). Now: when an open fence names the attempt and no
   receipt exists, `stopped-dispatches/{d}/inspect` stands on the attempt's
   own ADR-181 `stop_reported` evidence (what the guardian reported and why
   it could not prove the tree gone), the snapshot says `fenced: true`, and
   the settle actions of `resolve-stopped-dispatch` (`accept_completed`,
   `keep_blocked`) are what clear the fence. `continue` stays refused: it
   needs the receipt, which is the proof that the workspace is free, and a
   fenced attempt cannot have it — a continuation of a fenced task is a new
   task after the settlement. `recover-dispatch` stays refused (open stop
   row). The owner-facing form of the same act — a card in the agent's DM
   through the approval bot, rendered by the Matrix client, with the verdict
   relayed to this store operation — belongs to the fleet/approval slice.

4. **The in-memory retained owner is dropped once the fence is written.**
   The fence row is the custody; the `OwnedSession` is released, which reaps
   the guardian and, through its process-group backstop, ends what the stop
   could not prove ended. This supersedes ADR-096's "one OS owner retains any
   unresolved report; shutdown failure cannot consume the last owner", and
   with it the park sentences of ADR-127/133/135 and the close-blocking of an
   unknown file job in ADR-101/098/089: an unknown outcome is recorded as
   unknown (its rows already are) and the close proceeds.
5. **Shutdown always completes.** SIGTERM stops admission, cancels every
   attempt, waits each attempt's bounded cleanup receipt (the existing stop
   budget), writes a fence for any tree still unproven, closes the writers
   and exits. `Driver::close` returns `Ok` once the fence is durable;
   `drain_agents` no longer fails on it; `serve` reaches `stop_graceful`;
   `main` never parks on `pending()`. The store's own shutdown verdicts
   (ADR-075/082/106/120) are unchanged and still reported; they no longer
   keep the process alive.
6. **Status keeps the last failure.** `begin_attempt` no longer wipes the
   previous attempt's failure words: the status carries `last_failure`
   (dispatch, `owned_failure`, `authority_site`/`cause`, `stop_cause`, the
   wall-clock millisecond) beside the live `state`, so an agent that failed
   and went on reads like one, not like one that never failed. `fenced` is a
   state word; `awaiting_operator` becomes the count of this agent's
   unresolved dispatches, informational only. `error` stays the agent-level
   word (what the fleet's `failed` and readiness read): a one-attempt
   driver's failed attempt still sets it, a continuous worker's does not —
   the worker went on, and `last_failure` is what says it failed.

Unchanged on purpose: the store's quarantine and dirty workspace
(ADR-053:182-184); operator-only resolution (ADR-148, 162, 164, 165, 170);
the completion proof (ADR-060: a held completion row under an unproven stop
is published only after the operator's resolution); the two-census cleanup
proof itself (ADR-029); Matrix fences and the pump's scope (the next slice,
ADR-047/174/064/137); fleet admission and readiness (the next slice).

## Consequences

Good, because one fact has one blast radius again, the retained product's: a
turn that fails takes a session out, not an agent; an unproven cleanup takes
an agent out durably, not a process; and the service can be stopped and
started by the operator without `kill -9`.

Good, because the fence survives what the retained owner did not: a restart,
a crash, a re-attach — and it is visible in the store and the status.

Bad, because dropping the owner after an unproven stop accepts the retained
product's residual: a descendant the guardian could not place may outlive
the attempt until the process-group backstop or the operator ends it. The
fence, the recorded refusal rows (ADR-181) and the operator's inspection are
the answer, as they are in the retained product.

Bad, because six records lose a sentence each; each amendment is appended in
place and named below, so the history reads forward.

Bad, because an unproven stop cannot be produced by an offline fixture (the
one real cause is a tree that outlives SIGKILL for the guardian's whole
budget), so the fence path is proven through a diagnostics-build pin in the
runtime session (`test-diagnostics`, `owned-mcp.unproven-stop`), the way
ADR-046's approval traces are; a production build compiles none of it.

## Amends and supersedes

- ADR-036 `:293-299`: superseded by decision 1.
- ADR-096 `:160-166`, `:199-212`: superseded by decisions 4 and 5.
- ADR-127 `:17-20`, `:41-43`; ADR-133; ADR-135 `:111-113`: the park on an
  unknown close is superseded by decision 5 (records, then exits).
- ADR-101 `:240-244`, `:306`; ADR-098 `:25`, `:34`; ADR-089 `:88-89`: an
  unknown file job no longer blocks the close (decision 4).
- ADR-162 `:74-76`, ADR-169 `:16-17`: every failed attempt leaves the worker
  up; `awaiting_operator` names quarantined work, whether or not the tree is
  proven (decisions 1, 6).
- ADR-175 `:33-37`, ADR-117 `:29-31`, ADR-180 `:131-134`: a handoff refusal,
  an unsettled file delivery, a refused delegation notice each end the
  attempt and leave the worker up (decisions 1, 2).
- ADR-130 `:111-113`, ADR-060 `:140-141`, ADR-040 `:109-119`: an unproven
  stop is a persisted agent fence, inspected, not retried; the held row
  waits under the fence (decisions 3, 4).
- ADR-147 `:636-638`: re-attach reads the fence (decision 3).
- ADR-104 `:58-59`: the Windows parked worker stays the one named exception
  while Windows is paused (ADR-136).

## Alternatives Considered

- *Keep `awaiting_operator` as a blocking wait and only add the fence.*
  Rejected: it keeps two models (a proven stop parks the worker, an unproven
  one ends it) and the retained product has neither.
- *An automatic re-observation of the tree before fencing.* Rejected for this
  slice: the guardian reports once; a second observer is a new mechanism and
  the retained product does without one.
- *Keep the retained owner and make only shutdown exit.* Rejected: the owner
  is exactly what makes the exit impossible, and it is not durable.
