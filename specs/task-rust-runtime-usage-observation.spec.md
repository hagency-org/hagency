spec: task
name: "Retain scoped native Codex usage observations"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, metering, runtime]
---

## Intent

Retain fixed optional usage counters from the actual native session reader without
turning protocol metadata into provider billing or canonical allocation authority.

## Constraints

### Must
- Mint usage only after exact real driver thread turn and lifecycle validation.
- Keep the existing default Update stream and approval refusal behavior unchanged.
- Preserve missing null invalid and unsupported evidence without filling in zero.
- Bound each counter to an exact nonnegative JSON-safe integer.
- Retain cumulative and last-response counters separately without summing them.
- Keep source receipts tied to actual driver identity sequence and retirement.
- Keep raw metadata paths identifiers and text outside the fixed usage projection.
- Let progress consume usage receipts quietly without counting tools or finishing work.

### Must Not
- Do not add transcript readers quota enforcement domain attribution ledger writes or service wiring.
- Do not infer arithmetic consistency provider authenticity task completion or approval application.
- Do not let malformed metric values conceal wrong thread or turn identity.

## Boundaries

### Allowed Changes
- native/hagency-runtime/src/codex/session.rs
- native/hagency-runtime/src/codex/session/driver.rs
- native/hagency-runtime/src/codex/session/observation.rs
- native/hagency-runtime/src/codex/session/usage.rs
- native/hagency-runtime/tests/session.rs
- native/hagency-runtime/tests/session/usage.rs
- native/hagency-progress-runtime/src/lib.rs
- native/hagency-progress-runtime/tests/attachment.rs
- knowledge/decisions/adr-069-native-runtime-usage-observation.md
- specs/task-rust-runtime-usage-observation.spec.md
- docs/plan.md
- docs/progress.md
- docs/agent-knowledge.md

## Acceptance Criteria

Scenario: Real scoped notifications retain separate counters
  Test: native_codex_usage_observation_counters
  Given a real running SessionDriver with pinned token usage notifications
  When the host reads the next observed update
  Then cumulative last response and context window counters retain exact optional values
  And ordinary Update stays Progress without advancing canonical work

Scenario: Missing and invalid counters never become zero
  Test: native_codex_usage_observation_uncertainty
  Given missing null negative unsafe fractional boolean string or future field metadata
  When the actual reader projects the notification
  Then fixed diagnostics retain uncertainty while valid independent fields survive

Scenario: Usage evidence cannot impersonate another session
  Test: native_codex_usage_observation_scope
  Given distinct running drivers with identical textual IDs and wrong-scope notifications
  When events are consumed replayed or the original driver closes
  Then evidence retains exact source sequence and retirement and wrong scope produces no receipt

Scenario: Usage does not alter tool progress accounting
  Test: native_progress_attachment_usage_is_quiet
  Given a current progress attachment and real usage plus tool observations
  When the attachment reads them in order
  Then usage advances receipts without creating tool activity or finished status

## Out of Scope

Counter normalization and consistency policy usage ledger attachment resumed source
attribution provider measurement transcript capture release qualification and live services.
