spec: task
name: "Browser decision protocol for inspected stopped tasks"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-LOCAL-THREE-RUNNER-QUALIFICATION]
tags: [active, console, recovery]
---

## Intent

Carry the browser half of the ADR170 console workflow. The native routes, their
store authority and the real-browser fixture are specified in
`task-rust-console-outcome-workflow.spec.md`; this spec covers only the
JavaScript module that constructs an operator decision and reads its result.

## Constraints

### Must
- Bind the request and its result to the selected original dispatch and task.
- Report a lost response as an unknown outcome and keep the exact serialized
  decision for an explicit same-request retry.
- Keep the one-use inspection secret in memory only.

### Must Not
- Do not retry a lost decision automatically or accept a result for another task.
- Do not render, log, persist or place the inspection secret in a URL.

## Boundaries

### Allowed Changes
- mockup/lib/native-recovery.js
- tests/dashboard-native-recovery.test.js
- specs/task-console-outcome-protocol.spec.md

## Scenarios

Scenario: Browser decisions remain bound and preserve uncertain requests
  Test: native recovery protocol binds decisions and results
  Given an inspected stopped dispatch
  When the browser constructs and submits an operator decision
  Then the request and result bind the original task and response loss is unknown
  And no credential is rendered or persisted
