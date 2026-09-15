---
kind: decision
id: ADR-130
title: "Finite native agent lifecycle authority: start, stop and preset behind one scope"
status: Proposed
requirements: [REQ-RUST-MIGRATION-EXECUTION]
tags: [console, agents, lifecycle, scope, authority]
---

## Context

The retained console starts, stops and preset-binds agents, and each act needs a
host process on the machine the backend runs on. Native has no agent
**lifecycle** record — no registry, no `online`/`manualDown` — though
engagements carry `agent_name` (`hagency-core/src/project.rs:260`) and CL-S1's
roster read (`adr-126`) now makes the fleet observable. ADR-053:23 closes the
door on the obvious port: *"There is no second launcher."* The store's stop
kernel already exists and is exercised only by transport retirement
(`conversation_lifecycle.rs:54-102`, `matrix_routes.rs:66`): fence-and-record,
never settle — settlement is the host's, by `settle_conversation_stop`'s own
comment. What no surface owns is the operator's lifecycle act itself, and the
operator decision **D-SCOPE** (now in force) settles where that authority
lives.

## Decision

**One finite scope owns the agent lifecycle: `Scope::AgentLifecycle`.** Not the
configure scope, not a widened existing one — a third scope of the console
authority's own, minted exactly like the others and reaching exactly three
acts: **start, stop, and preset-apply**.

**Stop — fence, never settle; the store's verdict.** The route resolves the
named engagement's dispatch through the live set (`queued`, `leased`,
`started`, `parked` — `queued` because the fence kernel accepts it,
`conversation_lifecycle.rs:65`) **or** an unsettled `dispatch_stops` row
(`pending_conversation_stops`'s read, `:288`), newest by id, so a second call
is idempotent. It fences **only the resolved dispatch** — not `retire`'s
session cascade, which closes child conversations (`:125-150`); that widening
is a later, deliberate slice. The wire object is exactly five keys —
`stopped`, `stop_pending`, `dispatch_id`, `fence`, `state` — with refusals in
the console's existing `{"ok":false,"code":…}` envelope. `stopped` is true
only on a **settled** stop row, and no production path settles today
(`settle_conversation_stop` has no caller outside store tests), so an honest
stop reports `stop_pending` until the host that owns the process proof
settles it. A route is a runtime-facing command; it may never call the
settlement.

**Start — an ensure, never a launcher.** "Start" is the lifecycle act of
making an agent's engagement dispatchable again (releasing the parked stop,
re-arming the driver's queue participation) — an idempotent **at-most-once**
ensure against the store's own state word, refused with a named code when the
agent is already live. It spawns nothing: ADR-053's fixed launcher is the
owned-dispatch host's, and this scope grants no power to construct a child
process, an argv, or a workspace.

**Preset-apply — a pointer, not a second editor.** Applying a preset under
this scope is the act of pointing the agent at an **already-published**
preset id; the preset's own fields (profile, ceiling) remain the configure
scope's act, unchanged. The apply is bounded: one pending apply at a time,
refused for an unknown or unpublished preset id, and it never invents or
widens a field the store does not hold.

**D-SCOPE, in force — the scope's own rules.**

- **Name:** `Scope::AgentLifecycle`; the CLI grant flag is
  `--manage-agent-lifecycle`, mutually exclusive with both existing
  management flags (declared and asserted pairwise — a declaration is not a
  test).
- **Lifetime:** the scope rides a console **session's** `Grant` — at most 4
  concurrent sessions, absolute 15-minute lifetime, no rolling expiry
  (`authority.rs:145`, ADR-107). The scope itself introduces no longer-lived
  credential.
- **Who mints it:** only the console authority — the operator-authenticated
  `POST /api/native/v1/console/access` (operator token, one issuance per
  second, **one outstanding ticket at a time**, replacement invalidating the
  preceding, exchanged before the next is minted — ADR-107's issuance rules
  verbatim in force). No browser path, no runner, no API key mints it.
- **What it must never grant:** no resource publication or configuration
  (their own scopes); no account act — no enrollment, no readiness, no
  credential namespace; no dispatch, runner capability or workspace access;
  no Matrix path or content; no child process, argv or workspace
  (ADR-053); no settlement of any stop; no store schema change; no route
  outside the three lifecycle acts.

**The control follows served permissions.** CL-S1's roster read publishes a
`permissions` object (`publishResource`/`configureResource`'s pattern,
`console/resources.rs:162-175`); the lifecycle controls render only from
their served booleans, and a read-only session renders none enabled.

## Consequences

Good, because the one class of act that changes an agent's runtime posture is
behind one reviewable scope with the console authority's existing minting
discipline, the stop cannot report an unobserved success, and start/preset
are bounded ensures rather than ports of the retained surface's process
control.
Bad, because `stopped:true` is unreachable until a host settlement path
exists (the operator sees `stop_pending`), and the scope is a third grant an
operator must reason about — mitigated by the mutual-exclusion test and the
never-grant list.

## Alternatives Considered

- Fold lifecycle into the configure scope — rejected by D-SCOPE: a session
  handed configure to edit a ceiling would gain the power to stop a live
  agent; least privilege loses, and the pairwise test with it.
- A stop-only scope — the original shape of this record; superseded by
  D-SCOPE once the operator put the whole lifecycle behind one scope rather
  than three slices.
- Port the retained start (spawn a launcher) — rejected: ADR-053's fixed
  launcher rule; start here is an ensure, never a process birth.
- `retire`'s session cascade for stop — rejected: it closes child
  conversations the operator did not name; the widening is a later decision,
  not a default.
