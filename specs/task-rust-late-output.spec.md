---
spec: task
name: "Record fenced late runner output as non-authorizing evidence"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-TSS-FENCE]
tags: [active, rust, runner, custody, wiring]
---

## Intent

Close the one real new gap from the 2026-09-14 wiring audit for the owned-dispatch
lane: `record_late_output` (`hagency-store/src/domain/execution.rs:982`), whose only
caller is its own facade `DomainWorker::record_late_output`
(`hagency-store/src/domain_worker.rs:2308`, body at `:2314`) — no production function
reaches it.

**A correction to the audit premise, stated because it changes the shape of the fix.**
The brief's sibling `complete_dispatch` (`execution.rs:964`) is *not* the wired lane:
its only caller is likewise its own facade (`domain_worker.rs:2304`), so it is a
test-only legacy writer, not a model to copy. The lane that actually carries
production completions is the **owned** pair — `complete_owned_dispatch`
(`domain_worker.rs:1489`) → `complete_owned_clock` (`owned_dispatch.rs:398`) →
`complete_in_transaction` (`execution.rs:480`), reached from
`hagency_execution::operation::execute` (`hagency-execution/src/operation.rs:686`,
completion at `:1156`). The legacy `record_late_output` therefore sits on a
legacy/test-only writer family, and the owned family has **no** late-output writer at
all (verified: `owned_dispatch.rs` contains no `record_late_output`, and its only
`accepted=0` insert is `observe_owned_failure`'s failure evidence at `:477`).

This is a **wire-it** gap, not a delete decision: [REQ-TSS-FENCE]
(`knowledge/requirements/req-thread-scoped-agent-sessions.md:75`) makes the write a
MUST — *"Output from a runner whose dispatch is no longer current MUST be recorded as
fenced and MUST NOT settle any dispatch"*. The store already implements exactly that
contract; what is absent is a production route that reaches it. The scenario "Durable
fencing rejects late output after restart"
(`specs/task-thread-scoped-agent-sessions.spec.md:521`) states the same obligation but
is bound to `Test: test_fencing_generation_survives_restart`, which **does not exist in
the tree** (verified: `git grep -n 'fn test_fencing_generation_survives_restart' HEAD -- 'native/*'`
returns nothing). This slice states the Rust-side owed selectors for the obligation,
with no `Test:` line until the route lands.

## Constraints

- **No `Test:` line for an unwired obligation.** A `Test:` naming a nonexistent test
  fails the Rust binding gate; these scenarios use `Owed Selector:` only.
- **The store write is not weakened.** The route is added *around* `record_late_output`;
  its authentication, its no-clock property and its `accepted=0` verdict are the
  contract, not obstacles to it.
- **No ADR is allocated here.** The gap id `G10` is the wiring-audit gap number for
  `record_late_output` in ADR-146's new-gap row, not a decision number.
- The production caller is **not** invented. No production function reaches
  `record_late_output` on this head; the one honest `Production caller:` value is
  `owed (G10)`.

## Boundaries

In: the fence/retention semantics of a late submission and the production-caller
obligation for the route that will reach `record_late_output`.

Out: the route's own HTTP/ingress shape (a separate wiring slice), the sibling
`create_coordinator_task` gap (ADR-146), whether the late writer belongs on the legacy
`RunnerCapability` family or a new owned-lane writer, physical workspace or process
custody, and any change to `complete_dispatch`'s or the owned completion lane's
settled behavior.

## Acceptance Criteria

### When late output actually occurs

Two production facts bound the window, and neither is an assumption:

1. **Every completion route passes `&["started"]`.** `complete_owned_clock`
   (`owned_dispatch.rs:398`) and `finish_task_clock` (`owned_completion.rs:83`) both
   scope through `started`, and `authorize_attempt` (`execution.rs:162`) refuses any
   dispatch whose `state` is not in the passed set. Once `complete_in_transaction`
   (`execution.rs:480`) records the accepted row (`accepted=1`, `:493`), the dispatch
   leaves `started` and the same capability can no longer attach output — it is
   refused `Error::RunnerAuthority`, never retained.

2. **The child is not synchronously reaped at the completion decision.**
   `Operation` retains `report.late_child` and adopts a child that arrives *after* the
   completion path has moved on (`operation.rs:516-526`), and
   `native_owned_dispatch_spawn_outliving_budget_is_fenced`
   (`specs/task-rust-owned-dispatch.spec.md:129`) exists precisely because a child can
   outlive its budget. So a runner can still be producing when its dispatch is already
   settled.

The concrete production occurrence is therefore the **post-fence / post-restart
submission** [REQ-TSS-FENCE] names: a capability for an older fence generation
survives the process (or the backend restarts while a child lives) and presents output
to a backend whose dispatch row has moved on. The arrival is real; the **route is
absent**, so today the output is refused and dropped rather than recorded. The routed
runner surface (`hagency/src/runner.rs:23-46`) admits no such endpoint, and no
production source outside the two facts above mentions a late path. That is the gap.

### What the store already guarantees (read, not weakened)

`record_late_output` (`execution.rs:982-1013`) authenticates and writes:

- **Authenticates against the attempt row, not the dispatch state.** It reads
  `runner_attempts` for `(dispatch_id, fence)` (`:994`), requires the exact
  `runner_id` match and `matches_secret(&hash, &cap.secret)` (`:996`), else
  `Error::RunnerAuthority`. This is the same check as `historical()`
  (`owned_completion.rs:23`) and `observe_owned_failure` (`owned_dispatch.rs:459`).
- **Takes no clock.** The signature has no `now` parameter — deliberate, so the write
  authenticates even after expiry/revocation. ADR-053 D-7 pins `runner_attempts`
  precisely because *"both late paths authenticate against the attempt row with no
  clock"* (`execution.rs:1262`).
- **Bounds capacity.** Output over 32 KiB refuses `Error::Capacity` (`:988`); a 129th
  row for the same `(dispatch_id, fence)` refuses `Error::Capacity` (`:1004`).
- **Writes unaccepted evidence.** `INSERT INTO runner_outputs(dispatch_id,fence,output,accepted)
  VALUES(?1,?2,?3,0)` (`:1008`) — `accepted=0`, which is what makes it fenced
  display-only evidence that settles nothing. The row is not invisible to retention:
  `CARRIES_EVIDENCE` (`execution.rs:1234`) counts non-accepted rows, so the row is
  retained evidence with a bound, and the prune deletes all but the newest accepted row
  per fence (`:1370`).

The wiring must reach *this* write. Weakening the authentication or the no-clock
property to make a route easier would satisfy [REQ-TSS-AUTHORITY-SEPARATION] in name
only.

Scenario: A fenced late submission is retained as unaccepted evidence
  Owed Selector: native_late_output_records_fenced_evidence (parked — owed by the G10 wiring slice; no Test: line here yet)
  Given a capability whose attempt row authenticates and whose dispatch has left started
  When the runner submits output through the late route
  Then the runner_outputs row carries accepted=0 for that dispatch_id and fence
  And no dispatch is settled by it
  Production caller: owed (G10)

Scenario: A late submission from a different runner refuses without a row
  Owed Selector: native_late_output_refuses_foreign_attempt (parked — owed by the G10 wiring slice; no Test: line here yet)
  Given a capability whose runner_id does not match the attempt row for that dispatch_id and fence
  When the runner submits output through the late route
  Then the route refuses with runner authority and writes no runner_outputs row

Scenario: A late submission that exceeds the per-attempt bound refuses
  Owed Selector: native_late_output_refuses_at_capacity (parked — owed by the G10 wiring slice; no Test: line here yet)
  Given an attempt row whose runner_outputs rows for one dispatch_id and fence number 128
  When the runner submits further output through the late route
  Then the route refuses with capacity and the existing rows are unchanged

Scenario: A capability surviving a restart still authenticates against its attempt row
  Owed Selector: native_late_output_survives_restart (parked — owed by the G10 wiring slice; no Test: line here yet)
  Given a capability for an older fence generation that survives outside the process
  When it submits output after the backend restarts
  Then the runner_outputs row records the submission as unaccepted evidence for that fence
  And no current dispatch settles

Scenario: The late route reaches the store write from production
  Owed Selector: native_late_output_route_has_production_caller (parked — owed by the G10 wiring slice; no Test: line here yet)
  Given the late-output route registered on the routed runner surface
  When the production call graph is computed with tests, fixtures and the bootstrap probe stripped
  Then the route is reachable from a product root and its handler reaches record_late_output
  Production caller: owed (G10)

## Decisions

**The gap is wire-it, not delete.** The only reading under which `record_late_output`
is dead code is one in which [REQ-TSS-FENCE] is satisfied some other way. It is not:
the requirement is a MUST, the store implements it, and the scenario that states it is
bound to a test that does not exist. Deleting the write would delete the implementation
of a standing requirement.

**`Production caller: owed (G10)` is the only honest value.** No production function
reaches `record_late_output` on this head — `grep -rn '\.record_late_output(\|::record_late_output('
native --include='*.rs'` returns only `domain_worker.rs:2314`, the facade's own body.
Naming a would-be caller would fabricate the one thing this spec exists to check. `G10`
is the ADR-146 new-gap row id for this method, so the production-caller checker resolves
it (the checker reads `gap Gn` from ADR-146's table rows).

**The wired sibling is the owned lane, not `complete_dispatch`.** Wiring the late route
must not be modeled on the legacy `complete_dispatch`, which is itself test-only. The
owned lane's shape (attempt-row authentication, no clock, `accepted=0` evidence) is the
contract to preserve.

## Out of Scope

The route's ingress shape and authorization surface, `create_coordinator_task` (the
other ADR-146 new gap), `observe_owned_failure`'s failure fencing, the retention
sweep's prune ordering, and any change to the wired owned completion lane.
