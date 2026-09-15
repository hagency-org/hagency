spec: task
name: "Wire the production provisioning ingress to admit engagements"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, provisioning, ingress, admission, matrix]
---

## Intent

Bind the production provisioning ingress the store already assumes: the
native product must be able to provision an engagement the way the retained
product does. Today `DomainRepository::admit` is the only engagement-minting
write, but no production code calls it or `verify_request` — every native
agent in every test was admitted by a fixture. This slice wires the Matrix
intake admission chain (owner message → intake admit → provider approval →
effect observed) to `verify_request` and `admit`, exactly as ADR-095's
2026-09-14 amendment decides.

## Constraints

### Must
- Observe the provisioning request through the production Matrix ingress: the `com.hagency.engagement.request.v1` event carried by the fake peer, whose body carries a **native-valid `request_id`** (the idempotency key) and the Matrix event id as the **`source_event_id`**.
- Verify it with the same `verify_request` (`hagency-core/src/authority.rs:196`) — the event-type, source-event, room and sender checks already there — so an unverified request is refused **before** `admit`.
- Call `DomainRepository::admit` exactly once for a verified request — the single minting write — and observe the provider approval as the separate `approve` verdict, never folded into the mint.
- Key idempotency on `request_id`: an identical duplicate (same `request_id` + same digest) replays the prior admission; a same-`request_id` different-content request is refused as a conflict — never a second INSERT.

### Must Not
- Do not mint an engagement outside `admit` — no second INSERT, no direct roster write, no fixture-style seeding in the production path.
- Do not change `admit`, `verify_request`, the `VerifiedRequest` shape or any store schema; this slice wires the production caller, it does not move the write.
- Do not add a console create-agent HTTP route — the ingress is Matrix intake, not a REST endpoint (the console `agents.rs` stays GET + `{id}/start|stop|preset`).
- Do not gate any scenario by OS or feature in the binding set.

## Boundaries

### Allowed Changes
- native/hagency-matrix/src/intake.rs
- native/hagency-matrix/src/collector.rs
- native/hagency-matrix/src/collector/observation.rs
- native/hagency-matrix/src/event_batch.rs
- native/hagency-matrix/src/lib.rs
- native/hagency-matrix/src/config.rs
- native/hagency-core/src/replies.rs
- native/hagency/src/bootstrap.rs
- native/hagency/src/bootstrap/config.rs
- native/hagency/src/lib.rs
- native/hagency-store/src/domain/verified_ingress.rs
- native/hagency-store/src/domain/approvals.rs
- native/hagency-store/src/domain_worker.rs
- native/hagency-matrix/tests/
- native/hagency/tests/
- native/fixtures/
- specs/task-rust-provisioning-ingress.spec.md
- knowledge/decisions/adr-095-native-state-ownership.md
- docs/progress.md

### Forbidden
- Live services, live homeservers, credentials, deployed state.
- native/hagency-store/src/domain.rs (admit is unchanged); native/hagency-core/src/authority.rs (verify_request is unchanged); native/hagency/src/console/**.

## Decisions (2026-09-14, integration fix-up after the builder's blocker report)

- The reception room is **pre-project**. A `com.hagency.engagement.request.v1` event observed there never carries a `ReplyRoute` (that shape binds a project, an owner and a fleet, none of which exist before admission) and is never resolved through `targets()`; the intake routes it by discriminator to `provision()` **before** target resolution, so `event_batch`'s `NotTarget` drop never sees it. Session routes keep refusing the reception room (`matrix_routes.rs` stays outside this slice).
- The reception room enters the observed set through a dedicated `HostConfig` field (hence `config.rs` in Allowed Changes) filled at bootstrap from the store's recorded registration that `registration_fingerprint` names — store-driven and fail-closed (ADR-095 amendment 2026-09-14): a registration without a reception room is a named startup refusal, never a silent skip and never a new operator-supplied config field.
- The provisioning facts (`RoomObservation` ×3, binding, name, powers) come from the collector's own room-state snapshots of the reception, project and owner rooms, as already committed on the builder's branch.

## Acceptance Criteria

Scenario: A provider-approved request provisions an engagement through the production ingress
  Test: native_provisioning_ingress_admits_a_provider_approved_request
  Level: integration
  Test Double: the shared fake peer delivering a com.hagency.engagement.request.v1 event, plus the collector's own room-state snapshots for the reception, project and owner rooms
  Given an intake event with kind com.hagency.engagement.request.v1 whose body carries the retained request fields (requestId = a native-valid request_id distinct from the event id, source_event_id = the event id, project, projectRoomId, role, requester, requestedTokens, ratePerDay, agent, context)
  When the intake hook assembles the ProjectRequest and the three RoomObservations from the collector's SDK facts and calls verify_request then DomainStore::admit
  Then admit runs exactly once and the engagement exists with its minted en_ id — the provider verdict is observed afterwards as the separate approve step, never folded into the mint
  Production caller: hagency_matrix::intake::Inner::provision

Scenario: An identical duplicate provisioning request replays the prior admission
  Test: native_provisioning_ingress_replays_an_identical_duplicate
  Level: integration
  Test Double: the same event delivered twice with the same request_id and the same content digest
  Given a request already admitted for a request_id (the requester's native-valid idempotency key)
  When the identical event is delivered again
  Then the prior admission is returned and counted replayed — no second engagement row is minted
  Production caller: hagency_matrix::intake::Inner::provision

Scenario: A same-key different-content request is refused as a conflict and quarantined
  Test: native_provisioning_ingress_refuses_a_conflicting_request_by_the_same_key
  Level: integration
  Test Double: the same request_id delivered with a different content digest
  Given a request already admitted for a request_id whose content digest differs
  When the conflicting event is delivered
  Then the ingress refuses it as a conflict on the same request_id key and quarantines it — no second engagement row is minted
  Production caller: hagency_matrix::intake::Inner::provision

Scenario: An unverified or unenrolled provisioning request is refused before admit
  Test: native_provisioning_ingress_refuses_unverified_before_admit
  Level: integration
  Test Double: a request event whose assembled RequestObservation fails the verify_request checks (a missing/expired room snapshot or a mismatched binding), or whose owner has no enrolled owner room
  Given a request whose room snapshots, powers, binding or sender do not satisfy verify_request, or whose owner has no enrolled owner room in the store
  When the intake hook assembles the observation
  Then it is refused before admit runs with a named reason — an unverified request and an unenrolled owner both fail closed — and no engagement row exists
  Production caller: hagency_matrix::intake::Inner::provision

## Decisions

**The ingress is Matrix intake, not a console route.** There is no native
create-agent HTTP route, and this slice adds none: the product provisions
through the Matrix admission chain, and the store's own `admit`/`approve`
pair is the write surface — this slice only gives them their production
caller.

**The verdict, effect and route are decided by ADR-147.** How the provider's
verdict arrives (a new pre-project wire event kind), how the provision effect
is claimed and completed (inline, no worker), and how the new engagement's
session id and transport sender are derived are product decisions, recorded
and evidenced in `knowledge/decisions/adr-147-provisioning-verdict-effect-route.md`.
This spec's scenarios observe the rows and refusals that ADR names.

Scenario: The provider approval produces the provision effect
  Owed Selector: native_provisioning_effect_produced (parked — owed by the G2 wiring slice; no Test: line here yet)
  Given an admitted engagement pending provider approval
  When the provider approval is recorded by the wired approval path
  Then the effects row carries kind=provision for the engagement
  Production caller: owed (G2)

Scenario: The provision effect is claimed and observed complete
  Owed Selector: native_provisioning_effect_completed (parked — owed by the G2 wiring slice; no Test: line here yet)
  Given a recorded provision effect for an admitted engagement
  When the wired effect worker claims the effect and reports the provision outcome
  Then the effects row carries kind=provision state=complete and the engagement_ends row is observed
  Production caller: owed (G2)

Scenario: The admitted engagement's project room binds a session route
  Owed Selector: native_provisioning_session_route (parked — owed by the G3 wiring slice; no Test: line here yet)
  Given an admitted engagement with its project room and an intake plan naming a configured session id
  When the wired route registrar resolves the verified Matrix session
  Then the matrix_session_routes row binds the project room to the configured session id so the intake plan's session resolves
  Production caller: owed (G3)

## Out of Scope

The provider-approval verdict surface itself (already owned by the
approval ADRs), resource/seat allocation decisions (ADR-121/122/123), the
console roster read, and any change to `admit`/`verify_request`.
