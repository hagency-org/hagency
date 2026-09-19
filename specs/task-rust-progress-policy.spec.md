spec: task
name: "Scope native progress filtering and coalescing to one host run"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREE-LAYER-COMPLETION]
tags: [active, rust, runtime, progress]
---

## Intent

Port the pure progress filter and fixed summary vocabulary plus the relevant
reporter coalescing rules into a bounded per-run accumulator. Keep host activity,
progress acceptance, observed answer delivery and canonical task truth separate.

## Decisions

This contract retains the design boundaries in [ADR-026](../knowledge/decisions/adr-026-visible-runner-activity.md).

## Constraints

### Must
- Preserve default and perGroup whole-object replacement policy with exact JavaScript vectors.
- Fail closed on malformed selected rules and use only fixed verbs for unknown tools.
- Redact titles inputs errors paths credentials and raw tool names from emitted text.
- Map ACP starts once and reconcile failures without inflating activity or bypassing exclusion.
- Respect initial terminal ACP status and distinguish unresolved attempts from completed work at finish.
- Bind every input and attempt to immutable host run identity with bounded exact replay receipts.
- Reject gaps changed replay time reversal capacity overflow and retired-run writes without partial state.
- Coalesce steps with the interval floor and open throttle at attempt rather than acceptance.
- Clear only the accepted snapshot and preserve newer work failures and uncertain attempts.
- Keep lifetime totals for truthful finish summaries and accept answer-delivery evidence only from a typed host observation.
- Document corrections separately from unchanged JavaScript behavior and retain all integration gates.

### Must Not
- Do not infer canonical Done delivery or approval from model activity.
- Do not expose room or thread routing setters or reuse a global mutable anchor.
- Do not invoke hooks services or Matrix transports or edit retained JavaScript behavior.

## Boundaries

### Allowed Changes
- native/README.md
- native/hagency-progress/**
- native/scripts/progress-vectors.mjs
- native/fixtures/progress.json
- ./Cargo.toml
- ./Cargo.lock
- .github/workflows/rust.yml
- specs/task-rust-progress-policy.spec.md
- knowledge/decisions/adr-052-native-progress-policy.md
- docs/**

### Forbidden
- Existing runtime MCP Matrix collectors domain schemas live services and credentials.

## Acceptance Criteria

Scenario: Native progress policy preserves valid JavaScript behavior
  Test: native_progress_vectors
  Given exact pure filter mapping and summary oracle cases
  When native policy evaluates them
  Then retained results match and explicit corrections have separate rationale

Scenario: Scoped replay ordering and bounds cannot leak or inflate activity
  Test: native_progress_scope
  Given a host run with bounded observations and exact receipts
  When duplicate changed foreign reordered excessive or retired events arrive
  Then authority and counts remain scoped without partial state
  And emitted text omits raw titles inputs errors paths credentials and routing fields

Scenario: Coalescing retains failed or uncertain snapshots and new work
  Test: native_progress_coalescing
  Given fixed policy and monotonic host time
  When progress attempts fail succeed or become uncertain while new work arrives
  Then throttling starts at attempt and only accepted snapshot contributions retire

Scenario: Final activity summaries preserve failure and delivery uncertainty
  Test: native_progress_summary
  Given filtered ACP calls hooks and explicit host delivery evidence
  When runs finish after prior progress flushes and repeated terminal updates
  Then lifetime totals and once-counted failures produce truthful fixed text without task completion authority
  And unresolved attempts remain explicit even when mixed with failed and completed calls

## Out of Scope

No persistent journal hook installer timers runtime adapter Matrix status edits or
live sending. This is not full ADR026 operational status parity. New runs require
new host identity and fresh accumulators; no in-place reset or serialization can
adopt an old run as current. Unknown send outcomes require host inspection.
