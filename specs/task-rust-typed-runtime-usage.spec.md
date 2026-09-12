spec: task
name: "Normalize bounded typed untrusted runtime usage"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, metering]
---

## Intent

Normalize the pinned Codex runtime counter shape without fabricating transcript
records or granting counters source attribution or measurement authority.

## Constraints

### Must
- Keep fixed typed input explicitly untrusted and independent from runtime and store crates.
- Preserve optional cumulative last response context and projection diagnostics in versioned evidence.
- Sanitize unsafe individual counters to unknown and preserve independent valid counters.
- Separate fresh input cache reads cache writes and output without adding reasoning again.
- Use checked subtraction and mark contradictory totals and breakdowns visibly.
- Keep runtime stream coverage incomplete even when every counter is present.
- Hash the complete sanitized typed evidence with a version discriminator.
- Preserve existing transcript parsing counts and serialized observation shape.
- Refuse known cumulative normalized arithmetic overflow explicitly without a partial observation.

### Must Not
- Do not mint source identity current dispatch authority provider authenticity or complete capture proof.
- Do not construct transcript JSON or add runtime store HTTP ledger or live service integration.
- Do not sum cumulative snapshots last response reasoning or context as additional usage.
- Do not retain raw text paths identifiers credentials or unknown payload fields.

## Boundaries

### Allowed Changes
- native/hagency-metering/src/lib.rs
- native/hagency-metering/src/observation.rs
- native/hagency-metering/src/runtime_usage.rs
- native/hagency-metering/tests/runtime_usage.rs
- knowledge/decisions/adr-071-native-typed-runtime-usage.md
- specs/task-rust-typed-runtime-usage.spec.md
- docs/agent-knowledge.md
- docs/progress.md

### Forbidden
- Runtime execution store schemas manifests existing JavaScript behavior other worktrees and live data.

## Acceptance Criteria

Scenario: Pinned runtime cache writes remain separate from fresh input
  Test: native_metering_runtime_pinned_categories
  Level: unit
  Test Double: fixed counter DTO from the pinned upstream SSE conversion fixture
  Given input100 cached40 cachewrite60 output10 reasoning5 and total110
  When typed runtime usage is normalized
  Then fresh input is zero and displayed volume is110 with no duplicate cache write or reasoning
  And last response and context capacity never increase cumulative usage

Scenario: Unknown and invalid fields preserve independent evidence
  Test: native_metering_runtime_unknown_and_invalid
  Level: unit
  Test Double: bounded optional counters and unsafe integer boundary values
  Given absent fields unsafe values or projection uncertainty
  When normalization sanitizes the typed input
  Then invalid fields become unknown and independent valid categories remain
  And the retained evidence always marks stream coverage incomplete

Scenario: Contradictory cumulative observations are not repaired into truth
  Test: native_metering_runtime_contradictions_and_resets
  Level: unit
  Test Double: impossible input and reasoning breakdowns and upstream context reset shape
  Given contradictory totals decreasing cumulative values or an estimated context reset
  When independent typed snapshots are normalized
  Then contradictions remain diagnosed and impossible fresh input stays unknown
  And no cumulative total last response or context value becomes an added delta

Scenario: Typed evidence identity is complete and explicitly bounded
  Test: native_metering_runtime_digest_and_bounds
  Level: unit
  Test Double: field by field evidence changes and exact arithmetic boundaries
  Given equal changed or oversized fixed counter evidence
  When observations are digested or cumulative normalized categories exceed the exact bound
  Then equal sanitized evidence has equal identity and every retained field affects identity
  And overflow returns an error without a partial ledger observation

Scenario: Legacy transcript serialization and normalization remain unchanged
  Test: native_metering_runtime_legacy_shape
  Level: unit
  Test Double: retained parser fixture and exact serialized observation fields
  Given a legacy Codex transcript
  When the existing parser creates an observation
  Then counts diagnostics digest and JSON shape remain unchanged without a runtime evidence field

## Out of Scope

The parent ADR070 adapter must bind actual fresh source instances and dispatch
authority before ledger recording. This pure input can be constructed by anyone
and establishes neither provenance nor complete usage coverage. Overflow refusal
requires that adapter to retain its original source evidence and report failure.
