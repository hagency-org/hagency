spec: task
name: "Persist private owner decisions and exact native approval consumption"
inherits: project
satisfies: [REQ-OWNER-UI-APPROVAL, REQ-EXECUTION-AUTHORIZATION, REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, approval]
---

## Intent

Provide durable host-owned approval authority without enabling native runtime or Matrix transport.

## Constraints

### Must
- Bind every request to current dispatch fence task epoch full owner project private room and host upstream execution context.
- Verify encrypted private room membership and invalidate all shared bindings after negative observations.
- Derive reusable scopes only through the existing pure execution scope implementation.
- Persist verdict grants and content-bound receipts atomically with bounded admission.
- Consume each decision once before native application and preserve possible application uncertainty across restart.
- Revalidate grant revocation capability task and binding at consumption and block resume while any request remains unresolved.
- Keep task grants bounded to the original live task epoch and persistent grants bounded to exact agent workspace environment and binding incarnation.
- Prove exact writable workspace lease before writable context; refuse YOLO until operator policy and runtime enforcement exist.
- Keep public and console projections free of owner room workspace source and request payload details.

### Must Not
- Do not expose host approval context grant or verdict setters to Agent HTTP.
- Do not use chat text shell prefix parsing or upstream echoed identities as authority.
- Do not imply a durable decision proves native runtime application or Matrix delivery.
- Do not rearm a consumed or uncertain decision.

## Boundaries

### Allowed Changes
- native/**
- specs/task-rust-owner-approvals.spec.md
- knowledge/decisions/adr-043-native-owner-approvals.md
- docs/**

### Forbidden
- Root manifests and independent task CLI or protocol implementations.
- Credentials deployed services live models and original dirty checkouts.

## Acceptance Criteria

Scenario: Private observation owns exact Agent approval authority
  Test: native_owner_approval_binding
  Given verified project sessions and a shared owner approval room
  When host snapshots and wrong private verdicts arrive
  Then exact owner authority is required and negative evidence fences every old shared binding

Scenario: Host execution scope cannot substitute for resource custody
  Test: native_owner_approval_context
  Given current capabilities and exact workspace resources
  When host contexts and bounded upstream requests are admitted
  Then writable scope requires its exclusive lease and forged stale or unsupported contexts fail closed

Scenario: Durable approval grants are exact and revocable
  Test: native_owner_approval_grants
  Given pending requests and authenticated owner choices
  When task or always grants are saved reused revoked or invalidated
  Then only matching current incarnations and live task epochs authorize another request

Scenario: Every approval applies once and all unresolved work stays parked
  Test: native_owner_approval_application
  Given multiple pending native requests in one dispatch
  When decisions are consumed applied retried or lost
  Then no decision is rearmed and resume requires every exact application observation

Scenario: Failures retries and expiry preserve durable authority
  Test: native_owner_approval_recovery
  Given injected database failures expired authority and interrupted native application
  When operations retry or the repository reopens
  Then no partial grant survives and uncertain application never becomes a reusable allow

Scenario: Bounded metadata and private projections remain safe
  Test: native_owner_approval_bounds
  Given oversized requests saturated pending work and sensitive private fields
  When admission and console projection are attempted
  Then capacity remains bounded exact retries survive and private identifiers stay absent

## Out of Scope

No live Matrix requests verdict decryption or notice sends. No actual native protocol
allow response unsupported runtime decision mapping model launch or sandbox proof.
YOLO contexts are refused until operator policy and effective runtime enforcement
are integrated. Read-only allows are limited to supported network-only scopes.
Retention rejects new work at capacity; operational compaction remains a later gate.
