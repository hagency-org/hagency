spec: task
name: "Explicitly refuse conclusively pre-session Matrix custody without retry"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-MATRIX-DM-PRIVACY, REQ-THREAD-SCOPED-SESSIONS]
tags: [active, rust, matrix, operator, custody]
---

## Intent

The seventh synthetic 2026-09-16 soak request appears to predate native
generation 9's new session adoption. It is retained as known SDK output,
quarantined after domain admission refused its frozen scope. The command must
prove the exact original boundary rather than assume that timing. Provide an explicit
local operator refusal, not a retry, retarget, automatic recovery or task success.

## Constraints

### Must
- Require an exact 64-lowercase-hex batch digest and owner-private existing state.
- Require Quarantined with the exact domain-frozen-scope refusal category and known persisted SDK Derived evidence; never settle Prepared/Applying/coverage/crypto ambiguity.
- For every unacknowledged candidate, require an opaque domain-minted negative proof that binds the complete original observation and proves its source timestamp predates the original frozen session ingress boundary.
- Refuse a source with an existing canonical commit, changed frozen identity/content, missing ingress boundary, or non-stale timestamp; do not conflate general RunnerAuthority with stale timestamp.
- Preserve every acknowledged candidate and all original raw/source/SDK digest coverage; persist terminal StaleSession decisions before the ordinary completed receipt.
- Keep the original encrypted SDK identity, live/archive receipt limits, private state ownership, fences and uncertain-effect records.
- Permit identical repeat only from protected settled negative evidence; lost commit remains uncertain until inspected/reopened, never authorize another SDK apply.
- Keep the command explicit and operator-only. Normal collect/startup must not invoke it automatically.

### Must Not
- Do not admit or project rejected input, start a model, mark a task done, deliver a reply, retry effects, re-enqueue work, clear lease/workspace quarantine, or revive a retired route.
- Do not infer a negative proof from a generic error, a count, a raw HTTP field, or a user assertion.
- Do not contact real services in tests; real operator qualification remains separate.

## Boundaries

### Allowed Changes
- native/hagency-store/src/domain/verified_ingress.rs
- native/hagency-store/src/domain.rs
- native/hagency-store/src/domain_worker.rs
- native/hagency-store/src/lib.rs
- native/hagency-matrix/src/intake.rs
- native/hagency-matrix/src/event_batch.rs
- native/hagency-matrix/src/event_batch/disposition.rs
- native/hagency-matrix/src/sdk.rs
- native/hagency-matrix/tests/intake/**
- native/hagency/src/bootstrap.rs
- native/hagency/src/bootstrap/intake_refusal.rs
- native/hagency/src/main.rs
- knowledge/decisions/adr-151-native-stale-session-refusal.md
- docs/progress.md

## Acceptance Criteria

Scenario: Proven pre-session SDK candidate is terminally refused without projection
  Test: native_matrix_stale_session_refusal
  Level: integration
  Test Double: real encrypted owned SDK and native domain SQLite, scripted local TLS peer only
  Given retained known SDK-derived quarantine and a source older than its exact session boundary
  When the explicit operator command verifies all unacknowledged candidates
  Then the source is durably rejected, raw coverage is preserved, the completed receipt advances normally, and no task/input/effect is retried

Scenario: Ambiguity and current or committed sources remain quarantined
  Test: native_matrix_stale_session_refusal_negative
  Given wrong digest, non-stale source, existing canonical receipt, wrong scope, or SDK Applying uncertainty
  When refusal is requested
  Then protected custody remains unresolved and no caller claim becomes a negative proof

## Out of Scope

General quarantine resolution, Applying reconstruction, orphaned-dispatch resume
(ADR-148), source retargeting, automatic backlog replay, and entire-port/soak success.
