spec: task
name: "Explicit YOLO and revocable scoped Codex approvals"
inherits: project
satisfies: [REQ-EXECUTION-AUTHORIZATION, ADR-028, ADR-003]
tags: [active, runtime, approval, console]
---

## Intent

Implement optional contributor-selected YOLO and precise task/persistent grants
without changing the default sandbox or private authenticated approval authority.

## Constraints

- Only contributor-authenticated configuration may enable YOLO; defaults remain sandboxed.
- Only leased writable Codex dispatches may disable the sandbox.
- Saved grants must use host-provided scope and Agent/project/owner/workspace identity.
- Owner changes, completed tasks and revocation must prevent future grant matches.
- Preserve single-use digest-bound verdict consumption and replay rejection.
- Show the exact proposed scope before allowing task or persistent authorization.
- Unknown request scopes and legacy adapters retain approve once/deny.
- Public notices and shared SSE contain no detailed permissions.
- Deterministic tests use local fixtures only; record live validation separately.

## Boundaries

### Allowed Changes
- lib/execution-authorization.js
- lib/approval-store.js
- backend-v2.js
- bridge-matrix.js
- router/src/runner.ts
- router/src/store.ts
- router/src/task-repository.ts
- router/src/migrations/011-task-authorization-epoch.ts
- router/dist/**
- mockup/components/**
- mockup/app/resources/**
- mockup/app/api/hagency/[...path]/route.js
- mockup/lib/**
- mockup/scripts/check-execution-permissions.mjs
- tests/**
- scripts/architecture-boundaries.json
- specs/task-execution-authorization.spec.md
- knowledge/requirements/req-execution-authorization.md
- knowledge/decisions/adr-028-execution-authorization.md
- docs/**

### Forbidden
- Live credentials, root agent entry files, weakened Matrix authentication, inferred shell/domain grants.

## Acceptance Criteria

Scenario: YOLO is explicit and lease constrained
  Test: explicit YOLO changes both native policies while default and unleased dispatches stay confined
  Given contributor-selected Codex execution settings
  When a native runner starts
  Then only explicit YOLO with a writable lease uses never and danger-full-access

Scenario: Scope comes from the native request
  Test: approval scopes preserve exact command and structured permissions without inferring domains
  Given native command network and permission callbacks
  When reusable scope is derived
  Then exact supported scopes are displayed and unknown scopes remain single-use

Scenario: Grants persist and revoke within authority
  Test: scoped grants survive restart but never cross task agent project owner or revocation
  Given an authenticated owner approval
  When task and permanent grants are reused or revoked
  Then only matching current authority is approved and stale or different scopes require approval

Scenario: Configuration is contributor owned
  Test: execution policy and grant management reject agent credentials and preserve defaults
  Given a resource and a provisioned Agent
  When execution settings are changed through the API
  Then contributor writes persist and agent-origin escalation is rejected

Scenario: Private cards present supported scopes
  Test: scoped approval cards preserve private details and validate all four structured decisions
  Given scoped and legacy requests
  When the Matrix bridge publishes and consumes approval cards
  Then supported decisions retain digest and authenticated sender binding while public notices stay redacted

Scenario: Retention and failed writes preserve durable projection authority
  Test: approval rollback preserves projection state and pruning waits for delivery without revoking grants
  Given a terminal request with an independent grant and an undelivered status
  When a write fails or the historical request exceeds seven days
  Then a failed write restores every state field and the original disk bytes
  And the request remains available until its projection has a durable receipt
  And later pruning rejects request replay without revoking the separate grant

## Out of Scope

- Claude YOLO, granting OS permissions, undoing completed operations, replacing native sandboxing.
