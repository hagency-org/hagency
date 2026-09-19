---
kind: decision
id: ADR-180
title: An opt-in coordination tool group for owned Codex dispatches
status: Accepted
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-OWNER-UI-APPROVAL]
---

Live, a Codex agent asked to delegate answered that `delegate_task` is unavailable.
The helper already declares and serves the coordination tools (ADR051) behind the
runner capability, which confines them to the caller's fleet and project; Codex
simply never sees them, because `enabled_tools` admits only `TASK_MCP_TOOLS` and
the gated file tools (ADR057, ADR101, ADR105). Operator decision: let owned Codex
dispatches call delegation, task comments and the conversation and peer-message
tools behind an explicit host option. Graph tools stay out for now.

## Decision

A host option, `coordination_tools` in the driver configuration and
`with_coordination_tools()` on the execution host, factory and task-helper
profile, adds exactly eight names to the Codex `enabled_tools`: `comment_task`,
`delegate_task`, `open_conversation`, `get_conversation`,
`update_conversation_members`, `close_conversation`, `send_peer_message` and
`read_peer_inbox`. It is off by default, and the Claude owned profile is unchanged.

No environment marker is added. ADR101 and ADR105 needed one because the helper hid
file tools by default; it has always served these, so there is nothing to reveal and
no new value to reserve. Authority is unchanged: every call still needs the current
runner capability, fence and lease, and the store still requires caller and assignee
to be active engagements of one fleet and project.

Approval follows ADR-021. `comment_task` is one of the six task tools ADR-021
pre-approves, so it joins the pre-approved set when the group is on. The other seven
reach other sessions: they are optional tools and are never pre-approved. Each call
raises an MCP elicitation that the existing owner coordinator turns into an
approval card, exactly as a file send does.

One store defect had to be fixed for delegation to work at all. `delegate_task`
documents "omitted root uses the canonical source", but a task minted from a
verified agent inbox has no intent, so an omitted root found no source and every
delegation from a Matrix request was refused (RunnerAuthority, HTTP 403). Its source
is the waking entry, which selection binds last to the dispatch; that is now the
root. The visibility check on the root is unchanged, and a task with no bound input
still has no source.

## Live result (2026-09-19)

With the option on: the agent called `delegate_task`; the call became an owner
approval; "Approve once" in Robrix moved it to applying; the store created the
intent with the waking entry as root and a canonical task for the assignee, where
the same call had been a 403; the delegator replied with the new task's ID and
completed its own task. The fleet stayed healthy.

The delegated task was then never delivered. Minutes later its intent and task
notice were still pending and unclaimed and no dispatch existed:
`claim_task_notice` and `deliver_task_notice` have no production caller in the fleet
service. That lane is unported work and is not part of this decision. Until it
exists, delegation creates durable pending work that no assignee starts.

Also seen while qualifying this, and not decided here: an approval the owner does
not answer ends the agent (outcome-unknown, owner retained) when the owner wait
expires; there is no tool with which an agent can learn a peer's engagement ID; a
refused argument shape returns only "invalid native command or runner context",
which gives a model nothing to correct; and peer messages have no production wake
lane, so they reach only dispatches that are already running.

## Verification

`native_task_mcp_coordination_configuration` pins the eight names, the unchanged
environment, `comment_task` as the only added pre-approval and the absence of graph
tools. `native_agent_inbox_task_delegates_from_its_waking_entry` pins the root of a
delegation from an inbox-minted task and still refuses a root the dispatch cannot
see; it fails with RunnerAuthority without the store change.
