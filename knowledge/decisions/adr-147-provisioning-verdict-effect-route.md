---
kind: decision
id: ADR-147
title: "The provisioning verdict, its effect and the new engagement's route"
status: Decided
requirements: [REQ-RUST-MIGRATION-EXECUTION]
tags: [native, provisioning, ingress, engagement, verdict, matrix]
---

## Context

The provisioning ingress (task-rust-provisioning-ingress.spec.md, landing) admits
an engagement from a `com.hagency.engagement.request.v1` event observed in the
pre-project reception room, verified by `verify_request` and minted by
`DomainRepository::admit` (`domain.rs:1084`). Admission leaves the engagement
`pending` — minted but neither **effective** (the provider's approval observed,
`approve` written, the provision effect claimed and observed complete) nor
**routable** (a `matrix_session_routes` row). The next slice closes both.

The builder implemented that second half on four assumptions that no ADR or spec
decides. They are product decisions, and this ADR decides them, each against the
retained product (the JS/TS sources at the repo root) and the native ADRs.

### Placement (review addendum)

The review of the slice asks that each decision be recorded where the owning ADR
already governs that surface, amending an existing ADR rather than creating a new
one when one owns the surface. The placements are: (a) to the approval wire
surface, **ADR-143**; (c) the session-key derivation, **ADR-095**; (d) the
MXID/device shape, **ADR-014**; and (b) is confirmed-by-existing-decision under
**ADR-022**. This ADR records the four decisions together as the slice's single
decision of record and names that placement for each; the per-ADR amendment text
is applied to ADR-143, ADR-095 and ADR-014 in their own files. (The review's cited
evidence lines — ADR-095's 2026-09-14 amendment and the intake `provision()` body
— are on the slice's own branch, not this head; this ADR cites the retained
product's source directly instead.)

## Decision

### (a) The provider's verdict carrier — placed with ADR-143 (the approval wire surface)

**Decision: the retained product has no equivalent; the native product decides
`com.hagency.engagement.approval.v1` carrying `{requestId, decision}`, accepted
only from the fleet's representative — because the native provisioning ingress is
Matrix-only and no provisioning-verdict wire kind exists.**

Placement. ADR-095 decides the admission *chain* ("owner message → intake admit →
provider approval → effect observed") and names exactly one wire kind,
`com.hagency.engagement.request.v1`, but never says how the provider verdict
arrives. ADR-143 owns the native approval **wire** surface but knows only the
owner-approval v1 profiles. A new wire *kind* is a wire-surface decision, so it
belongs to **ADR-143**; this ADR records the decision and ADR-143's amendment
carries the kind, its fields and its sender rule.

Evidence. The retained product's provider verdict arrives over **HTTP**, not
Matrix: `POST /api/engagements/:id/verdict` (`backend-v2.js:15160`) calls
`engagementStore.decide({engagementId, approve, …})`
(`lib/engagement-store.js:593`). No `com.hagency.engagement.approval.*` event kind
exists in the retained source — the only retained verdict event is
`com.hagency.approval.verdict.v1` (`bridge-matrix.js:219-220`), and that carries
**execution** approvals (owner tool-call verdicts for a running agent), not
engagement provisioning. The retained console is a REST surface; the native
provisioning-ingress spec expressly forbids adding one ("Do not add a console
create-agent HTTP route — the ingress is Matrix intake, not a REST endpoint"). So
the verdict must arrive on the same pre-project reception room the request did, as
a new versioned event kind mirroring `com.hagency.engagement.request.v1`
(`lib/fleet-protocol.js:4`).

The kind is **`com.hagency.engagement.approval.v1`**, fields
**`{requestId, decision}`**, and it is **accepted only from the fleet's
representative sender** — the same authority the request's `verify_request`
checks — so a verdict from any other sender is refused before `approve`.
`requestId` binds the verdict to the request's own idempotency key.

### (b) Inline synchronous provision — confirmed by existing decision (ADR-022)

**Decision: confirmed by existing decision, not decided anew. ADR-022's title is
"…provisions agents on approval", and the retained behaviour is inline synchronous
provisioning with no effect worker; adopt it.**

Evidence. The retained product provisions inline inside the verdict request:
`fulfillEngagement` (`backend-v2.js:14700-14830`) provisions the agent home
(`provision-v1-agent-home.js`), mints the Matrix identity, binds the owner, admits
the agent to the project room, and launches it — all **inside** the verdict
handler, awaiting each step, with no background effect worker. ADR-022
(resource-first agent allocation) is built on exactly this retained inline
`createAgent`. The native store already models the effect as a claim/observe pair
rather than a queued job: `claim_effect` (`domain.rs:1338`) takes the pending
provision effect when the engagement is `reserved`, and `observe_effect`
(`domain.rs:1357`) completes it (`Applied` → `Active`). The intake handoff claims
the effect and observes it complete inline, in the same handoff — matching the
retained inline shape. No separate effect worker is introduced.

### (c) The session id derivation — placed with ADR-095 (native state ownership)

**Decision: the retained product has no equivalent; the native product decides the
intake derives the session id deterministically as `session_{engagement_id}` —
because native sessions are engagement-scoped, not room/thread-reused.**

Placement. The session id keys the engagement's later route resolution (G3), so
its derivation is a native state-ownership decision and belongs to **ADR-095**;
this ADR records it and ADR-095's amendment carries the key rule.

Evidence. The retained product has no engagement-derived session id at all: its
agent localpart is `{agentPrefix}{agentName}` (`backend-v2.js:14791`), derived
from the *agent name*, and its sessions are room/thread-scoped conversations, not
per-engagement. Native, by contrast, keys a session to its engagement and room
(ADR-011's backend-owned session model; the store's
`register_session`/`SessionBinding`). The intake mints the session for the
engagement it just made effective, and a deterministic `session_{engagement_id}`
keeps that derivation idempotent across restart and replay: re-observing the same
admission derives the same session id rather than minting a second row.

### (d) The MXID/device shape — placed with ADR-014 (agent Matrix identity provisioning)

**Decision: the retained product is the source; adopt the retained derivation,
keyed on the engagement id — `@…{engagement_id}` for the sender localpart and
`DEVICE_{engagement_id}` for the device — because the store's
`UNIQUE(server_name, sender_mxid)` forbids reusing the host's own sender.**

Placement. The MXID/device shape is the engagement's transport authority forever
after, so it is an agent-identity-provisioning decision and belongs to **ADR-014**;
this ADR records it and ADR-014's amendment carries the shape.

Evidence. The retained product derives a new agent's stable identity from a
deterministic, engagement-bound value: `fulfillEngagement` builds the agent name
as `mx_{sideId…}_{role}_{sha256(id)[:12]}` — a sha256 over the engagement id
(`backend-v2.js:14756`) — and derives the sender localpart as
`{agentPrefix}{agentName}` (`backend-v2.js:14791`). The native rule adopts the
retained "derive the identity deterministically from the engagement" behaviour
exactly in kind; it deviates only in the *key*, binding the localpart and device
directly to the engagement id rather than to an intermediate agent name, because
the native store has no separate agent-name surface and the
`UNIQUE(server_name, sender_mxid)` constraint requires a sender distinct from the
host's own. Deriving both from the engagement id gives each newly effective
engagement a distinct, stable, restart-safe sender and device that satisfy that
constraint without colliding with the host or any other engagement.

## Consequences (what the tests must observe)

- **(a) verdict kind.** Approving a minted engagement from `requestId` writes the
  `approve` verdict and moves the engagement out of `pending`; a verdict event
  from a sender other than the fleet's representative is refused **before**
  `approve` (no `decisions`/`approve` row); a verdict naming an unknown or
  already-decided `requestId` is refused (`NotFound`/`State`), never a second
  verdict write.
- **(b) inline effect.** After the verdict's handoff returns, the provision
  effect is observed `Applied` and the engagement is `Active` — with no pending
  `effects` row left for a worker; a failed observation leaves the effect
  recoverable (uncertain), not silently `Active`.
- **(c) session id.** A `matrix_session_routes` row exists whose session id is
  exactly `session_{engagement_id}`; re-admission/replay derives the same id and
  creates no second row.
- **(d) transport sender/device.** The new engagement's transport sender and
  device are derived from its engagement id and differ from the host's sender
  (satisfying `UNIQUE(server_name, sender_mxid)`); a second route reusing the
  host's sender is refused by the constraint.

## Alternatives Considered

- *Add a console/REST verdict route like the retained product.* Rejected: the
  provisioning-ingress spec expressly forbids a console create-agent HTTP route;
  the native ingress is Matrix intake, so the verdict must ride the wire.
- *Reuse `com.hagency.approval.verdict.v1` for the provisioning verdict.*
  Rejected: that kind carries execution approvals bound to a running agent's
  tool-call context; overloading it for engagement provisioning would conflate
  two authorities.
- *Introduce an effect worker to claim/complete the provision effect
  asynchronously.* Rejected: ADR-022 already decides inline provisioning, and the
  store's claim/observe pair already models the effect without a worker; one adds
  a failure surface (queue, retry, ordering) the slice does not need.
- *Derive the session id or sender from the room/thread or a random value.*
  Rejected: room/thread derivation is not engagement-stable (ADR-011 keys the
  session to the engagement), and a random id is not restart-idempotent.

## Related

- **ADR-143** (native approval wire oracle) — owns the wire surface; its amendment
  carries the new verdict kind (a).
- **ADR-095** (native state ownership) — owns the admission chain and the
  session-key derivation (c).
- **ADR-014** (agent Matrix identity provisioning) — owns the MXID/device shape
  (d).
- **ADR-022** (resource-first agent allocation) — already decides inline
  provisioning; (b) is confirmed by it.
- **ADR-011** (backend-owned ephemeral runner sessions) — the engagement-scoped
  session model (c) derives from.
- specs/task-rust-provisioning-ingress.spec.md — the ingress this ADR's second
  half completes.
