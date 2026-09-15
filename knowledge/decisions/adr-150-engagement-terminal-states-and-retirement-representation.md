---
kind: decision
id: ADR-150
title: "Engagement terminal states and the retirement representation"
status: Decided
requirements: [REQ-RUST-MIGRATION-EXECUTION]
tags: [native, engagement, lifecycle, effects, retirement, parity, spec-governance]
---

## Context

Two spec slices for the engagement lifecycle —
`specs/task-rust-engagement-refuse.spec.md` and
`specs/task-rust-engagement-retire.spec.md` — each carry a section headed
"Representation divergences (UNRESOLVED — decision owed, not decided here)".
They were written that way deliberately: the *parity* they assert is settled by
the retained product, but the *representation* the port should use is a decision
about the port, and a spec is not the place to make one. This ADR makes it.

**Measured at `c9f6df9d`** ("test(native): the console roster shows an agent
provisioned through the real ingress"), the head of `feat/rust-migration` this
lane holds; `origin` is unfetchable from this lane (`git fetch` → `Operation not
permitted` on the remote), so this is the newest source tree available here. The
port claims below are read from that tree; the retained claims from the tracked
retained sources (`lib/engagement-store.js`, `backend-v2.js`), which that ref
does not modify.

**The two divergences.**

1. **Terminal states.** The port writes two distinct terminal states —
   `EngagementState::Rejected` and `EngagementState::Revoked`
   (`native/hagency-core/src/project.rs:236-243`). The retained product has
   **one** terminal state: `STATES = ['pending','active','ended']`
   (`lib/engagement-store.js:39`), with the distinction carried as free text in
   `endedReason` (`'rejected'` `:604`, `'revoked'` `:735`).
2. **Retirement representation.** The port schedules a `retire` row in an
   `effects` table carrying `kind`, `state`, a `fence` and a
   `UNIQUE(engagement_id,kind)` (`native/hagency-store/src/domain.sql:50-59`),
   and holds a per-engagement `CleanupState`
   (`native/hagency-core/src/project.rs:246-251`). The retained product writes a
   per-record `withdrawal` object on the engagement
   (`lib/engagement-store.js:700-725`) and a `matrixRetirement` object on the
   agent record (`backend-v2.js:15197-15215`).

## Decision

### 1. The port keeps both terminal states — `Rejected` and `Revoked` do not collapse

**They do not collapse, because they are not the same fact, and four consumers
already branch on the difference.**

What each means that the other does not:

- **`Rejected`** (`domain.rs:1318`) is a **request-stage refusal**: the
  engagement was admitted (`Pending`, `domain.rs:1133`) and the provider or
  operator declined before any provisioning work. No `provision` effect was ever
  written for it, and its `cleanup` stays `CleanupState::NotRequired`.
- **`Revoked`** (`domain.rs:1316`) is a **retirement of live work**: the
  engagement had left `Pending` (it was `Reserved` `:1244` and/or `Active`
  `:1400`). Its `provision` effect existed and is cancelled with its fence
  advanced (`:1309`); when that effect had already left `pending`, a `retire`
  effect is scheduled and `cleanup` becomes `Pending` (`:1310-1313`).

The consumers that branch on the distinction:

| consumer | site | what it does with the difference |
|---|---|---|
| `end`'s state guard | `domain.rs:1294-1300` | admits both terminals from `Pending\|Reserved\|Active`, but `!revoke && value.state != Pending` restricts a **refusal** to `Pending` only |
| `end`'s effect branch | `domain.rs:1308-1314` | writes the `retire` effect only when the provision effect had left `pending` — the arm that produces `Revoked` |
| `retry_cleanup` | `domain.rs:1274` | `if value.state != Revoked` → `Error::State`: **only** a revoked engagement may retry a failed retirement |
| `claim_effect` | `domain.rs:1342` | selects the `retire` effect only for `e.state='revoked'`, while the `provision` effect requires `e.state='reserved'` |
| retention candidate query | `engagement_retention.rs:363`, `:382-384`, `:530` | treats `rejected` and `revoked` alike for candidacy, but its `retire`-effect pin (`:386-388`) is what a revoked-and-failed engagement carries and a rejected one does not |

**What a reader of the engagements projection sees.** The stored projection is
the `Engagement` struct, which carries `state` **and** `cleanup`
(`native/hagency-core/src/project.rs:265-266`). The roster read copies
`state: engagement.state` (`domain.rs:1060`) and the console serves it as
`state: row.state` with `state: EngagementState`
(`native/hagency/src/console/agents.rs:42`, `:94`). So an operator sees the
literal state — `"rejected"` or `"revoked"` — plus, via `cleanup`, whether the
engagement owes retirement work at all.

**What collapsing would cost** (the alternative, rejected): a single terminal
state would make `retry_cleanup`'s only precondition (`:1274`) inadmissible —
it would have to admit by "terminal" and fail later by the absence of a `retire`
row (`:1278-1280`) — and `claim_effect`'s retire selection (`:1342`) would lose
its key. It would also require migrating every `state='rejected'` row to the
surviving value and changing `domain.sql:36`'s `CHECK`, `state_name`
(`domain.rs:143-144`) and both guards — strictly more work than keeping both,
with the operator's ability to tell "declined request" from "retired live
engagement" destroyed.

**What the retained product loses by having one:** its `endedReason` free text
preserves the *label* (`:604`, `:735`) but nothing branches on it. Its guards are
coarser — `state !== 'pending'` → `conflict` (`:596`) and `state !== 'active'` →
`conflict` (`:731`) — so retained cannot express "retire only from
reserved-or-active" as a first-class rule, and a re-refusal of an already-ended
engagement is a generic conflict rather than a named state rule. The port's two
states give **stronger, named** guard semantics; that is the port's own
improvement, kept deliberately.

### 2. The effects-and-fence table governs retirement; the per-record field is not adopted — and there is no automatic driver

**The port keeps its `effects` table, its fence and its `CleanupState`.** It is
already the spine of the retirement path: the table and its constraints exist
(`domain.sql:50-59`, including `UNIQUE(engagement_id,kind)` `:58` and the
`pending_effects` index `:60`), `end` writes the row (`domain.rs:1312`),
`claim_effect` claims it and advances the fence (`:1342`, `:1346-1348`),
`observe_effect` settles it and moves `cleanup` (`:1400-1424`), `retry_cleanup`
resets a failed row (`:1277`), and the retention pin refuses to prune an
engagement whose retire effect is `failed` (`engagement_retention.rs:386-388`).

The retained per-record shape (`lib/engagement-store.js:700-725`,
`backend-v2.js:15197-15215`) is not adopted, because adopting it would replace
the table, the fence and the shared claim path, and would break the retention pin
that reads the `retire` row.

**What the retire-effect driver owes — and it is operator-triggered, never a
sweeper.**

`claim_effect` already selects both kinds (`domain.rs:1342`); today only the
`provision` arm is driven, inline by the provisioning intake
(`native/hagency-matrix/src/intake.rs:405-415`). The `retire` arm has no driver
at all. The driver owes, in this order:

1. **An operator-triggered claim and observe.** A caller that claims a pending
   `retire` effect (`:1342`) and observes its outcome (`observe_effect`, `:1357`
   — `Complete` sets `cleanup = Complete` `:1402`, `Uncertain` `:1424`,
   `Pending` `:1418`). It belongs behind the same console retire surface as
   `revoke`, mirroring the retained re-POST
   (`backend-v2.js:15233-15234`, "The decision is already durable. Retry only
   detachment…").
2. **The retry pairing.** A `failed` effect is reset by `retry_cleanup`
   (`:1274-1280`) — an operator act, part of the retire slice, not a third slice.
3. **The retirement effect itself.** The retained product performs a **remote**
   retirement and verifies it strictly (`lib/palpo-agent-retirement.js:9-24`,
   `POST /retire-agent`, requiring `state==='retired'`,
   `matrixIdentity==='deactivated'`, `appserviceAccess==='revoked'`, empty
   `joinedRooms`). **The port has no counterpart to this step** (grep for
   `retire-agent`/`retire_agent` across `native/` finds only a fixture inventory
   entry). See the deferral below.

**No automatic retry, in any form.** The retained product has no sweeper: its
only interval helper (`backend-v2.js:17422-17426`) is not used for withdrawals,
and the retry is the operator re-POSTing the revoke route (`:15221-15251`). A
periodic driver over pending or failed retire effects would be **new behaviour,
not parity**, and would auto-retry a failure the retained product deliberately
leaves to a human.

## Undecided (evidence would settle it)

**Whether the port's retire executor must reproduce the retained remote
retirement and its strict verification** (`lib/palpo-agent-retirement.js:9-24`)
is **not settled by the evidence I hold.** The settled part is only that the port
has no such step today; whether it *should* have one depends on a capability I
cannot see from this lane: whether the port's Palpo transport exposes an
agent-retirement endpoint at all. **Evidence that would settle it:** an
inspection of `native/hagency-palpo` (and the outbound transport configuration)
for an agent-retirement request/response pair. Until then, the retire-effect
driver's scope is "claim, observe, record" with the remote step explicitly
unowned — named here rather than assumed into a builder's remit.

## Alternatives Considered

- *Collapse the two terminal states into one `Ended` with a reason field.*
  Rejected: it loses `retry_cleanup`'s only precondition (`domain.rs:1274`) and
  `claim_effect`'s retire key (`:1342`), forces a migration of existing
  `rejected` rows plus a `CHECK`/`state_name`/guard change (`domain.sql:36`,
  `domain.rs:143-144`, `:1294-1300`), and removes the operator's ability to
  distinguish a declined request from a retired live engagement — more work, less
  information.
- *Adopt the retained per-record `withdrawal`/`matrixRetirement` shape.*
  Rejected: it replaces the `effects` table, the fence and the shared claim path
  (`domain.sql:50-59`, `domain.rs:1338-1356`) and breaks the retention pin
  (`engagement_retention.rs:386-388`). The port's table is what its own
  concurrency rules are built on.
- *Drive retirement with a periodic sweeper.* Rejected: the retained product has
  none, so this is new behaviour rather than parity, and it would retry a failed
  retirement automatically — precisely what the retained operator-driven retry
  exists to avoid (`backend-v2.js:15233-15234`).
- *Drop the `retire` effect entirely; do nothing after a revoke.* Rejected: the
  retention pin (`:386-388`) and `retry_cleanup` (`:1274-1280`) both exist to
  hold the failure; with no row there is nothing to pin and nothing to reset, and
  a revoked engagement becomes silently un-retirable.
- *Leave both divergences unresolved and let each builder choose.* Rejected:
  that is the state these specs were deliberately written in, and it is what this
  ADR exists to end. The two decisions above are supported by the consumers cited;
  the one item the evidence does not reach is named as Undecided rather than
  guessed.
