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
