spec: task
name: "Wire provider approval provision effects and project-room session routes into production"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, provisioning, wiring]
---

## Intent

Close wiring gaps G2 and G3 from ADR-146: the provider approval must produce the
provision effect, the provision effect must be claimed and observed complete,
and the admitted engagement's project room must bind a session route row so the
intake plan's configured session resolves. Today every write in these flows is
reached only by tests and fixtures (`approve`, `claim_effect`, `observe_effect`,
`retry_cleanup`; `resolve_verified_matrix_session` as the only inserter of
`matrix_session_routes`), so in a real deployment an admitted engagement never
becomes effective and no Matrix message is ever routed to an agent.

## Constraints

### Must
- Derive the provision effect from the provider approval decision itself, never from chat text or upstream echoed identity.
- Claim and observe each provision effect exactly once; keep retried or crashed effects outcome-unknown, never silently duplicated.
- Bind the session route row (`matrix_session_routes`) to the admitted engagement's project room and the intake plan's configured session id.
- Fail closed: with no route row the intake plan's session must not resolve, and no dispatch may proceed account-bound without the provision effect complete.

### Must Not
- Do not reach these store writes from tests fixtures or the bootstrap probe and call the flow wired.
- Do not invent a new approval, effect, or route surface beside the store methods ADR-146 names.

## Boundaries

### Allowed Changes
- native/**
- specs/task-rust-provisioning-ingress.spec.md
- docs/**

### Forbidden
- Root manifests and independent task CLI or protocol implementations.
- Credentials, deployed services, live models and original dirty checkouts.

## Acceptance Criteria

Scenario: The provider approval produces the provision effect
  Owed Selector: native_provisioning_effect_produced
  Given an admitted engagement pending provider approval
  When the provider approval is recorded by the wired approval path
  Then the effects row carries kind=provision for the engagement
  Production caller: owed (G2)

Scenario: The provision effect is claimed and observed complete
  Owed Selector: native_provisioning_effect_completed
  Given a recorded provision effect for an admitted engagement
  When the wired effect worker claims the effect and reports the provision outcome
  Then the effects row carries kind=provision state=complete and the engagement_ends row is observed
  Production caller: owed (G2)

Scenario: The admitted engagement's project room binds a session route
  Owed Selector: native_provisioning_session_route
  Given an admitted engagement with its project room and an intake plan naming a configured session id
  When the wired route registrar resolves the verified Matrix session
  Then the matrix_session_routes row binds the project room to the configured session id so the intake plan's session resolves
  Production caller: owed (G3)

## Out of Scope

Managed-account login (G4), restart recovery (G5), workspace registration (G6),
grant revocation (G7) and conversation stop settlement (G8) are separate
tracked gaps in ADR-146 and are owed by their named owners.
