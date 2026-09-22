spec: task
name: "Wait for the owner to join the new agent's room, without a deadline"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, matrix, provisioning, fleet]
---

## Intent

A new agent's DM is encrypted, so the agent's keys can only be shared once the
owner has joined it. That wait was bounded by a single request's budget, at
most sixty seconds; a slower human left the provision uncertain forever and
ended the coordinator. The retained product never waited for the owner at all.
Waiting for a person is a state, not a timeout: keep the provision Started,
look again on every coordinator turn, show the wait, and finish the agent when
the owner has joined.

## Constraints

- An attempt whose rooms exist (DM created, owner invited, agent joined) and
  whose owner has not joined within the attempt's own budget ends as
  awaiting-owner. It retires no room, observes no unknown outcome, and leaves
  the effect Started with its rooms custody resumable, GET-only, by the job
  that observed the wait. Six records without `complete` seen by any other
  job (a lost record, a restart) stay unknown, as before.
- The intake counts awaiting-owner as success for the approval it handled, and
  before every later read of the room gives each waiting provision one look:
  a resumed attempt polls once, within two seconds, and hands the wait back.
  Any other refusal on a resumed attempt is the provision's own, as inline.
- After the owner has joined, the same turn finishes rooms, enrollment and the
  factory exactly as the first attempt would have; nothing is created, invited,
  joined or registered again.
- The fleet publishes each waiting provision as `awaiting_owner` with the
  wall-clock millisecond it began waiting, replaces the row with the agent on
  admission, and drops it if the provision stops waiting without admission.
  The row decides nothing: the fleet is not failed and readiness is unchanged.
- No reminder is sent to the owner. A lost or torn POST is still unknown and
  never repeated. The SDK budget is unchanged: each look is one bounded read.

## Allowed changes

- native/hagency-matrix/src/lib.rs
- native/hagency-matrix/src/token_provision/rooms.rs
- native/hagency-matrix/src/provisioning.rs
- native/hagency-matrix/src/provisioning/factory.rs
- native/hagency-matrix/src/intake.rs
- native/hagency-matrix/tests/provision_rooms/mod.rs
- native/hagency/src/bootstrap.rs
- native/hagency/src/bootstrap/fleet.rs
- native/hagency/tests/inline_factory.rs
- native/hagency/tests/inline_factory/mod.rs
- knowledge/decisions/adr-147-provisioning-verdict-effect-route.md
- specs/task-rust-owner-join-wait.spec.md
- specs/task-rust-inline-agent-rooms.spec.md
- docs/**

## Scenarios

Scenario: The owner joins after the first attempt's whole budget and the agent still finishes
  Test: native_provisioning_waits_for_the_owner_without_a_deadline
  Production caller: hagency::bootstrap::fleet::Service::reconcile_awaiting_owners
  Given a provision whose owner joins the DM only after the SDK budget has passed
  When the intake handles the approval, the fleet runs, and later coordinator turns look again
  Then the effect stays Started and the fleet shows awaiting_owner with the time it began, not failed
  And after the join a turn finishes the agent, which takes the waiting row's place and runs its first task, registered exactly once

Scenario: Running out of the budget is awaiting-owner, not unknown, and resumes without a new POST
  Test: native_provisioning_inline_rooms_refusals
  Given a rooms attempt with the owner absent
  When the attempt's budget runs out, a turn passes with the owner still absent, then the owner joins and another turn passes
  Then the intake succeeds each time, the effect stays Started, the absent-owner turn looks once, and the turn after the join finishes rooms and enrollment with no create, invite or join repeated

## Out of scope

A restart during the wait (the Started effect is the operator's, as today), a
reminder to the owner, and starting the agent in the project room before its
DM is enrolled remain separate.
