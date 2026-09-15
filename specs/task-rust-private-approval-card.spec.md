spec: task
name: "Freeze native private owner approval cards from one current domain observation"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-OWNER-UI-APPROVAL, REQ-EXECUTION-AUTHORIZATION]
tags: [active, rust, approvals, private]
---

## Intent

Implement ADR110's original-domain card prerequisite in the clean worktree at
492bc59. An opaque host packet binds current pending private authority and exact
operation metadata in one transaction. It is not delivery or execution authority.
SDK enrollment, encrypted sending and executable coordination follow separately.

## Constraints

### Must
- Read the current pending request and private target in one original domain transaction using the post-lock writer clock.
- Keep the immutable host owner cutoff earlier than or equal to domain request expiry and refuse expired cards.
- Render the existing structured approval request fields with the exact native request and digest, engagement and project identifiers, upstream request type, operation and private workspace scope.
- Offer reusable actions only for the exact persisted reusable scope; never derive broader rules from display text.
- Bound encoded card bytes before returning; refuse oversize without truncating input, identifiers or scope.
- Revalidation compares the complete original packet with a fresh current snapshot and cannot turn historical metadata into send or decision authority.
- Preserve all existing request, response, room, grant and execution semantics and record truthful test evidence.

### Must Not
- No public API, arbitrary room override, mutable packet constructor, serde deserialization, new database schema or live service changes.
- No SDK transport, native response, runtime resume, delivery acknowledgement, Robrix compatibility or complete migration claim.

## Boundaries

### Allowed Changes
- native/hagency-store/src/domain/approvals.rs
- native/hagency-store/src/domain/approvals/card.rs
- native/hagency-store/src/domain.rs
- native/hagency-store/src/domain_worker.rs
- native/hagency-store/src/lib.rs
- native/hagency-store/tests/approvals.rs
- native/hagency-store/tests/approvals/card.rs
- specs/task-rust-private-approval-card.spec.md
- knowledge/decisions/adr-110-native-private-approval-card.md
- docs/progress.md
- docs/agent-knowledge.md

### Forbidden
- No core permissions runtime execution Matrix console or production deployment changes.

## Acceptance Criteria

Scenario: A pending card contains its exact private request and reusable scope
  Test: native_private_approval_card_content
  Given actual current native pending approvals
  When the original domain creates a private card
  Then request fields and actions exactly match stored scope and no expiry reserve is offered as owner time

Scenario: Changed private authority or decision refuses a card
  Test: native_private_approval_card_authority
  Given pending cards and the actual bound owner room and execution context
  When scope is retired promoted decided or expired
  Then fresh card creation and revalidation refuse without a new decision or response

Scenario: Card encoding never silently truncates private operation metadata
  Test: native_private_approval_card_capacity
  Given native requests within storage capacity and malformed card inputs
  When the encoded card exceeds its event budget or owner cutoff is invalid
  Then the packet refuses without mutating the request or widening the authority

Scenario: Original writer queue and lock waits use a fresh card cutoff
  Test: native_private_approval_card_clock
  Given the original SQLite lock and queued domain worker
  When an owner cutoff expires during contention
  Then no stale pending card is returned and the original request remains unchanged

## Out of Scope

This prerequisite does not send a card. Native request IDs currently have forty
hexadecimal characters while the retained client parser expects thirty-two;
client protocol qualification and approval-purpose SDK enrollment/delivery are
explicit remaining gates.
