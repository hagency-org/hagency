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

## Delivery of a delegated task (2026-09-20)

The lane named above as unported is now wired for inline factory agents. It was
dead in five places, not one: `create_intent` resolved an unverified assignee
session even for a verified delegator, so the notice had no verified route; the
verified claim was global, so any agent could take any agent's notice; nothing in
the fleet service claimed a notice at all; an activated intent had no selection
that would mint its dispatch; and the driver never looked at delegated sessions.

A delegation created from a verified dispatch now resolves a verified assignee
session and a verified notice route. The assignee's own driver claims only
notices whose route names its own engagement, at most four per attempt, and
posts them with the final reply's custody: any result other than Delivered is
unknown and ends the attempt, one claim is never sent twice, and the next
attempt's outgoing resume settles what was journaled before any new work. The
plan this came from let a Generation refusal skip the notice and carry on; that
was not adopted, because the send path can return Generation after its journal
entry exists, and carrying on would schedule new work over an unreconciled send.
A notice whose route is no longer current is never claimed and never replayed,
as with any group work retired by a membership change (ADR178).

Matrix accepting the notice activates the intent. The driver then lists its own
engagement's active delegated sessions, at most sixteen, and selects each: the
selection mints the dispatch the intent's own task was waiting for and never
creates a task. The two filters the ordinary agent inbox applies are absent, for
different reasons. Event provenance is recorded under the engagement that
admitted the event, which is the delegator's, so it would refuse every handed-over
message at any clock. The route's ingress floor is the point from which the
assignee could see the room, not a delegation clock: it usually sits below the
handed-over message and would look harmless, until an assignee whose transport or
room was observed after the request was sent silently lost the delegation. The
delegation's own authority takes their place: an owner-approved, activated intent
whose task inputs are exactly the messages `delegate_task` proved the delegator
could see. A verified delegated session holds its own durable copy of each of
those messages, written when they are projected into it; the first draft left
that copy empty, which every verified reader refuses.

The assignee is a first-class participant. Its delegated inbox shows those
original messages as every participant sees them, the payload names the agent
as it does for any inbox, and the instruction says plainly that the messages
are addressed to the delegator and that the task is the job.

The first live delegation (2026-09-20) was delivered and then done wrong. The
payload held the assignee's identity and one inbox entry, the owner's message to
the delegator, which told its reader to call `delegate_task`; the task's title and
description were reachable only through a tool. The assignee carried out the
message: it called `delegate_task` again and reported the refusal as the task's
result. A human assignee has the task card in front of them and knows who handed
it over, so the payload now carries `task` (id, title, description) and
`delegated_by` (Matrix ID and name, read from durable rows rather than the
delegator's current route), and the instruction leads with them. Nothing was
removed from the inbox.

Still owed: follow-up messages in the delegated thread are not admitted, the
delegator receives no completion report, and an ordinary single-agent host runs
none of this. Because the notice is sent on the attempt's fatal path with the
final reply's custody, a notice send that is refused or unknown ends the
assignee's worker, not only that attempt; that is the existing fail-closed rule
and is not softened here. `native_configured_fleet_delegated_task_delivery`
carries one delegation through two composed agents: approval, one `m.notice`
under the assignee's own identity threaded on the delegator's question, the
activated intent, a single dispatch on the assignee's delegated session, and the
delivered reply, all inside one attempt of the assignee. A live run is the
remaining evidence.

## Amendment: follow-ups in a delegated thread reach the assignee (2026-09-22)

The retained product routes a thread message that mentions the assignee to the
task bound to that thread (`findThreadTaskBinding` -> `attachTaskInputs`), and
lets only the original requester reopen a completed one. The native store
already did both at admission (`bound_intent`, the after-done rule), and the
intent selector already minted another dispatch for the same task when the
delegated session held an unprocessed input; but the assignee's intake never
targeted its delegated sessions, and a threaded event is admitted only through
the session bound to its thread, so the owner's follow-up in a delegated
thread was never admitted at all. A factory agent's intake plan now carries
its active delegated sessions (`intent_sessions`) beside its own.

Not added, on purpose: a completion report to the delegator. The retained
product has none. The assignee's final reply is posted as the assignee in the
shared thread, agent messages never wake another agent, and the delegator
learns by an explicit peer reply, by reading the task it created, or by reading
the thread later. Pinned by
`native_delegated_thread_followup_continues_the_delegated_task`.

The first live run with the plan change found the second half. The follow-up
was admitted, a second dispatch for the same task was minted and ran, and the
assignee did nothing, three times: the dispatch reused the handed-over
instruction, which says the inbox messages are addressed to the delegator and
not to this agent and must be read for context only. That is right for the
delegator's own words and wrong for the owner's follow-up. A delegated
dispatch now marks an entry this agent read from its own room as `follow_up`
(an ingress event of this engagement names it; a handed-over row has none),
and when one is present the instruction says to carry the follow-ups out as
part of the task while the delegator's words stay context only. The mark is
absent from every other payload.

### The delegator reads what it created (2026-09-22, operator decision)

The retained product's `get_task(id)` and `list_tasks` show a runner every
task bound to its own session's dispatches, so an agent that delegated reads
the task it created. Natively the service already exposed that (`visible`,
`runner_tasks`: the assigned task and the ones whose `creator_session_id` is
the session's), but the helper refused any id but the assigned one and served
no list. On the operator's "follow TS": `get_task` takes an optional id (the
assigned task when omitted) and `list_tasks` pages the visible set by id;
both read-only, both in the fixed task-tool set every runtime is configured
with, the helper naming the id and the service deciding visibility. No new
mutation, no completion report. Pinned in
`native_mcp_coordination_delegation`; the offline Codex and Claude peers
enumerate the widened set.
