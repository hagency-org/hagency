---
kind: decision
id: ADR-130
title: "Finite native agent lifecycle authority with fail-closed incomplete transitions"
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
authority's own, minted exactly like the others and reaching the reviewed
lifecycle routes, including stop, recovery, refusal and retirement. Compatibility routes
for start and preset-apply remain in the same scope but do not grant an operation
until their durable transitions exist.

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

**Start — unavailable until it has a real transition.** Native has no durable
agent lifecycle record independent of engagements, and no route-owned host proof
that can settle a stop, release its custody and re-arm queue participation. The
compatibility route therefore returns `agent_start_unavailable` (HTTP 501) after
scope validation and before any store read or spawn. A successful no-op is not an
ensure. ADR-053's fixed launcher remains the owned-dispatch host's.

**Preset-apply — unavailable until it preserves the whole engagement.** The
resource id on an engagement binds its budget, account association,
qualification and provision effect. An in-memory pointer to another preset
changes none of those durable facts and is therefore not an apply. The
compatibility route returns `agent_preset_unavailable` (HTTP 501) after scope
validation, creates no pending marker and reports no success. A future design
must own the full retire/reprovision transition atomically.

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
  outside the explicitly routed lifecycle commands.

**The control follows served permissions and implementation state.** CL-S1's
roster read publishes a `permissions` object. A lifecycle session renders only
the stop control, whose domain write exists. Start and preset controls remain
absent while their routes return named unavailable errors; a read-only session
renders none enabled.

## Consequences

### 2026-09-16: Withdraw unproven global host settlement

The later bootstrap sweep was reachable but unsafe: after any operation result,
including a pre-child failure, it formatted that report and settled every pending
stop. The store authenticates the supplied stop id/fence; it cannot infer which
physical owner or workspace the caller inspected. The old fixture directly
created Started rows and supplied a string, proving only the trusted store seam.

The driver no longer makes that inference. A regression now retains two actual
original Started operations, fences one through the real operator stop command,
and returns the other's real failure through the production completion path.
Before the correction both stops incorrectly became settled; after it both
retain pending rows, dirty workspaces and leases. Returning the second original
operation also cannot invent inspected effects. Successful original completion
publication and final delivery, and explicit operator recovery, are unchanged.

Automatic positive stop settlement remains G8 and requires exact original
stopped-owner and workspace-inspection evidence. Removing the unsafe sweep is
not completion of that obligation or of native agent lifecycle parity.

Good, because lifecycle mutations are behind one reviewable finite scope, stop
cannot report an unobserved settlement, and incomplete start/preset work can no
longer report operational success without changing state.
Bad, because `stopped:true` is unreachable until a host settlement path exists,
and native does not yet offer start or resource rebinding. Those gaps are visible
as named 501 refusals and absent controls rather than misleading success.

## Alternatives Considered

- Fold lifecycle into the configure scope — rejected by D-SCOPE: a session
  handed configure to edit a ceiling would gain the power to stop a live
  agent; least privilege loses, and the pairwise test with it.
- A stop-only scope — the original shape of this record; superseded by
  D-SCOPE once the operator put the whole lifecycle behind one scope rather
  than three slices.
- Port the retained start as a no-op ensure — rejected: without a durable re-arm
  transition, success would be false. Spawning a launcher is separately rejected
  by ADR-053's fixed launcher rule.
- Keep preset id only in the browser grant — rejected: it is neither durable nor
  connected to the engagement's budget, account or provision effect.
- `retire`'s session cascade for stop — rejected: it closes child
  conversations the operator did not name; the widening is a later decision,
  not a default.
