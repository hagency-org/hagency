spec: task
name: "Claim the exact original provision effect before inline fulfillment"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, provisioning, custody]
---

## Intent

Provide the exact effect claim needed by ADR-147's synchronous inline physical
provisioner. The existing adoption caller must not start an unrelated pending
effect and only then discover that its ID does not match the approved engagement.
Claiming an effect is not proof of physical provisioning or permission to report
Applied; full automatic account/home/runtime fulfillment remains required.

## Constraints

### Must
- Select the named effect inside the original immediate transaction, retaining the existing current-registration, pending-effect and engagement-state eligibility checks.
- Increment its fence exactly once; competing claims return at most one original Started effect.
- Leave unrelated effects untouched on missing, ineligible, duplicate or failed claims.
- Roll back state/fence on SQLite failure or exhausted fence.
- Preserve Started-to-Uncertain reopen behavior; uncertain, completed, cancelled and old-registration effects cannot be claimed anew.
- Route exact host claims through the same bounded DomainStore worker; no runner, MCP or HTTP authority setter is added.
- Change the actual external-account adoption caller to request its exact provision effect.
- Exercise real offline SQLite/domain-worker boundaries, not a mocked claim result.

### Must Not
- Do not synthesize account/home/process receipts, mark approved intent Applied, create routes from derived identity plans, add an effect worker, or retry uncertain external actions.
- Do not change generic cleanup claims, verdict authorization, admission, schema or production qualification gates.

## Boundaries

### Allowed Changes
- native/hagency-store/src/domain.rs
- native/hagency-store/src/domain_worker.rs
- native/hagency-store/tests/domain.rs
- native/hagency/src/bootstrap/provision.rs
- knowledge/decisions/adr-147-provisioning-verdict-effect-route.md
- docs/progress.md

## Acceptance Criteria

Scenario: The exact inline claim never starts another approved engagement
  Test: native_provision_claim_exact_owner
  Level: integration
  Test Double: real DomainRepository SQLite and bounded DomainStore writer
  Given two independently approved pending provision effects
  When two callers race to claim the exact lexically later effect
  Then exactly one claim starts that effect and the unrelated earlier effect remains pending with its original fence

Scenario: Failed or stale claims preserve original custody
  Test: native_provision_claim_recovery_fences
  Given actual SQLite update failure, exhausted fence, cancelled effect or a retired registration
  When the host claims an exact effect or reopens original Started custody
  Then failures roll back, reopen retains Uncertain custody and no unrelated or uncertain effect is rearmed

## Out of Scope

This prerequisite does not complete ADR-147. Automatic physical identity creation,
agent home, SDK enrollment, room admission, original runtime launch, route readiness
and real two-agent qualification remain owed by the overall migration goal.
