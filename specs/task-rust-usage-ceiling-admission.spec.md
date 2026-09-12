spec: task
name: "Admit against the drawn ceiling and publish headroom"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, metering, budget]
---

## Intent

Admission compares the requested allocation against what actually drew the
ceiling down — fresh measured tokens combined with commitments — never against
the cache-read-inflated display total, and the authority publishes the
headroom it used so no client re-derives it.

## Constraints

### Must
- Compute the draw as `max(reserved, spent)` per the retained
  `remainingFor` (backend-v2.js:14052-14053): an unknown measurement falls
  back to the commitment figure, never to zero and never to full headroom.
- Compare the request against `min` of the non-null limits: ceiling after
  the draw, the declared seat quota, and the pool; a seat-period mismatch
  nulls the figure as the JavaScript does (backend-v2.js:14057).
- Exclude the engagement being decided from its own commitment sum, exactly
  like the retained `decide()` call (`excludeEngagementId`); approve is the
  operator path, so the auto-join tightening does not apply here.
- Keep cache reads out of every enforced figure while still publishing the
  display total beside the draw.
- Publish the authority's own figures (`tokens_drawn`, `tokens_used`,
  `remaining_tokens`) on the usage report, keeping every existing key.
- Pin the admission expectations to the retained ledger through the
  ceiling oracle (approve 1M at 13.6M consumed / 681k drawn; refuse 1M at
  10M fresh; headroom after approval).

### Must Not
- Do not refuse on consumption that includes cache reads.
- Do not treat an unmeasured period as zero spend.
- Do not change refusal identity beyond what slice 2 established; the
  over-commit message now carries the measured figures but grants no retry,
  reply, lease or completion authority.
- Do not implement the ceiling overrun alarm here; it is a listed follow-up.

## Boundaries

### Allowed Changes
- native/hagency-store/src/domain.rs
- native/hagency-store/src/domain/usage.rs
- native/hagency-store/src/domain/usage/reads.rs
- native/hagency-store/src/domain/usage/types.rs
- native/hagency-store/src/lib.rs
- native/hagency-store/tests/usage.rs
- native/hagency-store/tests/usage/admission.rs
- native/hagency-store/tests/fixtures/ceiling-vectors.json
- native/hagency/tests/usage.rs
- native/scripts/ceiling-vectors.mjs
- specs/task-rust-usage-ceiling-admission.spec.md
- knowledge/decisions/adr-123-native-usage-ceiling-admission.md
- docs/progress.md
- ./Cargo.lock

## Acceptance Criteria

Scenario: Cache-swamped consumption still admits fresh work
  Test: native_ceiling_admission_uses_drawn_not_consumed
  Given a ten million token ceiling with 13.6M consumed of which 681k is fresh
  When a one million allocation is approved
  Then admission succeeds because only fresh tokens drew the ceiling down

Scenario: Fresh exhaustion refuses and names the binding draw
  Test: native_ceiling_admission_refuses_fresh_exhaustion
  Given a ten million token ceiling with ten million fresh tokens measured
  When a one million allocation is approved
  Then it is refused as over-commit naming measured spend as the binding draw

Scenario: Headroom is published after approval
  Test: native_ceiling_publishes_headroom_after_approval
  Given an approved allocation and a cache-read-inflated measurement
  When the usage report is read
  Then tokens_drawn max(reserved, spent), tokens_used the display total and remaining_tokens the authority's own figure are published beside the existing keys
