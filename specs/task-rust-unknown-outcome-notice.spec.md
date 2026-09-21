spec: task
name: "Say an unknown outcome in the thread and keep a recoverable agent visible"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREE-LAYER-COMPLETION]
tags: [active, rust, matrix, fleet, recovery]
---

## Intent

One turn the provider failed ended a live soak in silence: the dispatch was fenced
with an unknown outcome, the room saw only an agent that stopped answering, and
the fleet reported that agent as lost although its worker was alive and waiting
for the operator. The retained product says so in the thread
("Result uncertain…", `settleUnknownInternal`) and keeps the agent up. Do the same
with what the native port already has: the fence, the verified notice lane and the
worker's wait for the operator's resolution.

## Constraints

- Queue one notice, in the retained product's words, when the failure of a
  started run is recorded: for a verified session only, rooted at the request the
  dispatch was answering, once per task. A dispatch with no task, no request or
  no verified session says nothing. Never queue it in the fence itself: a
  successful completion retires its dispatch through the same fence.
- Recording a failure never fails because its notice could not be addressed. The
  attempt is its own savepoint; a refusal undoes only itself.
- The notice lane admits an ordinary room request. A delegated task's notice still
  lives and dies with its intent; a task with no intent at all is current on its
  task epoch and route alone. Nothing else about notice custody changes: claims
  stay scoped to the agent's own engagement, a stale route is never claimed, one
  claim is never sent twice.
- After a failed attempt the agent's own driver posts what is queued, best effort,
  before it waits for the operator or gives up. A refused send changes nothing
  about the failed attempt.
- A worker that waits for the operator says so: its status carries
  `awaiting_operator`, keeps reporting the failure beside it, and clears both with
  the next attempt. The fleet counts an agent as lost only when it failed and is
  not waiting.
- No new recovery authority. The operator's resolution, the stop inspection and
  the quarantine are unchanged. Later requests in a waiting session are not
  answered with the retained product's "Waiting…" notice in this slice.

## Allowed changes

- native/hagency-store/src/domain/task_intents.rs
- native/hagency-store/src/domain/owned_dispatch.rs
- native/hagency-store/src/domain/notice_custody.rs
- native/hagency-store/tests/received_files.rs
- native/hagency/src/bootstrap.rs
- native/hagency/src/bootstrap/driver.rs
- native/hagency/src/bootstrap/notice.rs
- native/hagency/src/bootstrap/fleet.rs
- knowledge/decisions/adr-045-native-notice-custody.md
- knowledge/decisions/adr-162-native-stopped-owner-inspection.md
- specs/task-rust-unknown-outcome-notice.spec.md
- docs/**

## Scenarios

Scenario: An unknown outcome is said in the thread once, by the agent it belongs to
  Test: native_outcome_unknown_is_said_in_the_thread_once
  Production caller: hagency::bootstrap::notice::deliver
  Given an ordinary room request whose started dispatch fails
  When the failure is recorded
  Then one pending verified notice carries the retained product's words
  And the task has no delegation record, yet the notice is current and only its own agent's pump can claim it
  And observing the same failure again adds no second notice

Scenario: A recoverable agent is visible as waiting and is not counted as lost
  Test: native_factory_failure_diagnostics
  Given a fleet with one failed agent and one healthy agent
  When the failed agent's attempt is recoverable and it waits for the operator
  Then its status keeps the failure and carries awaiting_operator, and the fleet is not failed
  And an agent that failed and is not waiting still makes the fleet failed, and the next attempt clears the wait
