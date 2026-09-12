spec: task
name: "Bind native Codex lifecycle to one host-owned session"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREAD-SCOPED-SESSIONS]
tags: [active, rust, runner, session]
---

## Intent

Add a typed one-turn session wrapper around the bounded native Codex transport.
Keep upstream lifecycle observations separate from authenticated Hagency task,
approval, lease and process custody.

## Constraints

### Must
- Build thread start resume turn start and interrupt requests only from validated host settings and current upstream identities.
- Require a bounded absolute host cwd and chosen model and effort without arbitrary config or permission overrides.
- Default to workspace-write and on-request approval with explicit user review; support a host-selected read-only mode.
- Validate exact resumed thread returned thread cwd and reported policy without claiming sandbox efficacy.
- Buffer notification-before-response data within count and complete-byte limits and validate its scope before exposing activity.
- Reject wrong stale or substituted thread turn and item observations.
- Bound item identities event count and aggregate text; fail visibly instead of truncating final results.
- Distinguish completed failed interrupted unsupported-request and transport-unknown observations without completing canonical tasks.
- Make cancellation close the owned transport and keep a visible unknown outcome.
- Refuse server requests explicitly while the typed approval authority adapter is unavailable.
- Permit at most one turn and no warm reuse replay or automatic resume.

### Must Not
- Do not spawn models release leases change domain state or infer process cleanup from upstream events.
- Do not let model text notification fields or echoed sandbox settings grant execution approval.
- Do not contact live services or introduce shared domain DTOs.

## Boundaries

### Allowed Changes
- native/hagency-runtime/**
- specs/task-rust-codex-session.spec.md
- knowledge/decisions/adr-036-native-codex-session.md
- docs/**

### Forbidden
- Live services, credentials and the original dirty checkout.

## Acceptance Criteria

Scenario: Host settings produce safe typed lifecycle requests
  Test: native_codex_session_settings
  Given bounded host cwd model effort and optional read-only mode
  When a thread is started or explicitly resumed and its turn begins
  Then the wire uses the installed schema spellings and fixed sandbox approval policy
  And arbitrary permission overrides cannot enter the typed request

Scenario: Upstream identity and pre-response events remain scoped
  Test: native_codex_session_identity
  Given thread and turn notifications before or after their RPC responses
  When IDs are exact stale wrong or substituted during resume
  Then only correlated activity is exposed and mismatches close with unknown outcome

Scenario: Item and final text bookkeeping stays finite and explicit
  Test: native_codex_session_items
  Given item starts completions text deltas and final turn observations
  When items are reused duplicated or exceed text event and identity limits
  Then stale data and overflow fail visibly without silent truncation
  And observed completed failed and interrupted states do not mutate canonical task state

Scenario: Unsupported permissions and lost transport never become success
  Test: native_codex_session_outcomes
  Given a server request cancellation RPC rejection or closed transport
  When the session handles the event or its operation future is dropped
  Then it returns explicit unsupported failed interrupted or unknown observations
  And no approval child termination lease release warm reuse or replay is inferred

## Out of Scope

Real child startup and guardian handoff, effective sandbox qualification, actual
model/platform execution, authenticated task/approval identity, grants, MCP
execution, usage projection, Matrix delivery and full M4 parity.
