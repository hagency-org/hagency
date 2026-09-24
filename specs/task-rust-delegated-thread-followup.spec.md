spec: task
name: "A follow-up in a delegated thread continues the delegated task"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, matrix, delegation, fleet]
---

## Intent

The retained product routes a thread message that mentions the assignee to the
task bound to that thread (`findThreadTaskBinding` -> `attachTaskInputs`), so
the owner can follow up on a delegated task in its own thread. The native
store already attaches such a message to the delegated task and selects it
again for the assignee; what was missing is that the assignee never targeted
its delegated sessions in its intake, so a threaded follow-up was never
admitted through them. The retained product has no automatic completion
report to the delegator: the assignee replies in the shared thread, and the
delegator learns by an explicit peer reply, by reading its created task, or by
reading the thread later. None is added here.

## Constraints

- A factory agent's intake plan carries, beside its own sessions, its active
  delegated sessions: intents in state active whose task is not done and whose
  route is current. Projection only; admission re-checks everything.
- A follow-up admitted through a delegated session attaches to the delegated
  task and is selected for the assignee through the intent selector, minting
  a dispatch that carries the same task; nothing mints a second task.
- In that dispatch a follow-up is marked as such (`follow_up`): an entry this
  agent read from its own room, as opposed to one handed over from the
  delegator, and the instruction says to carry follow-ups out as part of the
  task while the delegator's own words stay context only. Live, shown a
  follow-up under the handed-over instruction, the assignee did nothing.
- After the delegated task is done, only the sender of its root message wakes
  it again, as the retained product allows only the original requester to
  reopen a completed task.
- No completion report to the delegator. The delegator reads the task it
  created through `get_task` by id and `list_tasks` (read-only; the service's
  existing visibility decides, the helper selects nothing) — the retained
  product's own two tools, added 2026-09-22 on the operator's "follow TS".

## Allowed changes

- native/hagency-core/src/messages.rs
- native/hagency-store/src/domain/messages.rs
- native/hagency-store/src/domain_worker.rs
- native/hagency-store/tests/delegated_intents.rs
- native/hagency/src/bootstrap/driver.rs
- native/hagency/src/mcp.rs
- native/hagency/src/mcp/catalog.rs
- native/hagency/src/task_client.rs
- native/hagency/src/task_client/transport.rs
- native/hagency-runtime/src/task_mcp.rs
- native/hagency/tests/**
- native/hagency-runtime/tests/claude_task_mcp.rs
- knowledge/decisions/adr-180-codex-coordination-profile.md
- specs/task-rust-delegated-thread-followup.spec.md
- docs/**

## Scenarios

Scenario: A follow-up in the delegated thread continues the delegated task
  Test: native_delegated_thread_followup_continues_the_delegated_task
  Given an active delegated intent whose first dispatch the assignee completed
  When the owner posts a follow-up mentioning the assignee in the delegated thread and it is admitted through the delegated session
  Then the follow-up belongs to the delegated session, addresses the assignee, and is attached to the delegated task
  And the delegated session waits for a dispatch again and the next intent selection carries the same task and the follow-up
  And the follow-up entry is marked follow_up and the instruction says to carry it out, while the first dispatch keeps the handed-over instruction

Scenario: The delegator reads the task it created
  Test: native_mcp_coordination_delegation
  Production caller: hagency::runner::list_tasks
  Given a delegation this session's dispatch created
  When the delegator calls get_task with the new task's id and list_tasks
  Then the created task is returned by id, the list holds the assigned task and the created one in id order, and a task id outside the session is refused

## Out of scope

A completion report to the delegator (not parity), the retained product's
"This task is complete…" reply to a follow-up after completion, and the
delegator reading its created task through a tool remain separate.
