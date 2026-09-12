spec: task
name: "Draw a resource ceiling down by measured fresh tokens only"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-CONTRIBUTION-CONSOLE]
tags: [active, rust, metering, budget]
---

## Intent

Compute what a resource's declared ceiling has left, counting measured fresh
tokens for the current period alongside committed allocations, without turning
unknown evidence into zero or cache reads into spend.

## Constraints

### Must
- Draw a ceiling only against fresh kinds input output and cacheWrite exactly
  mirroring the retained JavaScript ceiling kinds and drawn arithmetic.
- Combine committed reservations and measured spend as the larger of the two
  per period granularity with the period key of the observation clock.
- Treat an absent usage period bucket as unknown so the commitment figure
  stands alone rather than reporting full headroom.
- Keep the display sum of all four kinds alongside the fresh sum so callers
  can show consumption without enforcing on it.
- Report the preset name ceiling tokens period and spend period key so a
  refusal or console never re-derives context.
- Match the retained JavaScript oracle vectors exactly including the cache
  swamp parity case where measured consumption far exceeds drawn fresh tokens.

### Must Not
- Do not count cache reads toward a ceiling at any layer.
- Do not sum committed and measured spend or the same tokens are counted twice.
- Do not enforce refuse or publish decisions in this slice; it computes and
  reports the draw only.
- Do not invent per-agent attribution beyond engagement-derived resource
  membership or trust untrusted evidence as provider measurement.
- Do not turn missing regressed or incomplete evidence into zero headroom.

## Boundaries

### Allowed Changes
- native/hagency-store/src/domain.rs
- native/hagency-store/src/domain/usage.rs
- native/hagency-store/src/domain/usage/reads.rs
- native/hagency-store/src/domain/usage/types.rs
- native/hagency-store/src/lib.rs
- native/hagency-store/tests/usage.rs
- native/hagency-store/tests/usage/**
- native/hagency-store/tests/fixtures/ceiling-vectors.json
- native/scripts/ceiling-vectors.mjs
- .github/workflows/rust.yml
- specs/task-rust-usage-ceiling-draw.spec.md
- knowledge/decisions/adr-121-native-usage-ceiling-draw.md
- docs/agent-knowledge.md
- docs/progress.md
- ./Cargo.lock

## Acceptance Criteria

<!-- lint-ack: bdd-rule-grouping — Each fixture exercises an independent boundary of the read-side ceiling draw. -->

Scenario: Only fresh kinds draw a ceiling
  Test: native_ceiling_draws_fresh_tokens_only
  Given recorded usage where cache reads dominate and cache writes bill above
  fresh input
  When the resource ceiling draw is computed
  Then input output and cacheWrite draw and cache reads never do

Scenario: Draw matches retained JavaScript vectors
  Test: native_ceiling_vectors_match_javascript
  Given exact JavaScript-derived vectors spanning cache swamp no-cache parity
  cacheWrite-only and fresh exhaustion
  When the native draw and consumption figures are computed
  Then they match the oracle including the big cache swamp numbers

Scenario: Unknown measurement is not zero
  Test: native_ceiling_unknown_spend_falls_back_to_commitments
  Given a resource with committed engagements and no usage period bucket for
  the current period
  When the ceiling draw is computed
  Then measured spend stays unknown and the commitment figure stands alone

Scenario: The larger draw wins
  Test: native_ceiling_draw_takes_max_of_committed_and_measured
  Given commitments above measured spend and the mirror case
  When the ceiling draw is computed
  Then the drawn figure is the maximum of the two and each side remains
  separately reportable
