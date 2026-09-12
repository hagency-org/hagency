spec: task
name: "Local supervisor fixture port custody and failure evidence"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [node, regression, supervisor]
---

## Intent

Close the demonstrated duplicate-port defect in the legacy supervisor fixture
and preserve bounded evidence when its strict startup assertion fails.

## Constraints

### Must
- Hold both real ephemeral listeners until both distinct service ports exist and release all listeners on success or failure.
- Preserve the startup test's exact four-event assertion and three-second event deadline.
- Report bounded sanitized event names, restart counts and recognized child error codes on failure.
- Keep the original hosted failure distinct from the controlled same-port reproduction and passing local checks.
- Run exact Vitest selectors separately; agent-spec cannot execute Node tests.

### Must Not
- Do not change supervisor production code, health policy, leases, timeouts or live services.
- Do not print fixture environment, arbitrary event fields or raw child logs.
- Do not identify the unobserved historical CI cause as proven.

## Decisions

- [JS-only] Execute the actual Node fixture and Vitest selectors directly; retain unsupported or failed native lifecycle results separately.

## Boundaries

### Allowed Changes
- tests/local-service-supervisor.test.js
- tests/fixtures/local-service-fixture.mjs
- specs/task-node-supervisor-fixture.spec.md
- knowledge/context/node-supervisor-fixture-evidence.md
- docs/agent-knowledge.md
- docs/progress.md

## Acceptance Criteria

Scenario: Both selected ports remain reserved until selection completes
  Test: reserves two distinct service ports until the reservation scope ends
  Given two real loopback listeners
  When a second client attempts to bind either reserved port
  Then both binds fail with address in use
  And both ports are available after the scope ends

Scenario: Failed fixture construction releases its reservations
  Test: releases both service port reservations when fixture construction fails
  Given a fixture callback that fails after acquiring both ports
  When its reservation scope exits
  Then both ports can be bound again

Scenario: Strict startup evidence is retained after a real stop
  Test: preserves extra stopped events and restart evidence in startup failures
  Given four ready fixture children and a deliberately stopped relay
  When the exact four-event condition is checked again
  Then the additional stop is not filtered away and bounded diagnostics explain the failure

Scenario: Diagnostic projections are bounded and sanitized
  Test: bounds startup diagnostics without exposing arbitrary event or log fields
  Given extra event fields and a child log containing a recognized bind error
  When startup evidence is projected
  Then only fixed event and service fields and recognized error codes are emitted

Scenario: Normal supervisor startup remains healthy and ordered
  Test: starts all four services in dependency order and reports healthy
  Given independently reserved backend and dashboard ports
  When the actual supervisor starts the four fixture services
  Then all four report healthy and exactly four ready events retain backend first

## Out of Scope

Proving the old hosted failure's cause; eliminating unrelated processes racing
for a released port; changing legacy process-health or lease semantics; native
runtime migration behavior and production deployment.
