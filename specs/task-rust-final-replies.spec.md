spec: task
name: "Bind native final replies to current private Matrix routes"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREAD-SCOPED-SESSIONS, REQ-MATRIX-DM-PRIVACY]
tags: [active, rust, replies, privacy]
---

## Intent

Persist bounded final reply intentions independently of external Matrix delivery,
with host-owned session privacy and exact canonical completion authority.

## Constraints

### Must
- Freeze Matrix server room sender device owner registration and privacy generations before a new session can execute.
- Reject final replies from legacy sessions without verified route metadata and from internal sessions.
- Retire old direct routes on promotion including sessions with no thread root.
- Bind runtime reply content to the caller's exact canonical done epoch or inspected report grant.
- Keep one content-bound final intent per task epoch and stable transport transaction identity across retries.
- Separate intent admission transport claim send-start and acknowledged delivery into durable transitions.
- Fence stale leases route generations receipts and capabilities; record uncertain sends after timeout or restart.
- Require host observation before retrying uncertain sends and preserve immutable history.
- Persist cancellation independently so a NotSent observation cannot revive cancelled output.
- Record negative room observations durably and fence absent agents across full group snapshots.
- Keep runtime payloads free of routing owner device generation or delivery authority.
- Keep repository and HTTP tests offline and verify transaction rollback and recovery.

### Must Not
- Do not mark canonical tasks done from reply body or external delivery.
- Do not infer DM privacy from names member counts alone or missing metadata.
- Do not expose a transport claim observation or room binding through the runner API.
- Do not send Matrix traffic start a model read credentials or change live services.

## Boundaries

### Allowed Changes
- native/**
- specs/task-rust-final-replies.spec.md
- knowledge/decisions/adr-033-native-final-reply-custody.md
- docs/**

### Forbidden
- Live deployments, runtime credentials and the original dirty checkout.
- Codex protocol crate and root Cargo manifests.

## Acceptance Criteria

Scenario: Matrix session routes freeze current privacy authority
  Test: native_reply_routes
  Given authenticated host observations and legacy internal or stale sessions
  When new Matrix sessions are admitted and room or device generations change
  Then only a fresh session with exact current route metadata can execute or reply

Scenario: Canonical completion admits a single immutable final intent
  Test: native_reply_intents
  Given current task or inspected report capabilities and injected write failures
  When a runner submits bounded reply content or repeats its call
  Then the exact done epoch owns one content-bound intent atomically without task mutation

Scenario: Private output cannot cross room promotion
  Test: native_reply_promotion
  Given direct main and thread sessions with queued claimed or sending replies
  When the host observes room promotion or owner allocation rotation
  Then old output remains permanently ineligible even without a thread root

Scenario: Transport claims distinguish admission from actual delivery
  Test: native_reply_transport
  Given pending replies and host-owned transport claims
  When leases expire sends begin or exact event acknowledgements arrive
  Then stale or substituted observations fail and only matching delivery records success

Scenario: Interrupted sends require explicit reconciliation
  Test: native_reply_recovery
  Given uncertain sends and backend or runner restart
  When the host observes delivered or definitively not sent
  Then stable transactions survive retries while old generations cannot be resent

Scenario: Private HTTP exposes content-only reply commands
  Test: native_runner_http_replies
  Level: integration
  Test Double: offline repository and loopback HTTP
  Given authenticated runners and forged routing delivery or identity fields
  When final intent submission and receipt reads use the private API
  Then only scoped content commands reach the single writer and host authority remains inaccessible

Scenario: Host preview and write validation retain exact send custody
  Test: native_reply_send_readiness
  Given an exact current claimed reply or a stale substituted or retired claim
  When the host previews routing or checks a Sending claim before IO
  Then preview leaves state unchanged and only current Sending custody validates

Scenario: Journaled delivery can reconcile a still Sending reply
  Test: native_reply_sending_inspection
  Given an authenticated host delivery observation and the exact send fence
  When the sender lost its claim secret before recording the response
  Then matching delivery is idempotent while NotSent and substituted observations are refused

## Out of Scope

Matrix SDK authentication and encryption, network sends, effective process cleanup,
file/media snapshots, operator projections, runtime execution and deployment cutover.
Host observations are deterministic fixture evidence, not proof of a live homeserver.

The bounded route proof accepts a first group only in its project room or a later
promotion of a known DM. Direct humans are current project owners; all observed
members are on the same server and DMs require encryption and invitation-only
access. Arbitrary invited groups, other authorized humans, federated members,
taskless/front-desk final output and actual Matrix task-intent route integration
remain unimplemented. Null-root canonical task coverage does not imply taskless
feature parity. See ADR-033 for the host negative-observation and send contract.
