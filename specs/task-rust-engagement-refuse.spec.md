spec: task
name: "Refuse a pending engagement request through the console verdict route"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, engagement, lifecycle, console, parity]
---

## Intent

Give the engagement lifecycle its refusal arm. The native store already models a
refusal — `DomainRepository::reject` (`native/hagency-store/src/domain.rs:1257`),
a two-line wrapper over `end(command_id, id, false)`
(`native/hagency-store/src/domain.rs:1285-1334`), which is the sole writer of
`EngagementState::Rejected` (`:1318`) — but no production code calls it. An
admitted request that a provider or operator declines therefore has no exit in
the Rust port: the engagement stays `pending` forever.

The retained product refuses such a request through
`POST /api/engagements/:id/verdict` (`backend-v2.js:15160-15187`): the
else-branch (anything not `approve === true`) reaches
`engagementStore.decide({ approve: false, ... })` (`lib/engagement-store.js:593-613`),
which requires `state === 'pending'` (`:596`) and writes the refusal. This slice
wires that same act to `reject`, behind a console verdict route.

This is **parity**, not new behaviour. It is deliberately **separate** from the
retirement slice (`specs/task-rust-engagement-retire.spec.md`): the retained
refusal never touches the withdrawal/retirement lifecycle
(`lib/engagement-store.js:700-707` returns early without an allocation), and the
two guarded states differ (a refusal is `pending`-only).

## Constraints

### Must

- Reach the refusal through `DomainRepository::reject` (`domain.rs:1257`) and its
  single writer `end(..., revoke = false)` (`:1285-1334`) — never a second
  `UPDATE engagements` path.
- Guard on the engagement's own state, inside the store, exactly as the retained
  `decide` does (`lib/engagement-store.js:596`): a refusal is accepted **only**
  from `pending` (`domain.rs:1294-1300`); any other state is `Error::State` with
  no write.
- Keep the store's decision idempotency: a replay of the **same**
  `command_id` + the same `decision_digest("reject", id)` returns the prior
  engagement without a second write (`domain.rs:1289-1292`,
  `decision_digest` `:356-357`, `replay_decision` `:359-381`).
- Record the decision receipt in the same transaction (`record_decision`,
  `domain.rs:397-412`), so a rolled-back verdict carries no receipt.
- Stamp the `engagement_ends` row (`domain.rs:1324-1328`,
  `migrations/030-engagement-retention.sql:15-18`), because that side table is
  what the retention phase's candidate query reads
  (`engagement_retention.rs:380-382`).
- Authenticate the operator behind the console session and the finite scope gate
  the console already uses (`native/hagency/src/console/agents.rs:139-160`), and
  re-check that scope before responding — the console's recheck-after-write
  pattern (`agents.rs:236-244`).

### Must Not

- Do not fold the refusal into the provisioning ingress or the approval verdict:
  the refusal is an operator/console act, not a Matrix admission
  (`specs/task-rust-provisioning-ingress.spec.md`).
- Do not add a Matrix-level deny verdict in this slice. The retained
  `denyPending` (`backend-v2.js:10958`) denies an *approval delivery*, a
  different act; the Rust intake deliberately refuses a non-`approve` decision as
  `Error::Wire` (`native/hagency-matrix/src/intake.rs:290-292`).
- Do not change `end`, the state guard, the `decisions`/`engagement_ends`
  schemas, or move the write into a new store method.
- Do not add any timer, sweeper or background retry (see the retire slice's
  "no automatic retry" constraint; a refusal has no cleanup at all).
- Do not name a `Test:` selector for a test that does not exist. Every scenario
  below carries `Owed Selector:` until its test lands.

## Representation divergences (UNRESOLVED — decision owed, not decided here)

1. **Two terminal states where the retained product has one.** The port
   distinguishes `EngagementState::Rejected` from `EngagementState::Revoked`
   (`native/hagency-core/src/project.rs:236-243`; `state_name` `domain.rs:143-144`).
   The retained product has **one** terminal state — `STATES =
   ['pending','active','ended']` (`lib/engagement-store.js:39`) — differentiated
   only by `endedReason` (`'rejected'` `:604`, `'revoked'` `:735`). **This spec
   assumes the port keeps its two states** and asserts `rejected` for a refusal.
   If the decision goes the other way, every `Then` below that names `rejected`
   must be restated against a single `ended`-with-reason shape. **Unresolved.**
2. **A shared effects/fence table where the retained product keeps a per-record
   field.** The port writes `effects(kind IN ('provision','retire'), state,
   fence)` (`native/hagency-store/src/domain.sql:50-59`) and a `cleanup` field on
   the engagement. The retained product keeps a per-record `withdrawal` object
   (`lib/engagement-store.js:736`) and `matrixRetirement` on the agent
   (`backend-v2.js:15198`). **This spec assumes the port keeps its table**, and
   for a refusal it asserts the *absence* of effect writes (a pending engagement
   has no provision effect to cancel). The refusal arm is where the two models
   differ least; the retire slice is where they differ most. **Unresolved.**

"Unresolved" means: not settled by this document, and not assigned an ADR number
here. The two are recorded so a reader can decide whether either needs its own
record.

## Production-caller gate (state honestly)

Every scenario names `hagency::console::engagements::verdict` — the handler **this
slice intends to add** (`native/hagency/src/console/engagements.rs`, mounted under
the console API `/api` router beside `agents::router()`,
`native/hagency/src/console.rs:70-80`). That name **cannot resolve yet**: the
module file does not exist. Measured on `66c5a184` with this slice present,
`node native/scripts/check-production-callers.mjs` (a wired CI gate,
`.github/workflows/rust.yml:124`) returns `count 31, wired 17`, `unresolved` = 6
entries for this file's lines — the named handlers, all with reason "no module
file for hagency::console::engagements". Baseline without this file: `count 17,
wired 17, unresolved 0`.

The interim form that keeps that gate green — verified: `owed (G2)` resolves
against ADR-146's gap table (`retry_cleanup` is row `domain.rs:1265`, class
"gap G2/G5 (shared)", the same refusal/retirement slice) and produces `count 31,
wired 17, owed 6, unresolved 0` — is `Production caller: owed (G2)`, to be
replaced by the named handler when the route lands. The named form is kept
because the intent is to built, not deferred; **the builder must add the module
in the same change or the gate will fail.**

## Acceptance Criteria

Scenario: A pending engagement request is refused and recorded as rejected
  Owed Selector: native_engagement_refuse_pending_is_rejected (owed — no test yet; no Test: line here)
  Given an engagement in state pending that has no provision effect (it was admitted but never approved)
  When the operator refuses it through the console verdict route with the operator's command id
  Then the engagement's stored state is rejected, its projection carries the refusal and no cleanup obligation, an engagement_ends row exists for it, and no effects row is written for it
  Production caller: owed (G10)
  Retained: POST /api/engagements/:id/verdict else-branch (backend-v2.js:15160-15187) -> lib/engagement-store.js:593-613 (decide approve:false: requires pending :596, state='ended' :602, endedReason='rejected' :604, decidedAt/decidedBy :605-606, record engagement.rejected :607, pruneEnded :611, one commit :612); detachEngagement backend-v2.js:15182 -> :14931

Scenario: A refusal is refused when the engagement is not pending
  Owed Selector: native_engagement_refuse_requires_pending (owed — no test yet; no Test: line here)
  Given an engagement in active (or any state other than pending)
  When the operator refuses it through the console verdict route
  Then the store returns a state error, no engagements row and no engagement_ends row is written, and the engagement's projection is unchanged
  Production caller: owed (G10)
  Retained: lib/engagement-store.js:596 throws EngagementError('conflict', `engagement is ${e.state}, not pending`); respondEngagementError maps conflict to 409 (backend-v2.js:14953-14958)

Scenario: A replayed refusal command is idempotent
  Owed Selector: native_engagement_refuse_replays_the_recorded_decision (owed — no test yet; no Test: line here)
  Given a refusal already recorded under a command id and its reject digest
  When the identical command id and digest are presented again
  Then the prior engagement is returned and no second decisions row and no second state write occurs
  Production caller: owed (G10)
  Retained: backend-v2.js:15189, :15223-15247 (the in-flight revocations record makes concurrent identical requests share one promise) — NOTE the port is more idempotent than the retained store here: retained decide() replayed against an already-ended engagement throws conflict (:596), it does not replay

Scenario: A refusal with a reused command id but different content is a conflict
  Owed Selector: native_engagement_refuse_rejects_a_changed_replay (owed — no test yet; no Test: line here)
  Given a refusal already recorded under a command id
  When the same command id is presented with a digest for a different engagement or decision kind
  Then the store returns a conflict and no write occurs
  Production caller: owed (G10)
  Retained: lib/engagement-store.js:596 (a different decision on a non-pending engagement is a conflict, never a silent second write)

Scenario: A refusal naming an unknown engagement is not found
  Owed Selector: native_engagement_refuse_unknown_is_not_found (owed — no test yet; no Test: line here)
  Given an id that names no engagement
  When the operator refuses it through the console verdict route
  Then the store returns not-found and no rows are written
  Production caller: owed (G10)
  Retained: lib/engagement-store.js:595 throw EngagementError('not_found', 'engagement not found'); route 404 at backend-v2.js:15154-15155

Scenario: A refusal schedules no retirement work
  Owed Selector: native_engagement_refuse_schedules_no_retirement (owed — no test yet; no Test: line here)
  Given an engagement in state pending with no provision effect
  When the operator refuses it
  Then no retire-kind effects row is inserted, the provision effect is not touched, and the engagement's cleanup obligation stays not-required
  Production caller: owed (G10)
  Retained: backend-v2.js:15180-15183 (decide approve:false) and lib/engagement-store.js:700-702 (beginWithdrawal returns early unless state==='ended' and allocatedTokens>0) — a refusal never starts a withdrawal

## Out of Scope

- **Retirement and cleanup-retry**: `revoke` and `retry_cleanup` are the separate
  slice in `specs/task-rust-engagement-retire.spec.md`. A refusal never starts a
  withdrawal, so the two do not share a test.
- **`register` and `create_canonical_task`**: the two other ADR-146 unowned rows
  are separate, smaller parity items (fleet self-registration, and the
  receive-inbox plan's task minter). Neither belongs to the engagement-lifecycle
  slices.
- The approval/deny verdict surface (owned elsewhere), the provisioning ingress
  (`specs/task-rust-provisioning-ingress.spec.md`), the console roster read, and
  any change to `admit`/`approve`/`verify_request`.
- Any automatic retry, sweeper or timer.
