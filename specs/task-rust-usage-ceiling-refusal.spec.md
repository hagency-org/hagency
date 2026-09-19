spec: task
name: "Name the binding draw in ceiling refusals"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, metering, budget]
---

## Intent

A ceiling refusal must tell the operator which of the two competing draws is
binding, which preset to raise, and which period the measurement belongs to —
byte-faithful to the retained JavaScript wording — and the store must
distinguish unknown capacity (`no_ceiling`) from a known ceiling exceeded
(`over_commit`) without changing the admission decision.

## Constraints

### Must
- Mirror `lib/engagement-store.js:58-103` exactly: `compactTokens` formatting,
  the plain head, the ceiling sentence, the two binding-draw sentences with
  the period-key parenthetical, the conditional cache-read note, and the
  preset/period remedy including the no-preset variant.
- Produce the plain form when no spend context is supplied: a caller without
  a ledger still gets a usable error, never a crash or fabricated breakdown.
- Keep the message generation pure in `hagency-core` with no IO and no
  decision authority.
- Split the store refusal identity: `NoCeiling` for unknown capacity
  (matching the JavaScript `no_ceiling` at engagement-store.js:618) and
  `OverCommit` carrying the wording (matching `over_commit` at :638), while
  the shared-seat resource-pool refusal keeps the existing
  `InsufficientCapacity` identity the JavaScript does not model.
- Leave the admission decision itself unchanged: measured spend joins the
  refusal context in slice 3.
- Pin the wording to the retained JavaScript through
  `native/scripts/ceiling-vectors.mjs` message vectors computed by importing
  `overCommitMessage`, with the source sha256-pinned in the fixture.

### Must Not
- Do not count cache reads toward any enforced figure or imply they drew.
- Do not change any ceiling, seat, or deadline arithmetic.
- Do not grant retry, reply, lease or completion authority in a refusal.
- Do not fabricate a measurement: absent spend stays `None`, never zero.

## Boundaries

### Allowed Changes
- native/hagency-core/src/ceiling.rs
- native/hagency-core/src/lib.rs
- native/hagency-core/tests/ceiling.rs
- native/hagency-store/src/lib.rs
- native/hagency-store/src/domain.rs
- native/hagency-store/tests/domain.rs
- native/hagency-store/tests/resource_configuration.rs
- native/scripts/ceiling-vectors.mjs
- native/hagency-store/tests/fixtures/ceiling-vectors.json
- specs/task-rust-usage-ceiling-refusal.spec.md
- knowledge/decisions/adr-122-native-usage-ceiling-refusal.md
- docs/progress.md
- ./Cargo.lock

## Acceptance Criteria

Scenario: The plain form survives without context
  Test: native_ceiling_refusal_plain_form_without_context
  Given an allocation refusal with no spend context supplied
  When the message is produced
  Then it is the usable plain head exactly as the retained JavaScript emits it

Scenario: The refusal names the binding draw, preset and period
  Test: native_ceiling_refusal_names_binding_draw_preset_and_period
  Given measured fresh spend binding over commitments with cache reads inflating consumption
  When the message is produced
  Then it names the ceiling, both draws, the binding side, the period key, the preset and the cache-read note byte-for-byte

Scenario: The committed mirror names reservations without the cache note
  Test: native_ceiling_refusal_mirror_names_committed_without_cache_note
  Given commitments binding over a smaller equal-consumed measurement
  When the message is produced
  Then it names committed allocations as binding and omits the cache-read note

Scenario: No ceiling is distinct from over-commit
  Test: native_ceiling_no_ceiling_distinct_from_over_commit
  Given one resource whose capacity is unknown and another whose known ceiling the allocation exceeds
  When approve refuses each
  Then the first is the no-ceiling refusal and the second is the over-commit refusal carrying the binding-draw wording
