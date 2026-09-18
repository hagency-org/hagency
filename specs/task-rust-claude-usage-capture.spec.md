spec: task
name: "Capture source-bound native Claude usage without duplicate token accounting"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-LOCAL-THREE-RUNNER-QUALIFICATION]
tags: [active, rust, claude, metering]
---

## Intent

Connect bounded native Claude usage observations to the existing private owned
dispatch ledger, without enabling an incompletely integrated production runner.

## Constraints

### Must
- Mint immutable observations only after actual session admission, before exposing mutable messages.
- Bind source identity to the original driver and validated session, not peer text alone.
- Attach immediately after system init; sequence every accepted message on all read/control paths.
- Retire sources on close, failure and Drop; never use results as cleanup or canonical completion.
- Deduplicate main-loop assistant input/cache counts by nested message ID with finite retention.
- Leave per-step output unknown; replace cumulative snapshots with result totals, never add them.
- Label result model totals separately from main-loop fallback and preserve missing/invalid counters.
- Bound event, identity and model counts and check known arithmetic even when other counts are unknown.
- Keep ledger attribution from acknowledged Started scope and retain exact pending writes before awaiting.
- Preserve Codex and transcript behavior, and label every runtime observation incomplete.

### Must Not
- Do not trust billing estimates, model metadata or transcript paths as attribution or quota authority.
- Do not add production Claude admission, credentials, account readiness, cleanup claims or live tests.

## Boundaries

### Allowed Changes
- native/hagency-runtime/src/claude/session.rs
- native/hagency-runtime/src/claude/session/observation.rs
- native/hagency-runtime/src/claude/session/usage.rs
- native/hagency-runtime/src/owned/claude.rs
- native/hagency-runtime/src/bin/claude_probe/mod.rs
- native/hagency-runtime/tests/claude_usage.rs
- native/hagency-runtime/tests/claude_owned.rs
- native/hagency-metering/src/lib.rs
- native/hagency-metering/src/observation.rs
- native/hagency-metering/src/claude_usage.rs
- native/hagency-metering/tests/claude_usage.rs
- native/hagency-execution/src/usage.rs
- native/hagency-execution/src/usage/**
- native/hagency-execution/tests/support/usage.rs
- native/hagency-execution/tests/support/claude_usage.rs
- knowledge/decisions/adr-157-native-claude-usage-capture.md
- specs/task-rust-claude-usage-capture.spec.md
- docs/progress.md
- docs/agent-knowledge.md

## Acceptance Criteria

Scenario: Native observations retain original source and exact ordering
  Test: native_claude_usage_source_and_order
  Level: integration
  Test Double: bounded synthetic bidirectional native streams
  Given matching textual IDs different drivers mutable messages and control returns
  When every message is consumed or an original driver closes or drops
  Then immutable evidence stays source-bound every read advances sequence and retirement remains visible

Scenario: Main-loop steps and result snapshots are not added twice
  Test: native_claude_usage_projection
  Level: integration
  Test Double: actual JSONL decoder with duplicate assistant blocks and result frames
  Given repeated nested IDs subagent frames missing counters and an error result
  When runtime projects numeric evidence
  Then each main step counts once output stays unknown until result and model totals replace step sums

Scenario: Conflicting identities and numeric capacity remain explicit
  Test: native_claude_usage_bounds
  Level: integration
  Test Double: exact integer limits finite identity maps and malformed metrics
  Given conflicting step counters invalid fields too many models or known subtotal overflow
  When bounded projection runs
  Then capture invalidates or retains explicit unknown counters without invented zero or unbounded memory

Scenario: Typed Claude normalization preserves coverage and legacy shapes
  Test: native_metering_claude_runtime
  Level: unit
  Test Double: fixed untrusted numeric DTOs
  Given step main-loop and reported-model totals plus invalid counts
  When typed observations normalize
  Then cache categories stay separate placeholder output stays unknown evidence is versioned incomplete and overflow refuses

Scenario: Actual original process pipes mint Claude usage observations
  Test: native_claude_owned_usage
  Level: integration
  Test Double: native offline peer through the original guardian
  Given a live owned peer emitting usage and a result while remaining alive
  When usage is observed before explicit stop
  Then source matching holds and the result grants no process cleanup proof

Scenario: Private capture records Claude snapshots under original dispatch attribution
  Test: native_owned_claude_usage_capture
  Level: integration
  Test Double: native streams and actual domain writer
  Given acknowledged Claude Started scope source guards and a retained result write
  When private capture normalizes records and retries the original pending tuple
  Then durable high-water accounting does not double count and no task or cleanup authority is created

## Decisions

ADR157 defines this slice. Production Host remains closed to Claude until scoped
tools, private owner authorization, account binding and platform ownership gates
are separately satisfied. Ordinary tests never launch a provider or contact Palpo.
