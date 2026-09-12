spec: task
name: "Expose private native send checkpoints and positive journal reconciliation"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-MATRIX-DM-PRIVACY]
tags: [active, rust, replies]
---

## Intent

Supply exact host send checkpoints without granting runtime transport authority.

## Constraints

### Must
- Require the exact claimed secret fence lease and current route for preview.
- Leave preview state unchanged and require Sending for write validation.
- Accept a positive journal observation from Sending only with exact immutable delivery identity.
- Keep NotSent restricted to Uncertain and retain idempotent inspection receipts.
- Keep all three methods host-only and account bounded queue bytes.

### Must Not
- Do not add runner endpoints or claim a preview or validation performed network IO.
- Do not infer NotSent from timeout restart or missing response.

## Boundaries

### Allowed Changes
- native/hagency-store/src/domain/replies.rs
- native/hagency-store/src/domain_worker.rs
- native/hagency-store/tests/replies.rs
- specs/task-rust-reply-send-boundary.spec.md
- specs/task-rust-final-replies.spec.md
- knowledge/decisions/adr-033-native-final-reply-custody.md
- docs/**

## Acceptance Criteria

Scenario: Send checkpoints retain exact claim and route
  Test: native_reply_send_readiness
  Level: integration
  Test Double: fresh offline domain repository
  Given current or stale substituted expired retired transport claims
  When a host previews or validates a send
  Then preview leaves Claimed unchanged and only the current Sending claim validates

Scenario: Positive journal evidence can reconcile Sending
  Test: native_reply_sending_inspection
  Level: integration
  Test Double: typed offline authenticated transport observations
  Given an exact Sending fence and an immutable delivery journal
  When the host submits Delivered or NotSent observations
  Then only matching Delivered reconciles Sending and identical receipts replay once

## Out of Scope

Actual Matrix network IO, canonical completion handoff, runtime configuration,
platform custody and deployment. ADR059 owns the transport consumer separately.
