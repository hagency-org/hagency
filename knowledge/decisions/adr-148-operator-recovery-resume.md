---
kind: decision
id: ADR-148
title: "Operator recovery and resume of an orphaned dispatch"
status: Decided
requirements: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREE-LAYER-COMPLETION]
tags: [native, dispatch, recovery, operator, console, custody]
---

## Context

The 2026-09-14/15 audit re-run established that a crashed host's dispatch **is**
reconciled in production: the driver's claim (`bootstrap/driver.rs:257`) →
`claim_clock` → `expire` (`domain/execution.rs:810`) → `lose` (`:186-214`)
settles it to `outcome_unknown`, quarantines the session, and the candidate query
(`:812`) never re-claims it. That satisfies the definition-of-done clause about
surviving crashes without duplicate effects. But nothing in production performs
what `recover_dispatch` (`domain/execution.rs:1025`) does: `DELETE FROM
resource_leases` (`:1103`), clear `quarantined=0` (`:1107`), clear `dirty=0`
(`:1110`), supersede the queued rows (`:1112`), enqueue the replacement (`:1115`),
and write the recovery records (`:1118`, `:1120`). `reconcile_dispatches`
(`:1014`) is likewise an unreached facade — it only calls `expire`, never
`recover_dispatch`.

Consequence: an orphaned dispatch is settled but never resumed — it holds its
resource lease, keeps counting against the live-dispatch cap, and leaves its
session quarantined permanently, with no operator path back. The question this ADR
decides before any code is written is **who triggers recovery and under what
authority** — the question a builder answered on its own last time and had to
have unwound.

## Decision

### 1. What the retained product does

**The retained product never resumes in-flight dispatch work; it only
re-registers orphaned agent *homes* at startup.** Its orphan handling is a
startup reconciliation loop (`backend-v2.js:3049-3095`, comment "Startup
reconciliation: orphaned agent homes") that walks agent home directories and, for
each manifest with no live record, re-registers the agent record with
`offlineReason: 'orphan-discovered'` (`:3072`). It marks the agent offline; it
does not re-run, re-enqueue, or resume any in-flight task, dispatch or turn.
There is no `recover_dispatch`-equivalent in the retained source — no handler that
takes a settled/orphaned run and re-enqueues its work.

So the native product is **deciding fresh**: there is no retained resume semantic
to adopt, only the retained caution that a crashed agent is left offline for a
human to inspect, never auto-restarted into new work.

### 2. Who triggers it natively, and with what authority

**An operator console route under the existing `Scope::AgentLifecycle`, never an
automatic path.**

`recover_dispatch` is already documented as the operator-only inspection command:
"Operator-only inspection command. The real process adapter must first prove the
old process stopped and inspect every named workspace. Never runner-callable"
(`domain/execution.rs:1023-1025`). The trigger that matches that authority is the
console's agent-lifecycle surface: it is the console's scope for direct store
mutations — agent `start`/`stop`/`preset` ride it today
(`native/hagency/src/console/agents.rs:27-32`, gated by `can_lifecycle`,
`native/hagency/src/console/authority.rs:333`), and the same scope already gates
the bounded grant-revocation mutation. A new route `POST
/console/api/agents/{id}/recover-dispatch` (or its dispatch-addressed analogue)
under `Scope::AgentLifecycle` is the smallest authority-consistent trigger.

**Why the automatic path loses.** An automatic resume — at claim time, or a sweep
like the ceiling/retention sweeps wired at bootstrap — would re-enqueue work a
crashed host may have *already partly performed*. That is a correctness question,
not a convenience one. ADR-053 is explicit that uncertainty must be preserved,
not laundered into a retry: a command that may have committed "remains
`OutcomeUnknown` and 'reconcile before retrying' stands unchanged … The new code
is a diagnosis for the operator and grants no retry, reply, lease or completion
authority" (`adr-053-native-owned-dispatch.md:145-146`). Re-enqueue-on-sight is
exactly the uninspected duplicate-effect the DoD's "no duplicate effects" clause
exists to forbid. Only an operator who has inspected the workspace and proven the
old owner stopped may certify that resuming is safe — which is precisely the
evidence `recover_dispatch` demands (`:1023-1025`, section 3). A sweep cannot
hold that evidence, so a sweep may not trigger recovery. This also matches the
retained product's posture (section 1): a crashed agent is left for a human.

A CLI verb would carry equivalent authority to the console route but adds a
second operator surface for the same act; the console is where the operator
already sees the quarantined agent and acts on its lifecycle, so the console is
the single trigger.

### 3. What evidence recovery requires before it may run

`recover_dispatch` takes an `evidence: &str` argument and writes it into the
recovery record (`domain/execution.rs:1118`). Before an orphan may be resumed
rather than left settled, three things must be true and observed, per the
existing custody and completion rules:

- **The old owner is proven stopped.** ADR-060 requires the host to retain and
  stop the specific owner and to observe all three facts —
  `whole_tree_stopped`, `leader_exited`, and `signals_accepted`
  (`adr-060-native-owned-completion.md:99-101`) — before any publication or
  release. The same proof gates recovery: the operator/adapter must prove the
  orphaned leader and its whole tree exited, not merely that the lease expired.
  ADR-053's settlement fencing makes the same demand — "Successful dispatch
  settlement requires Completed plus a retained stop report proving
  whole_tree_stopped, leader_exited and signals_accepted"
  (`adr-053-native-owned-dispatch.md:147-149`).
- **The workspace is inspected, not assumed.** ADR-095's crash-recovery column is
  explicit: "uncertain effects require inspection"
  (`adr-095-native-state-ownership.md:26`), and "Inspect ambiguous
  account/process creation; do not launch again on timeout" (`:25`). The operator
  inspects every named workspace the orphan held before clearing its `dirty` flag
  (`domain/execution.rs:1110`) — the dirty bit exists precisely to block
  resumption into an unverified workspace.
- **The scope is proven clean and current.** Recovery clears `quarantined=0`
  (`:1107`) and deletes the orphan's leases (`:1103`) only after the adapter has
  confirmed no sibling still holds the resource and the dispatch's scope is still
  the current one — the same quarantine/lease fences ADR-060 keeps set through
  publication (`adr-060-native-owned-completion.md:110-115`).

The `evidence` string records which of these the operator observed; an empty or
unsubstantiated evidence is not a recovery.

### 4. The consequences the tests must observe

- **Recoverable path.** After a legitimate `recover_dispatch`: the orphan's
  `resource_leases` rows are deleted (`:1103`); the session's `quarantined` is
  cleared (`:1107`); the workspace's `dirty` is cleared (`:1110`); the orphan's
  queued rows are superseded (`:1112`); the replacement is enqueued (`:1115`);
  and the two recovery records are written with the evidence (`:1118`, `:1120`).
  The replacement — not the orphan — is what a later claim picks up.
- **Named refusals.** A recovery attempted without the stopped-owner/workspace
  proof is refused before any row changes; a recovery against a dispatch that is
  not in the settled-orphan state is refused; a runner-initiated recovery is
  refused outright (the command is operator-only, `:1023-1025`).
- **Non-recoverable dispatch stays settled.** A dispatch whose workspace cannot
  be proven clean, or whose owner cannot be proven stopped, must remain
  `outcome_unknown`, session quarantined, lease held, and must never be
  re-claimed or re-enqueued — the candidate query (`:812`) continues to exclude
  it, and no sweep may resurrect it.

### 5. Placement

**A new ADR is right; this is it.** No existing ADR owns the *decision* of who
resumes an orphaned dispatch. ADR-146 names the gap but explicitly enumerates the
unwired flows as a gap register (it says "a crashed dispatch is never reconciled"
among the G1–G9 gaps, `adr-146-production-callers-and-store-surface.md:14-16`)
without deciding the trigger or authority. ADR-053 owns dispatch custody and
*forbids* uninspected resume (`:145-146`) but does not decide who may perform an
inspected one. ADR-060 owns completion/stop proof, ADR-095 owns state-ownership
recovery boundaries; neither owns the operator-resume decision. This ADR decides
it and points back: the trigger is ADR-148, the custody rules it must satisfy are
ADR-053/060/095.

## Alternatives Considered

- *Automatic resume at claim time or via a retention-style sweep.* Rejected: it
  re-enqueues work a crashed host may have partly performed, laundering
  uncertainty into a duplicate effect — the exact outcome ADR-053:145-146 forbids
  and the DoD's "no duplicate effects" clause exists to prevent. A sweep cannot
  hold the stopped-owner/workspace evidence recovery requires.
- *A CLI verb as the trigger.* Rejected as the single surface: equivalent
  authority to the console but a second operator surface for one act; the console
  is where the operator already sees and acts on the quarantined agent.
- *Leave orphans settled forever (status quo).* Rejected: the orphan holds its
  lease, counts against the live-dispatch cap and keeps its session quarantined
  permanently — a resource leak with no operator path back, which is the gap that
  motivated G5a.
