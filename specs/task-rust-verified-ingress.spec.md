spec: task
name: "Connect verified Matrix ingress to native canonical task execution"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREAD-SCOPED-SESSIONS, REQ-MATRIX-DM-PRIVACY, REQ-THREE-LAYER-COMPLETION]
tags: [active, rust, matrix, ingress]
---

## Intent

Admit authenticated host Matrix observations into current session-owned input,
create and activate canonical tasks through exact host anchor receipts, and carry
fresh human follow-up through frozen dispatches to final reply intent.

## Constraints

### Must
- Bind host observations to exact current full Matrix server room account device session and registration generations.
- Derive group wake from authenticated full-MXID mentions and current joined non-service senders; direct human messages need no mention.
- Preserve background input without waking another agent and reject stale future unknown or mismatched route observations.
- Persist event digests generation provenance and independent immutable session input copies atomically.
- Require current verified input before creating task intent and acknowledgement; never upgrade legacy sessions or remap old private input.
- Keep direct main as a continuing null-root conversation while storing original source root and activation acknowledgement separately.
- Derive task thread roots from authenticated input and keep group mention tasks distinct per assignee.
- Activate task input only after exact current host acknowledgement and fence old notice claims after route retirement.
- Freeze dispatch input and advance canonical epoch only when fresh original-human follow-up actually starts.
- Verify offline repository and HTTP flow from admitted input through real task intent activation completion and final reply.

### Must Not
- Do not accept event target wake generation or transport receipt setters through runtime HTTP.
- Do not infer privacy from names or member counts or copy private context into promoted group sessions.
- Do not let agent-originated output create agent-to-agent wake loops or completion authority.
- Do not contact models homeservers credentials or deployed services.

## Boundaries

### Allowed Changes
- native/**
- specs/task-rust-verified-ingress.spec.md
- knowledge/decisions/adr-038-native-verified-ingress.md
- docs/**

### Forbidden
- Root manifests and independent protocol or runner transport implementations.
- Original dirty checkout, live runtime state, credentials and deployed services.

## Acceptance Criteria

Scenario: Verified admission derives wake from authenticated policy
  Test: native_verified_ingress_policy
  Given current direct or group routes and full sender mention observations
  When human agent background or malformed events are admitted
  Then only eligible human messages wake their exact current agent

Scenario: Canonical task and anchor activation share verified provenance
  Test: native_verified_ingress_task_activation
  Given verified main or thread input and injected transaction failures
  When task intents and exact host acknowledgements are committed
  Then source root task and immutable inputs stay atomic and become executable only after activation

Scenario: Original human follow-up retains task and rotates completion epoch
  Test: native_verified_ingress_followup
  Given a completed direct or mentioned group task
  When fresh stale foreign or background follow-up is scheduled
  Then only eligible unprocessed human work reopens the same task at start

Scenario: Retired context never becomes a fresh route input
  Test: native_verified_ingress_fencing
  Given pending active or completed private work including null-root main sessions
  When privacy membership parent allocation or device authority changes
  Then old observations copies notices and dispatches cannot be retargeted to a new session

Scenario: Copies and frozen input remain independently owned
  Test: native_verified_ingress_copies
  Given the same authenticated room event projected for multiple sessions
  When one task consumes its copied input or another message arrives
  Then other copies and the existing dispatch payload remain unchanged

Scenario: Retry and restart preserve exact ingress and activation identity
  Test: native_verified_ingress_recovery
  Given duplicate conflicting or interrupted observations and task operations
  When the domain owner restarts and retries the exact command
  Then immutable source generation digests and per-session processing survive without duplicate work

Scenario: HTTP execution reaches final intent through actual task activation
  Test: native_runner_http_verified_ingress
  Level: integration
  Test Double: authenticated host fixtures and loopback HTTP
  Given a host-admitted message and acknowledged canonical task intent
  When the runtime reads input completes the task and submits final content
  Then the final intent binds the actual task epoch and runtime forged ingress authority stays inaccessible

## Out of Scope

Real Matrix authenticity crypto network and transport observation remain M5 gates.
Taskless front-desk execution arbitrary first invited groups generalized human or
federated DM policy and production cutover are not part of this bounded slice.

Verified notice claims exercise offline acknowledgement scheduling only. Live
notice sending requires immediate authenticated route validation and durable
begin-send uncertain-outcome custody before this API may be wired to transport.
A stable transaction ID alone cannot prevent a delayed pre-promotion private send.
Automatic unread room-window selection media retrieval and taskless execution
remain outside this input-to-canonical-task proof.
