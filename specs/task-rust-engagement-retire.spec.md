spec: task
name: "Retire an active engagement and retry a failed retirement through the console"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, engagement, lifecycle, console, parity]
---

## Intent

Give the engagement lifecycle its retirement arm, and its operator-driven
cleanup retry. The native store already models both — `DomainRepository::revoke`
(`native/hagency-store/src/domain.rs:1260`) and
`DomainRepository::retry_cleanup` (`:1265`) — but no production code calls
either. `revoke` is a two-line wrapper over `end(command_id, id, true)`
(`:1285-1334`), the sole writer of `EngagementState::Revoked` (`:1316`);
`retry_cleanup` is the **only** way a failed retirement is ever retried
(`:1274-1280`). An engagement that is live therefore cannot be decommissioned in
the Rust port, and one whose retirement failed can never be un-stuck.

The retained product retires through `POST /api/engagements/:id/revoke`
(`backend-v2.js:15221-15251`), idempotent through its in-flight revocations
record (`:15189`, `:15223-15247`), reaching
`engagementStore.revoke({ engagementId, by, reason })`
(`lib/engagement-store.js:728-744`) and then the withdrawal work
(`:15239-15241`). Its cleanup retry is **operator-driven and has no sweeper**:
the route's own comment says the decision is already durable and only the
detachment is retried (`:15233-15234`). This slice wires the retirement and that
retry to `revoke` and `retry_cleanup`.

This is **parity**, not new behaviour. It is deliberately **separate** from the
refusal slice (`specs/task-rust-engagement-refuse.spec.md`); `retry_cleanup` is
part of **this** slice, not a third one, because its sole precondition — an
engagement in `revoked` whose `retire` effect is `failed` — is created by this
slice's own writer.

## Constraints

### Must

- Reach retirement through `DomainRepository::revoke` (`domain.rs:1260`) and its
  single writer `end(..., revoke = true)` (`:1285-1334`); reach the cleanup retry
  through `DomainRepository::retry_cleanup` (`:1265`). No second writer of
  `revoked`.
- Keep the state guard inside the store (`:1294-1300`): retirement is accepted
  from `pending | reserved | active`; any other state (including an already
  `revoked` or `rejected` engagement) is `Error::State` with no write.
- Cancel the outstanding provision obligation and schedule the retirement
  obligation, in the same transaction, exactly as `end` does: cancel the
  `kind='provision'` effect and bump its fence (`:1309`), and — **only when that
  effect had already left `pending`** — insert a `kind='retire'` effect and set
  the engagement's cleanup obligation to pending (`:1310-1313`).
- Stamp the `engagement_ends` row (`:1324-1328`) and reconcile graphs and routes
  (`:1329-1330`) inside the same transaction.
- Keep the store's decision idempotency for both writes: replaying the same
  `command_id` + digest returns the prior engagement with no second write
  (`:1289-1292`, `:1370-1272`, `decision_digest` `:356-357`,
  `replay_decision` `:359-381`).
- Make `retry_cleanup` reset **only** a failed retirement: the engagement must be
  in `revoked` (`:1274`) and exactly one `kind='retire'` effect in state `failed`
  must be moved back to `pending` with its outcome digest cleared (`:1277-1280`);
  anything else is `Error::State`.
- Authenticate the operator through the console session and finite scope gate
  (`native/hagency/src/console/agents.rs:139-160`), with the console's
  recheck-after-write pattern (`agents.rs:236-244`).

### Must Not

- **Do not add any automatic retry.** No sweeper, no timer, no background pass
  may re-drive a failed retirement; the retained product has none (its only
  interval helper, `trackLifecycleInterval` `backend-v2.js:17422-17426`, is not
  used for withdrawals) and inventing one would be **new behaviour, not parity**.
  The retry is an operator action only.
- Do not fold retirement into the provisioning ingress, and do not add a Matrix
  deny verdict (the intake refuses a non-`approve` decision as `Error::Wire`,
  `native/hagency-matrix/src/intake.rs:290-292`).
- Do not change `end`, `retry_cleanup`, the state guard, the `effects`/`decisions`
  /`engagement_ends` schemas, or the retention prune's predicates.
- Do not name a `Test:` selector for a test that does not exist. Every scenario
  below carries `Owed Selector:` until its test lands.

## Dependencies and assumptions (state honestly)

- **The retire-effect driver is NOT in this slice.** `claim_effect` is what moves
  a `pending` effect to `started` and `observe_effect` what settles it
  (`domain.rs:1338-1356`, `:1357+`), and today only the provisioning intake drives
  them for `kind='provision'` (`native/hagency-matrix/src/intake.rs:405-415`). A
  `kind='retire'` effect therefore cannot reach `failed` until a retire driver
  exists. **This slice asserts the retirement *decision* and the retry *write*;
  the scenario that exercises a failed retirement supplies the failed row as its
  Given** and the driver is a separate piece of work. Without the driver,
  retirement schedules an obligation that nothing carries out — which is exactly
  why the retry must not be a timer.
- A failed retirement is load-bearing beyond cleanup: the retention phase refuses
  to prune an engagement whose `retire` effect is `failed`
  (`engagement_retention.rs:386-388`), so an un-retried failure also pins the
  engagement's rows.

## Representation divergences (UNRESOLVED — decision owed, not decided here)

1. **Two terminal states where the retained product has one.** The port
   distinguishes `EngagementState::Revoked` from `EngagementState::Rejected`
   (`native/hagency-core/src/project.rs:236-243`; `domain.rs:143-144`). The
   retained product has **one** terminal state — `STATES =
   ['pending','active','ended']` (`lib/engagement-store.js:39`) — differentiated
   only by `endedReason` (`'revoked'` `:735`). **This spec assumes the port keeps
   its two states** and asserts `revoked`. **Unresolved.**
2. **A shared effects/fence table where the retained product keeps a per-record
   retirement field.** The port writes `effects(kind IN ('provision','retire'),
   state, fence)` (`native/hagency-store/src/domain.sql:50-59`) plus a `cleanup`
   obligation on the engagement, and retries by resetting the `retire` row
   (`domain.rs:1277`). The retained product keeps `withdrawal` per record
   (`{state:'pending'|'failed'|'retained'|'complete', scope:'room'|'agent'}`,
   `lib/engagement-store.js:700-725`) and `matrixRetirement` on the agent
   (`backend-v2.js:15198`, `:15213`). The retained `withdrawal.scope`
   (`:15239`) and the remote `/retire-agent` step
   (`lib/palpo-agent-retirement.js:9-24`) have **no port counterpart named in
   this slice** — this spec asserts only the effects-table shape and the
   `cleanup` obligation. **Unresolved.**

"Unresolved" means: not settled by this document, and not assigned an ADR number
here. The two are recorded so a reader can decide whether either needs its own
record.

## Production-caller gate (state honestly)

Every scenario names `hagency::console::engagements::retire` or
`hagency::console::engagements::cleanup_retry` — the handlers **this slice intends
to add** (`native/hagency/src/console/engagements.rs`, mounted under the console
API `/api` router beside `agents::router()`,
`native/hagency/src/console.rs:70-80`). Those names **cannot resolve yet**: the
module file does not exist. Measured on `66c5a184` with this slice present,
`node native/scripts/check-production-callers.mjs` (a wired CI gate,
`.github/workflows/rust.yml:124`) returns `count 31, wired 17`, `unresolved` = 8
entries for this file's lines — the named handlers, all with reason "no module
file for hagency::console::engagements". Baseline without this file: `count 17,
wired 17, unresolved 0`.

The interim form that keeps that gate green — verified: `owed (G2)` resolves
against ADR-146's gap table (`retry_cleanup` is row `domain.rs:1265`, class
"gap G2/G5 (shared)", precisely this slice) and produces `count 31, wired 17,
owed 8, unresolved 0` — is `Production caller: owed (G2)`, to be replaced by the
named handlers when the route lands. The named form is kept because the intent is
to build, not defer; **the builder must add the module in the same change or the
gate will fail.**

## Acceptance Criteria

Scenario: An active engagement is retired and its obligations are transferred
  Test: native_engagement_retire_active_is_revoked
  Given an engagement in state active whose provision effect is complete
  When the operator retires it through the console retire route with the operator's command id
  Then the engagement's stored state is revoked, its provision effect is cancelled with its fence advanced, a retire-kind effect is scheduled pending for it with the engagement's cleanup obligation set to pending, and an engagement_ends row exists for it
  Production caller: hagency::console::engagements::retire
  Retained: POST /api/engagements/:id/revoke (backend-v2.js:15221-15251) -> lib/engagement-store.js:728-744 (revoke: state='ended' :733, endedReason='revoked' :735, withdrawal={state:'pending'} :736, record engagement.revoked :737, pruneEnded :741, one commit :742) -> :15239 beginWithdrawal -> lib/engagement-store.js:700-707; then retireEngagementAgent backend-v2.js:15190 or detachEngagement :14931 -> setWithdrawalOutcome :15241 -> lib/engagement-store.js:709-725

Scenario: An already-ended engagement cannot be retired again
  Test: native_engagement_retire_requires_a_live_engagement
  Given an engagement already in revoked or rejected
  When the operator retires it again under a new command id
  Then the store returns a state error and neither the engagement's state nor its effects rows change
  Production caller: hagency::console::engagements::retire
  Retained: lib/engagement-store.js:731 throws EngagementError('conflict', `engagement is ${e.state}, not active`); the route's ended re-entry (backend-v2.js:15228) retries only detachment, it never re-writes the decision

Scenario: A replayed retirement command is idempotent
  Test: native_engagement_retire_replays_the_recorded_decision
  Given a retirement already recorded under a command id and its revoke digest
  When the identical command id and digest are presented again
  Then the prior engagement is returned and no second decisions row and no second effect write occurs
  Production caller: hagency::console::engagements::retire
  Retained: backend-v2.js:15189, :15223-15247 (the in-flight revocations record: concurrent identical revokes share one promise) and :15227-15228 (a settled revoke re-enters on state==='ended'); NOTE the port is more idempotent than the retained store: a replayed revoke against an ended engagement throws conflict here (lib/engagement-store.js:731), it does not replay

Scenario: A retirement with a reused command id but different content is a conflict
  Test: native_engagement_retire_rejects_a_changed_replay
  Given a retirement already recorded under a command id
  When the same command id is presented with a digest for a different engagement or decision kind
  Then the store returns a conflict and no write occurs
  Production caller: hagency::console::engagements::retire
  Retained: lib/engagement-store.js:731 (a conflicting decision on a non-active engagement is refused, never a silent second write)

Scenario: A retirement naming an unknown engagement is not found
  Test: native_engagement_retire_unknown_is_not_found
  Given an id that names no engagement
  When the operator retires it through the console retire route
  Then the store returns not-found and no rows are written
  Production caller: hagency::console::engagements::retire
  Retained: lib/engagement-store.js:730 throw EngagementError('not_found', 'engagement not found')

Scenario: An operator retries a failed retirement
  Test: native_engagement_retry_cleanup_requeues_a_failed_retire
  Given an engagement in state revoked whose retire-kind effect is in state failed (the failed row supplied as the Given while the retire driver is unwired)
  When the operator retries cleanup through the console cleanup-retry route with the operator's command id
  Then exactly one retire-kind effect returns to state pending with its outcome digest cleared, and the engagement's stored state is unchanged
  Production caller: hagency::console::engagements::cleanup_retry
  Retained: backend-v2.js:15233-15234 ("The decision is already durable. Retry only detachment, rechecking other live allocations before removing a binding or Matrix room seat.") and :15239-15241 (the retry re-runs beginWithdrawal/retireEngagementAgent/detachEngagement + setWithdrawalOutcome)

Scenario: A cleanup retry is refused when there is nothing failed to retry
  Test: native_engagement_retry_cleanup_requires_a_failed_retire
  Given an engagement that is not in state revoked, or whose retire-kind effect is not in state failed
  When the operator retries cleanup
  Then the store returns a state error and no effects row is modified
  Production caller: hagency::console::engagements::cleanup_retry
  Retained: no separate guard exists in the retained product — it re-runs the idempotent detachment (backend-v2.js:15239-15241). The port's guard is a deliberate tightening of the shape, not a parity claim

Scenario: A failed retirement is never retried without an operator
  Test: native_engagement_failed_retire_waits_for_an_operator
  Given an engagement in state revoked whose retire-kind effect is in state failed
  When no operator retries and an arbitrary period elapses (no sweeper pass, no tick, no timer fires)
  Then the retire effect remains failed, the engagement remains revoked, and no automatic retry write occurs
  Production caller: hagency::console::engagements::cleanup_retry
  Retained: no sweeper — the only interval helper (backend-v2.js:17422-17426) is unused for withdrawals; the retry is the operator re-POSTing the revoke route (:15221-15251, comment :15233-15234)
  Also: the failed retire pins the engagement from the retention prune (native/hagency-store/src/domain/engagement_retention.rs:386-388)

## Out of Scope

- **The retire-effect driver** (claim/observe for `kind='retire'`): named under
  "Dependencies" above, not built here.
- **The remote retirement step** the retained product performs
  (`lib/palpo-agent-retirement.js:9-24`, `POST /retire-agent`, strict
  verification) and the `withdrawal.scope` room-vs-agent split
  (`backend-v2.js:15239`): the port has no counterpart named in this slice.
- **Refusal of a pending request**: the separate slice in
  `specs/task-rust-engagement-refuse.spec.md`.
- **`register` and `create_canonical_task`**: the two other ADR-146 unowned rows
  are separate, smaller parity items. Neither belongs to this slice.
- Any automatic retry, sweeper or timer.
