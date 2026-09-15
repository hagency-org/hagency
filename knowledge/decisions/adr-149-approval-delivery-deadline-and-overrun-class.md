---
kind: decision
id: ADR-149
title: "Whose clock bounds approval-card delivery, and what class an overrun is"
status: Decided
requirements: [REQ-RUST-MIGRATION-EXECUTION]
tags: [native, approval, delivery, deadline, cancellation, custody]
---

## Context

`native_private_approval_delivery_is_wired`
(`native/hagency/tests/bootstrap/approval.rs:67`) failed on a hosted Ubuntu lane
with `approval card not delivered within the fixture watchdog`
(`:191-193`) and the child's own stderr naming
`send outcome: Matrix operation cancelled`. It is an intermittent: it is not
caused by the commit it appeared under, and a later stack containing that same
commit passed the same lane green. Root-causing it produced one finding that is
a decision, not a repair:

**The private approval card's delivery budget is anchored on the owner's
decision window, and its overrun is reported as a cancellation.**

Three sites compose that:

1. The delivery deadline is derived from the *owner's* expiry —
   `native/hagency-matrix/src/approval_delivery.rs:92-94`:
   `Instant::now() + Duration::from_millis(card.owner_expires_at().saturating_sub(now)).min(Duration::from_secs(45))`.
2. On that same deadline the job **cancels the very token it handed the work
   future** — `:96` `let cancel = cancel.child_token();` (the token passed to
   `deliver_private_card` at `:101`) and `:102`
   `tokio::select!{r=&mut work=>r,_=tokio::time::sleep_until(deadline)=>{cancel.cancel();work.await}}`.
3. The transport then reports the expired budget as a cancellation, because the
   select is `biased` with the token arm first —
   `native/hagency-matrix/src/http.rs:479-483`:
   `_ = cancel.cancelled() => Err(Error::Cancelled)` precedes
   `result = timeout_at(deadline, future) => result.map_err(|_| Error::Timeout)`.
   An overrun and a real cancellation are therefore indistinguishable at the
   instant they occur, and both surfaces carry distinct words —
   `native/hagency-matrix/src/lib.rs:39-42` (`"Matrix operation cancelled"`,
   `"Matrix operation timed out"`).

The 1000 ms budget this inherits is not incidental. `owner_wait_ms` is fixed at
`native/hagency/src/bootstrap/config.rs:412` (`ApprovalHost::new(8, 2, 1000,
response_reserve_ms)`), validated at
`native/hagency-execution/src/approval/capacity.rs:14-32`. It originates as the
owner's decision deadline — `native/hagency-runtime/src/codex/transport/control.rs:41`
(`let owner = self.origin + Duration::from_millis(admitted + owner_ms);`) — and
is inverted back to a wall-clock expiry at
`native/hagency-execution/src/approval/state.rs:224-230`. That single number is
then made to fund the host's whole delivery: reading the card
(`native/hagency/src/bootstrap/approval.rs:85-101`), opening the bot's SDK
owner, `Command::Read`/`Start`/`Query`, `POST /keys/query`, `Command::Encrypt`
and the room-event `PUT` (`approval_delivery.rs:298-375`). When that work
overruns, `checkpoint` (`native/hagency-matrix/src/enrollment.rs:265-273`) sees
`cancel.is_cancelled()` first and returns `Cancelled`, never `Timeout`.

**Precedent — the shape decided here already exists three times.** Sibling
delivery surfaces separate the classes and map a fired *deadline* to `Timeout`:
`native/hagency-matrix/src/attachments.rs:259-265` (token arm, then a
`tokio::time::sleep_until(deadline) => Err(Error::Timeout)` arm, then an in-band
`is_cancelled()` / `now >= deadline` re-check), `native/hagency-matrix/src/upload.rs:193-197`,
and `native/hagency-matrix/src/receive.rs:99-102`. The approval-card send is the
one card-delivery path that does not; the fresh-enrollment wrapper
(`approval_delivery/enrollment.rs:29`) shares the self-cancel shape and stays
outside this record's scope.

**Why this is observable at all.** The delivery leg cannot be observed by the
composition on hosted lanes; ADR-138 records this and the fixture slice that
made it observable — "PC-C0's wiring observation and the fixture slice (PC-C0b)"
(`knowledge/decisions/adr-138-bounded-native-approval-observation.md:106-121`):
a callback-capable runtime probe selected by the test's own pinned executable
path, plus a scripted second-identity enrollment of the approval collector
against the shared fake peer, with "no production change" following. This record
changes no part of that machinery; it relies on it to have seen the defect at all.

## Decision

### 1. Whose clock bounds delivery

**The host's delivery obligation is bounded by a delivery clock that starts at
the first send attempt, not by the owner's decision deadline.** Delivery and
deciding are different obligations with different owners: the owner decides
whether to allow the action, the host delivers the card that asks. Today they
share one deadline because the delivery deadline is computed from
`card.owner_expires_at()` (`approval_delivery.rs:92-94`), which is the owner's
window expressed as a wall clock.

`card.owner_expires_at()` **continues to govern the owner's decision window, and
nothing else changes hands**:

- it stays the admission gate for the card —
  `native/hagency-store/src/domain/approvals/card.rs:46-50` refuses a card whose
  `owner_expires_at <= now` or exceeds the request's own `expires_at`;
- it stays the veto a fresh check applies before any authority is conferred
  (the same `check_private_approval_card` gate);
- it stays the bound on the owner's *response*, separate from delivery.

What the delivery clock must **not** do is borrow the owner's remaining window
as its own budget. The owner's clock bounds *deciding*; the host's clock bounds
*delivering*. Every bound that exists today keeps an owner; none is removed.

### 2. What class an overrun is

**An expired budget is `Error::Timeout`. `Error::Cancelled` means a real
cancellation** — the process is stopping, or a caller withdrew the work.

This is the `lib.rs:39-42` split, which already exists and already has distinct
messages. The change is that the delivery path must stop *manufacturing* a
cancellation out of its own expired timer: the arm at `approval_delivery.rs:102`
must not `cancel.cancel()` the token it then awaits. A deadline must surface as
`Error::Timeout` through the existing `timeout_at` arm (`http.rs:482`) or the
`checkpoint` deadline arm (`enrollment.rs:268-269`), exactly as
`attachments.rs:262`, `upload.rs:196` and `receive.rs:102` already do.

**What depends on the distinction — searched, and the case is weakened, so it is
recorded rather than hidden.** Every consumer of these two classes was inspected
at `66c5a184`:

- **No consumer branches on the two classes.** The only class-sensitive sites on
  this path are a string format (`approval_delivery.rs:117`,
  `format!("private approval card send failed: {error}")`) and two test
  literals. The fail-closed denial leg (`:105-133`) treats any error
  identically: it denies the pending request at most once, whatever the class.
- **Therefore the change is safe, and that is also the cost**: because nothing
  branches on the distinction, no existing behaviour proves the distinction is
  preserved. The only current evidence for the class is the operator-visible
  string in `delivery_denial_reason` (`native/hagency-store/src/domain/approvals.rs:458`),
  against a hand-written literal in
  `native/hagency-store/tests/approvals/reissue.rs:27` —
  `"private approval card send failed: Matrix operation timed out"`. **That
  literal is the precedent the product should match and the product currently
  contradicts**: the store's own test suite already assumes an overrun says
  *timed out*, while the delivery path can only ever emit *cancelled*.
- **Honest weakening:** no live consumer *requires* the split today. The
  distinction is being decided because it is the product's existing vocabulary
  (`lib.rs:39-42`), because the ADR-137 denial text is the operator's only
  diagnosis, and because a mis-classified overrun is what sent this
  investigation down the wrong root cause in the first place — not because a
  caller breaks without it.

### 3. Where the bound lives

**Per round trip, on the existing `timeout_at` (`http.rs:482`), fed by a
delivery deadline that starts at first attempt.** The bound is enforced at the
I/O boundary, where a stall actually occurs — the same place
`attachments.rs:259-265`, `upload.rs:193-197` and `receive.rs:99-102` put it —
and the body read re-applies it per chunk
(`http.rs:446-453`, `body_idle`). The delivery continues to carry a
whole-obligation ceiling (`approval_delivery.rs:92-94`) so the send cannot run
unbounded, but that ceiling is a *last resort*, evaluated on the delivery clock,
and its expiry is a `Timeout`.

**Rejected: one wrapper around the whole job.** Wrapping the entire delivery in a
single `sleep_until` is what `:102` does today. Its costs are concrete:
(a) it cannot report which leg overran, so its expiry is uninformative;
(b) it *destroys* the class distinction by cancelling the token the inner work
holds, which is exactly how a `Timeout` becomes a `Cancelled`;
(c) it bounds the sum of many round trips instead of each round trip, so one slow
`keys/query` consumes the room-event `PUT`'s budget; and
(d) it cancels its own child token as a timeout mechanism — a shape it shares
with `approval_delivery/enrollment.rs:29`, `approval_intake.rs:166`,
`outgoing.rs:75` and `upload/publication.rs:314`, all out of scope here — while
every sibling that classifies at the I/O boundary (`attachments.rs:259-265`,
`upload.rs:193-197`, `receive.rs:99-102`) uses a deadline arm that returns
`Timeout`.

### 4. What must not change

**No deadline, budget, ceiling or watchdog is lengthened by this record.**

- `STARTUP_WATCHDOG` stays at 15 s
  (`native/hagency/tests/bootstrap/fixture.rs:22`); it bounds *fixture waits*,
  not the product's delivery.
- `owner_wait_ms` stays 1000 (`config.rs:412`) and its validation stays
  (`capacity.rs:14-32`); this record does not raise the owner's budget.
- The 45 s delivery ceiling (`approval_delivery.rs:92-94`) is unchanged in value.
- Re-anchoring the ceiling on the delivery clock lengthens the effective
  delivery budget on a typical card from the owner's ~1 s remainder to that 45 s
  ceiling. The ceiling's value, `owner_wait_ms`, `STARTUP_WATCHDOG` and
  `pump.close()`'s 2 s are unchanged; the lengthening is the point — delivery
  stops spending the owner's window — and is recorded here rather than slipped
  past this rule.
- `pump.close()`'s 2 s bound and the shutdown ordering
  (`native/hagency/src/bootstrap.rs:1078-1084`) are unchanged.

**The fixture's 25 ms poll and the single shared fake-peer queue are
observability artifacts and are explicitly NOT the fix.** The poll
(`native/hagency/tests/bootstrap/approval.rs:136` and `:171`,
`Duration::from_millis(25)`) and the one `mpsc::channel(32)` request queue
shared by two identities (`native/hagency-matrix/tests/common/mod.rs:275-280`,
`:306`) make the intermittent reproducible under parallel load. They do not
cause it. A path that cancels its own send at ~1000 ms is broken at any load
level; lengthening the watchdog or making the poll faster would hide the defect
that this record exists to fix. No harness timing may be adjusted to make the
selector pass.

### 5. Consequences — the sites a builder touches, and the test that must fail first

**The test that must fail first, on the current product, and pass after.**
A delivery whose budget expires must return `Error::Timeout`, not
`Error::Cancelled`. `native_private_approval_delivery_is_wired` cannot carry
this: it observes delivery of a card that *does* arrive in the happy case, and
its only deadline-sensitive assertion is the fixture watchdog (`:191-193`,
`:243-245`) — it is red on the hosted lanes for the separate, already-recorded
PC-C0b reason (ADR-138:106-121). The asserting test belongs in the delivery
suite, where the deadline is already exercised: the expiry scenario at
`native/hagency-matrix/tests/approval_delivery/privacy.rs:97,111-114` and the
post-expiry refusal at
`native/hagency-matrix/tests/approval_delivery/enrollment.rs:176-193`. Both
currently assert only `is_err()` (`privacy.rs:112`, `enrollment.rs:188-193`) —
the class is unpinned today, which is why the defect survived. The new assertion
must pin the class: an expired delivery budget yields `Err(Error::Timeout)`, and
a genuine token cancellation still yields `Err(Error::Cancelled)`.

**Exact sites a builder touches:**

| Site | Change |
|---|---|
| `native/hagency-matrix/src/approval_delivery.rs:92-94` | Anchor the ceiling on a delivery clock started at first attempt, not on `card.owner_expires_at()`; keep the owner expiry as the card's admission gate (`card.rs:46-50`), unchanged. |
| `native/hagency-matrix/src/approval_delivery.rs:96,101-102` | Replace the deadline arm with a returning `Err(Error::Timeout)` arm (sibling shape); do not cancel the child token, and do not await the inner work unboundedly from the deadline arm. |
| `native/hagency-matrix/src/http.rs:479-483` | Unchanged in shape — it already carries the correct split; confirm the delivery's deadline (not a self-cancelled token) reaches it. |
| `native/hagency-matrix/src/enrollment.rs:265-273` | Unchanged; `checkpoint` already returns `Timeout` on an expired deadline once the token is not falsely cancelled. |
| `native/hagency-matrix/tests/approval_delivery/privacy.rs:97,111-114` and `…/enrollment.rs:188-193` | Pin the class on the expiry scenario instead of `is_err()`. |
| `native/hagency-store/tests/approvals/reissue.rs:27` | Already the intended literal; no change — it is the evidence the product should match. |

Good, because: an overrun is reported as what it is, so the ADR-137 denial text
tells the operator the truth and the next investigation does not chase a
cancellation that never happened; delivery is no longer funded by the owner's
window, so a slow host no longer loses the owner's authority by spending it; and
the one path that diverged from the workspace's three sibling delivery surfaces
is brought into line with them.

Bad, because: no live consumer branches on the class, so the change buys
diagnosis and correctness rather than a repairing a caller — its value is
observability, and it must be pinned by a new test assertion because no existing
behaviour will fail if the class silently regresses. And the underlying
1000 ms `owner_wait_ms` (`config.rs:412`) is untouched: this record fixes *whose
clock bounds delivery and what an overrun is called*, not how generous the
owner's window is; a delivery clock that is still too short would now at least
say so.

## Alternatives Considered

- **Lengthen the fixture watchdog.** Rejected. It bounds fixture waits
  (`fixture.rs:22`), and the defect is the product cancelling a send it should
  have completed; a longer window hides it under lighter load and leaves the
  mis-classification in place. Rule 4 forbids it.
- **Raise `owner_wait_ms`.** Rejected. It is the owner's decision budget, not the
  delivery's; spending the owner's window to fund delivery is the error being
  corrected, and `capacity.rs:14-32` + `operation_ms` (`config.rs:345`) would
  have to move with it.
- **Keep one wrapper around the whole job but stop cancelling the token.**
  Rejected as incomplete. It repairs the class but leaves the bound on the sum of
  round trips rather than each round trip (Decision 3), so one slow leg still
  consumes another's budget.
- **Accept `Cancelled` as the delivered class and change the store's literal to
  match.** Rejected. It entrenches a mis-report on the operator's only diagnosis,
  and contradicts `lib.rs:39-42`, which already defines both words.
- **Treat this as a harness race and relax the poll/queue.** Rejected. Both are
  observability artifacts (Rule 4); the product self-cancels at ~1000 ms
  regardless of how the fixture services the peer.
