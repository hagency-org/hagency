spec: task
name: "Drain thread outboxes during botless appservice startup"
inherits: project
satisfies: [REQ-TSS-TASK-ACTIVATION, ADR-016]
tags: [matrix, appservice, regression]
---

## Intent

Fix the local macOS E2E failure where an appservice-only bridge receives a
worker task but leaves it in pending_thread forever because router outbox
polling starts only after ordinary bot login succeeds.

## Constraints

### Must
- Start the initial router outbox poll and recurring poll from common bridge
  startup after acting credentials and the agent roster are ready.
- Preserve backend-selected sender, room, thread root and transaction ids.
- Keep polling behind the existing thread-sessions feature flag.

### Must Not
- Do not require an ordinary bot password for appservice outbox delivery.
- Do not weaken Matrix authorization or mark task completion from reply text.
- Do not contact live services from deterministic regression tests.

## Decisions

- Move the existing poll and timer from startBotSide to start; reuse the
  existing outbox delivery implementation without changing its protocol.
- [JS-only] Verify with Vitest using the real startup, poll and delivery
  methods, replacing unrelated startup I/O and the external send boundary.
- Classify fixture backend calls by method and exact route: approval request/room/marker
  GET pages return their real empty collection shape and never count as router receipts.
  Exercise their startup and recurring worker polls, then stop owned work and clear
  fake timers during cleanup. Keep the real router startup and delivered metadata checks.
- Startup diagnostics distinguish unavailable local-bot E2EE and approval rooms from
  project-side plaintext owner approvals under their normal publisher, binding, membership
  and room-security checks. The representative path does not support encrypted approvals.
- Keep live Docker Palpo and Robrix evidence separate from deterministic
  coverage. Report incomplete E2E boundaries explicitly.

## Boundaries

### Allowed Changes
- bridge-matrix.js
- tests/bridge-botless-router-start.test.js
- specs/task-botless-thread-outbox.spec.md
- docs/E2E-RUNBOOK-macos.md
- docs/progress.md
- docs/agent-knowledge.md

### Forbidden
- remote/**
- Runtime credentials and local .env files
- Task authorization, task state-machine or runner capability changes

## Acceptance Criteria

Scenario: Failed bot login still permits immediate and recurring appservice delivery
  Test: a pending task acknowledgement and a later reply both leave after bot login fails
  Given thread sessions are enabled and ordinary bot login fails
  When common bridge startup runs with an appservice sender
  Then the pending task acknowledgement is sent and its delivered receipt is recorded
  And a later reply is sent on the recurring poll with its delivered receipt

Scenario: Outbox delivery preserves the backend-selected route
  Test: router outbox delivery uses backend sender route and stable transaction metadata
  Given an outbox command with backend-selected sender and thread root
  When the bridge delivers the command
  Then the Matrix content and delivered receipt preserve that route

Scenario: Retrying an explicit Matrix transaction keeps its identity
  Test: explicit router transaction id is used verbatim for idempotent Matrix send
  Given an outbox command carries an explicit transaction id
  When it crosses the Matrix send boundary
  Then the bridge uses the same id without generating a replacement

## Out of Scope

- Exposing legacy task lifecycle MCP tools to ephemeral runners.
- Changing global Claude or Codex settings to isolate this E2E deployment.
