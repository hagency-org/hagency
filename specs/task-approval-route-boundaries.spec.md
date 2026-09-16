spec: task
name: "Register canonical approval route ownership and read effects"
status: accepted
inherits: project
satisfies: [REQ-APPROVAL-CANONICAL-PROJECTION, REQ-OWNER-UI-APPROVAL]
---

## Intent

Keep the architecture boundary registry aligned with the accepted canonical approval APIs. The complete CI check currently rejects eight undeclared mutation registrations. Read endpoints that can expire requests or run maintenance must also state those effects.

## Decisions

- Register marker synchronization, migration, retirement and delivery, legacy attestation, publisher observation and projection preparation/delivery with their existing approval bridge-secret guard.
- Register the literal dynamic operation paths as scanned by the existing checker. Do not change the scanner or route authorization.
- Record native request and Matrix request reads plus projection maintenance as stateful GETs. Keep native agent-token and Matrix bridge-secret authentication distinct.
- Register private marker inventory, due marker and legacy evidence reads as sensitive routes. Passive queue/inventory reads remain distinct from maintenance.

- Validation: Run npm run check:architecture-boundaries and the exact Vitest files tests/architecture-boundaries-check.test.js and tests/api-approval-projections.test.js. Run agent-spec parse/lint and lifecycle boundary checks separately; Node lifecycle skips are not test passes.

## Boundaries

### Allowed Changes
- scripts/architecture-boundaries.json
- specs/project.spec.md
- specs/task-approval-route-boundaries.spec.md

### Forbidden
- Production code, authorization policy, route implementations, checker logic, dependencies, live service or runtime changes.

## Acceptance Criteria

Scenario: Declared stateful reads retain their authorization
  Test: passes listed stateful GET routes and validates their auth policy
  Given a registered stateful read has an expected authorization guard
  When the existing architecture checker scans its handler
  Then it accepts the matching guard.

Scenario: Maintenance cannot be presented as passive reading
  Test: fails when a stateful GET route has no owner entry
  Given a handler contains a registered state-changing read marker
  When its ownership entry is absent
  Then the checker rejects the route.

Scenario: Projection metadata remains protected
  Test: projection listing is bridge-secret only and keeps metadata out of agent response
  Given the canonical projection API is installed
  When unauthenticated or agent-authenticated callers request Matrix projection details
  Then those callers cannot read bridge-only metadata.
