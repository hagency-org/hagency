spec: task
name: "Expose the provider's headless login through the retained native account"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, accounts, qualification]
---

## Intent

Enable an operator on mini3 to authenticate a fresh managed account using the
pinned provider's device authorization flow before real two-agent qualification.
The existing CLI only invokes plain login; an arbitrary wrapper must not be
necessary to select the provider's fixed headless option.

## Constraints

### Must
- Expose an explicit opt-in --device-auth on account login; keep plain login unchanged.
- Pass only login and the fixed --device-auth argument to the selected provider binary.
- Preserve the original retained namespace, environment clearing, attempt-before-spawn and exit-derived settlement.
- Keep the provider's terminal streams inherited and never inspect or copy credentials.
- Keep refused and unclassifiable exits unknown, including device authorization.
- Use actual isolated native CLI and provider fixture processes for regression tests.

### Must Not
- Do not initiate live provider login from tests or from service startup.
- Do not import credentials, accept arbitrary provider options or grant browser login authority.
- Do not assert real authentication, model execution or soak completion from offline tests.
- Do not change resource associations, readiness TTL, sandbox defaults or the running live fleet.

## Boundaries

### Allowed Changes
- native/hagency/src/bootstrap/accounts.rs
- native/hagency/tests/login.rs
- native/hagency/tests/fixtures/login_probe.rs
- docs/design/two-agent-qualification-runbook.md
- specs/task-rust-managed-device-login.spec.md
- docs/progress.md
- docs/agent-knowledge.md

### Forbidden
- Credential bytes in the source tree or logs; live services inside Cargo tests.
- Store/schema changes, automatic login or arbitrary command argument passthrough.

## Acceptance Criteria

Scenario: Plain operator login remains unchanged
  Test: native_account_login_route_records_ready
  Level: integration
  Test Double: actual native executable with an isolated offline provider fixture
  Given a fresh retained account and hostile ambient environment
  When the operator login route runs without the new flag
  Then the provider receives exactly login in the retained namespace and observed readiness settles only after successful exit

Scenario: Headless login preserves exact invocation and settlement
  Test: native_account_device_login_route_preserves_settlement
  Level: integration
  Test Double: actual native executable with isolated success refusal and unknown provider processes
  Given a fresh retained account for each provider exit class
  When the operator login route receives --device-auth
  Then the provider receives exactly login --device-auth without ambient provider values
  And the original attempt is settled with the observed refused or uncertain outcome and only observed reads ready

## Decisions

This is ADR114's host-only operator login, not a service-driven authentication
flow. The pinned Codex binary's offline login help and local upstream CLI source
both expose --device-auth. No live login or token inspection is necessary to
verify argument forwarding. Real login still requires the operator's provider
consent and is separate qualification evidence.

## Out of Scope

API-key enrollment, automatic credential refresh, provider identity or quota
claims, full migration parity, and declaring the two-agent qualification passed.
