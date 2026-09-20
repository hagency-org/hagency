spec: task
name: "Deliver a delegated task to the agent it was assigned to"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREE-LAYER-COMPLETION]
tags: [active, rust, matrix, fleet, coordination]
---

## Intent

ADR180 lets a Codex agent call `delegate_task`, and the owner's approval creates a
durable intent, a canonical task and a task notice for the assignee. Nothing then
delivered it: the notice had no production claimer, the delegated session was
unverified, and no selection ever minted a dispatch for an active intent, so an
approved delegation stayed pending forever. Close that lane for inline factory
agents: the assignee posts its own task notice, Matrix accepting it activates the
intent, and the assignee's own driver dispatches the delegated task.

## Constraints

- The assignee is a first-class participant. Its delegated inbox shows the
  original request messages exactly as every participant sees them; nothing is
  hidden or rewritten, and the payload names the agent as it does for any inbox.
- A delegation created from a verified dispatch resolves a verified assignee
  session and a verified notice route, and that session holds its own durable
  copy of each handed-over message. An unverified creator keeps the existing
  unverified path unchanged.
- An agent claims only notices whose verified route names its own engagement, and
  selects only its own delegated sessions. A notice whose route is no longer
  current is never claimed and never replayed.
- Notice custody is the final reply's: any result other than Delivered is
  unknown and ends the attempt, one claim is never sent twice, and the next
  attempt's outgoing resume settles what was journaled before new work.
- At most four notices per attempt. Selection never creates a task; it mints the
  dispatch the intent's own task was waiting for, only after activation.
- Inline factory agents only. An ordinary single-agent host owns no engagement a
  notice could be scoped to and runs none of this.
- Follow-up messages in the delegated thread, a completion report to the
  delegator, and a peer-message wake lane are outside this slice.
- Offline tests use synthetic local peers only. No live-account mutation in
  Cargo tests, no dependency change, no production cutover.

## Allowed changes

- native/hagency-store/src/domain/task_intents.rs
- native/hagency-store/src/domain/notice_custody.rs
- native/hagency-store/src/domain/messages.rs
- native/hagency-store/src/domain_worker.rs
- native/hagency-store/tests/received_files.rs
- native/hagency-store/tests/delegated_intents.rs
- native/hagency/src/bootstrap/driver.rs
- native/hagency/src/bootstrap/notice.rs
- native/hagency/tests/configured_fleet.rs
- native/hagency/tests/configured_fleet/**
- native/hagency/tests/fixtures/owned_mcp_peer.rs
- knowledge/decisions/adr-180-codex-coordination-profile.md
- knowledge/decisions/adr-146-production-callers-and-store-surface.md
- specs/task-rust-delegated-task-delivery.spec.md
- docs/**

## Scenarios

Scenario: A delegation from a verified dispatch is verified and scoped to its assignee
  Test: native_agent_inbox_task_delegates_from_its_waking_entry
  Production caller: hagency::bootstrap::notice::deliver
  Given an agent whose inbox-minted task delegates work to another agent
  When the owner-approved delegation is applied
  Then the notice carries a verified route and the assignee session is verified
  And the unverified claim finds nothing, the assignee's scoped claim finds the notice and another engagement's finds none

Scenario: A delivered notice activates the intent and its own driver dispatches it
  Test: native_delegated_intent_activates_and_dispatches
  Production caller: hagency::bootstrap::notice::schedule
  Given a delegated intent whose task notice Matrix accepted
  When the assignee selects its delegated session
  Then one dispatch is minted for the intent's own task and no new task is created
  And the assignee's claim profile claims it and a repeated selection replays

Scenario: An intent whose notice is not delivered yet is not dispatched
  Test: native_delegated_intent_is_not_selected_before_activation
  Production caller: hagency::bootstrap::notice::schedule
  Given a pending delegated intent
  When its session is selected before the notice is delivered
  Then selection reports no wake and mints nothing

Scenario: An agent selects only its own delegated sessions
  Test: native_delegated_intent_selection_is_scoped
  Production caller: hagency::bootstrap::notice::schedule
  Given active delegated intents for two engagements
  When one engagement lists the sessions waiting for a dispatch
  Then only its own sessions are listed

Scenario: Handed-over messages are authorised by the delegation, not by the assignee's own room reading
  Test: native_delegated_intent_inputs_are_handed_over_not_room_read
  Production caller: hagency::bootstrap::notice::schedule
  Given a delegated message only the delegator's engagement admitted, and an assignee whose own room-visibility floor is raised above it
  When the assignee selects its delegated session
  Then the message is still selected and reaches the assignee with its original content

Scenario: Two composed agents carry a delegation from approval to the assignee's reply
  Test: native_configured_fleet_delegated_task_delivery
  Production caller: hagency::bootstrap::notice::deliver
  Given two inline factory agents in one project with the coordination tools on and an owner who approves once
  When the mentioned agent delegates a task to the other and completes its own
  Then the assignee posts exactly one m.notice, under its own identity, threaded on the delegator's question and with no mentions
  And the intent activates, one dispatch is minted on the assignee's delegated session only, and its payload names the assignee and carries the owner's original message
  And the delegated task reaches Done with its reply delivered in that thread and both agents stay healthy
