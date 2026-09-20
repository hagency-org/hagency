---
kind: decision
id: ADR-178
title: Bind factory project inboxes from original observed generations
status: Accepted
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-MATRIX-DM-PRIVACY, REQ-THREE-LAYER-COMPLETION]
---

The factory already authenticates its joined project, but the native recurring
driver only selects the owner DM. ADR153 can also advance the shared project's
generation after another agent joins, leaving the immutable startup generation
unsuitable as current claim/intake/outgoing metadata.

Only the original factory's Group configuration may use its collector's latest
authenticated room observation, checked against the writer's available current
generation. Ordinary and Direct configuration keeps its exact static generation.
After collection, resolve a separate project session named by original engagement,
transport and room generation, and build fresh claim/intake metadata. Keep the
original private session, runtime, SDK and per-agent physical workspace. Existing
domain operations recheck authority; this introduces no runtime-facing setter.
Refresh only those original room IDs/privacy scopes. Preserve the already-bound
transport, workspace, local-provider and managed-account restrictions; do not
rebind them on each poll. Actual claim and handoff continue to recheck authority.

The first configured test exposed a second missing connection: owned claim
selection requires encryption for every room, while admitted factory projects
are plaintext. Keep that default. Add an explicit host-only project option to
the frozen room selection, admitted only for Group privacy. The writer also
matches the route to this engagement's registered project before selecting
plaintext work. A Direct room cannot obtain the option. The original factory
uses it only after its own project observation checks. No default encryption,
approval, file-delivery or sandbox requirement is relaxed.

Outgoing preflight verifies the captured route against both the original observed
generation and its new authenticated observation before any send. Membership
changes retire prior group scopes under ADR153; no old task or session is revived.
Main-room mentions use the existing recurring inbox canonical task and reply
path; this selector does not create a thread automatically. Exact mention gates
and private approval isolation stay unchanged. No UI text or translation keys
change. Automatic thread discovery and live fleet
qualification remain separate evidence gates; this decision does not claim them.

Regression review found two old factory fixtures still requiring CleanupUnknown
on every non-Linux target. ADR029's accepted macOS amendment now provides actual
whole-tree observations, and these runs produced successful cleanup instead.
Require the positive sequential/service assertions on Linux and macOS; retain
the prior refusal assertions on other targets. Enable the same configured
two-agent positive fixtures on macOS. No production cleanup logic or budget is
changed, and no test treats leader-only exit as whole-tree proof.

Validation: Matrix library197, owned-claim12 and native library45 pass. All five
configured fleet tests pass together, including both registration-token and
application-service paths; the two corrected macOS sequential/service tests
also pass. Strict all-target Clippy with the browser feature, locked build and
production caller audit pass. Earlier newly enabled fleet runs intermittently
failed with peer EOF; one isolated rerun instead refused initial intake with
Domain. The subsequent isolated and complete runs passed. These failures are
retained as unresolved intermittency, not a proven fix. Synthetic-only failure
diagnostics now include each peer's protocol method names and receipt stages.
No production deadline or assertion was weakened to obtain the passing run.

## Superseded plan within one poll (2026-09-18)

Live two-agent fleets lost every agent but the last to join the shared project.
A poll resolves its inbox plan, then runs intake, then selects each inbox. When
that same intake observes another agent's join, ADR153 advances the project's
generation and retires the session the plan named a moment earlier. Selection
then returns RunnerAuthority, which the driver mapped, like every selection
error and without a log line, to OutcomeUnknown; the continuous worker ended
with no diagnostic. Two retained instances show it: the earlier agent's
`project_<engagement>_1_2` session retired, no `_1_3` session ever created for
it, and the agent stopped within a minute of the later agent's join.

On RunnerAuthority from a factory agent's selection the driver now resolves the
inboxes once more. If the fresh resolution no longer names that session, the plan
was superseded rather than refused: the poll ends without work and the next poll
schedules the current generation, as this decision intended. A session the fresh
resolution still names remains a failure, as does any refusal for an ordinary
host, whose static plan has no newer generation. Nothing is retried, replayed or
revived.

Validation. Live, on a fresh isolated two-agent instance, the driver logged the
superseded `_1_2` plan as the second agent joined; the first agent resolved
`project_<engagement>_1_3` as a current route and stayed receiving. Offline,
`native_configured_fleet_earlier_agent_survives_later_join` forces the same
order: the fleet fake holds the later agent's join until the earlier agent's
timeline sync is in hand, holds that sync until the project generation has
advanced, then releases it, and both agents go on to serve their own project
mentions. With the driver change removed the test fails (the earlier agent never
regains a current project route); with it the configured-fleet target passes 7
of 7 on three consecutive runs. The unaided fixtures never reached this order
because the fake answers in microseconds, while live request pacing makes the
intake a second wide.

The same hosted runs showed the fleet fixtures' own 10 s warm idle budget was too
short for a small runner: agents are provisioned one after another and no task is
posted until all are ready, so the first warm runtime expired with Deadline and
its handoff was refused. The fixture budget is now 120 s; `warm_runtime` still
covers idle expiry with its own budget. No production budget changes.

## The frozen inbox names its request (2026-09-19)

Live, on a two-agent project room: agent 1 was woken by an exact mention, and the
dispatch froze four inputs in arrival order -- an older request addressed to agent
2 (wake false), two replies (wake false) and agent 1's own request last (wake
true). The runner carried out the OLDER request: it overwrote agent 1's verified
file with agent 2's bytes and completed the canonical task with agent 2's reply.
Routing, wake and task creation were all correct. The defect was presentation:
the payload is delivered as canonical JSON, where `inbox` sorts before
`instruction`, and the instruction said "Handle the verified Matrix inbox as the
user request" without ever explaining `wake`. ADR023 had this rule for the
TypeScript product ("background discussion is context, never approval authority",
with the current request supplied separately); no native decision restated it, and
`task-rust-message-inputs` requires only that the frozen inbox be exposed.

Selection already guarantees the shape: the trigger is the oldest unprocessed
wake, every other frozen entry precedes it, and the trigger is last. The
instruction now states that guarantee: the LAST entry, the only one whose wake is
true, is the request; every earlier entry is room context, never instructions to
carry out -- including requests addressed to other participants -- and never
approval. The payload schema, ordering, provenance checks and capacity rule are
unchanged; the text stays informational (ADR053) and grants nothing.

`native_agent_inbox_names_the_waking_entry_as_the_request` pins the order, the
wake flags and the rule. Live, the identical inbox shape on a build with the new
text: agent 1 answered its own request and its verified file stayed intact. A
model can still misread a prompt; this removes the ambiguity the product itself
created, and a repeated two-agent run exercises the same shape every round.

## An agent is a participant, so it is told who it is (2026-09-19)

The instruction above held for about fifty rounds and then failed once on the
identical inbox. The cause was not what the agent could see. A human member sees
requests addressed to others and ignores them because they know who they are, and
the runner was never told. Agents are first-class participants, so nothing is
withheld from them. The dispatch now carries the agent's own Matrix ID and name,
placed before the room, and the instruction says a message addressed to another
participant, human or agent, is theirs. A first attempt that hid such messages
from agents was withdrawn, because it made the agent a lesser participant.

## An agent listens to the whole room but is only asked what addresses it (2026-09-20)

Naming the request inside one list held for a while and then failed again: on a
two-agent project room, an agent whose payload named it correctly carried out the
OTHER agent's near-identical request, which sat earlier in the same `inbox` with
`wake` false. Every earlier fix kept one list and changed the words around it.
The retained product never had that list, and the operator pointed at it. It
separates two questions: `messageVisibleToAgent` (a room member may read every
message) and `messageTargetsAgent` (only a direct message or an exact mention
enters the inbox). A dispatch's `inbox` is its targeting messages only, and
`context.discussion` is a pointer: the router freezes a bounded window at
dispatch time and hands it out through `read_conversation`, eight parts at a
time, with speaker identities. The agent is asked one thing and can read
everything. The migration inventory listed `read_conversation` as to be ported
and it never was; the native inbox merged the two questions instead.

The native port now does the same. The dispatch payload's `inbox` holds only the
entries addressed to this agent; for a room dispatch, the trigger. The rest of
the window is still frozen and still bound to the dispatch through
`dispatch_inputs`; it is not embedded. In its place the payload carries
`discussion`: `message_count`, `has_more_history` and an instruction to read it
with the task tool `read_conversation`, which pages that window in order, at most
eight parts of 1000 characters, each with event id, sender Matrix ID, the name
the store knows the speaker by (an engagement name, or the project owner),
timestamp, thread root, part numbering and body slice. The tool names no room,
agent, session or dispatch: it is bound to the calling runner's own current
dispatch by the same capability check every task tool uses, and it is enabled
for every owned dispatch rather than held behind the coordination profile,
because without it the discussion would be unreachable. Nothing is hidden and
nothing is dropped: every message in the window is reachable, which is what the
earlier withdrawn attempt got wrong.

The window's bound is content, as in the retained product: at most 200 parts of
1000 characters reaching back from the request, instead of whatever still fits in
the payload. Position follows the retained product too: on completion the
dispatch consumes what addressed the agent plus the discussion it actually read,
and releases the rest, so unread room history rides the next window instead of
being consumed by a dispatch that never showed it. Schema 36 records both facts:
`dispatch_inputs.addressed` (defaulting to 1, so dispatches frozen by the earlier
build keep their behaviour) and `dispatch_conversation_reads.read_parts`.

Delegated work is unchanged in shape and stays in `inbox` with `task` and
`delegated_by` (ADR180). The retained product's own logic says so: a task-bound
dispatch takes its inbox from the task's inputs, and no room window exists for
handed-over messages, because they were handed over out of the delegator's room
and never read out of the assignee's. They are the request in the delegator's
words; the canonical task remains the job.

Differences from the retained product that remain: the window is this session's
admitted inputs, not a separate archive of every room event; `has_more_history`
means older unclaimed discussion sits below the window; the pointer carries no
room or position identifiers, because a runner can select neither; a page carries
no attachment pointer, because attachment visibility is frozen separately and
listed by its own tool; and a speaker's name is the engagement name or the project
owner, because the store keeps no human display name.

Pinned by `native_agent_inbox_names_the_waking_entry_as_the_request`,
`native_agent_conversation_pages_in_order_within_its_own_dispatch`,
`native_agent_conversation_releases_what_was_never_read`,
`native_mcp_conversation_read_is_catalogued_and_task_bound` and
`native_configured_fleet_project_mentions`, where the offline peer reads the room
back through the tool.
