spec: task
name: "Attach redacted progress to exact native Codex observations"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREAD-SCOPED-SESSIONS, REQ-THREE-LAYER-COMPLETION]
tags: [active, rust, progress, runtime]
---

## Intent

Attach the bounded progress policy to actual typed Codex session observations
without turning runtime activity into task truth process custody or delivery.

## Decisions

This contract retains the design boundaries in [ADR-026](../knowledge/decisions/adr-026-visible-runner-activity.md).

## Constraints

### Must
- Establish one immutable host RunId after exact upstream thread and turn are Running.
- Bind private observation receipts to the exact driver instance thread turn sequence and item.
- Preserve the existing default Update and OwnedSession lifecycle behavior.
- Derive command success only from completed lifecycle explicit completed status and exact zero exit code.
- Keep missing contradictory or unsupported tool evidence unresolved or explicitly gated.
- Honor the same event and tool filters for starts completions and failures.
- Emit only fixed verbs counts and lifecycle text without raw commands inputs output errors paths or reasoning.
- Retire projection irreversibly on cancellation stale source ordering conflict or capacity exhaustion.
- Retain the borrowed runtime and process owner and avoid canonical execution or Matrix side effects.
- Bound observation receipts item counts sequences and host times without silent truncation.

### Must Not
- Do not fabricate ACP completion statuses or accept caller JSON as scoped runtime observations.
- Do not expose routing setters live sends domain writes or execution-worker integration.
- Do not claim canonical Done independent filesystem proof or answer delivery from a completed item or turn.
- Do not attach late or reuse a retired RunId for a replacement connection or turn.

## Boundaries

### Allowed Changes
- ./Cargo.toml
- ./Cargo.lock
- native/hagency-progress-runtime/**
- native/hagency-progress/src/**
- native/hagency-progress/tests/**
- native/hagency-runtime/src/codex/session.rs
- native/hagency-runtime/src/codex/session/**
- native/hagency-runtime/src/owned/session.rs
- native/hagency-runtime/tests/session.rs
- knowledge/decisions/adr-056-native-progress-attachment.md
- specs/task-rust-progress-attachment.spec.md
- docs/**

### Forbidden
- Execution workers domain schemas runtime launch policy Matrix sends existing JavaScript behavior live services and credentials.

## Acceptance Criteria

Scenario: Actual session observations retain exact source authority
  Test: native_progress_attachment_source
  Level: integration
  Test Double: actual SessionDriver with bounded offline Rust protocol peer
  Given two running drivers with matching textual thread and turn IDs
  When an attachment sees foreign late gapped duplicate or changed observations
  Then only exact driver receipts are accepted and invalid scope irreversibly retires projection

Scenario: Tool summaries require explicit result evidence
  Test: native_progress_attachment_evidence
  Level: integration
  Test Double: real SessionDriver streams with pinned protocol tool lifecycle fields
  Given command and file-change lifecycles with completed failed missing or contradictory evidence
  When the native attachment projects their progress
  Then fixed summaries separate completed failed and unresolved attempts
  And default perGroup and tool exclusion policies remain enforced

Scenario: Cancellation preserves runtime ownership and retires projection
  Test: native_progress_attachment_cancellation
  Level: integration
  Test Double: actual native child through OwnedSession and deterministic pending streams
  Given a borrowed session and pending local progress attempt
  When its observation future is cancelled or its source is retired
  Then the runtime owner remains retained and the old projection cannot emit or retarget

Scenario: Native pipe activity remains redacted and bounded
  Test: native_progress_attachment_owned_pipes
  Level: integration
  Test Double: offline native app-server process through the existing owned pipes
  Given hostile raw fields and supported and unsupported tool kinds
  When the host drives the exact turn to its protocol end
  Then local text contains only fixed progress vocabulary and gated evidence is explicit
  And no task completion or Matrix receipt is inferred

Scenario: Typed tool entry preserves progress invariants
  Test: native_progress_typed_tools
  Level: unit
  Test Double: typed host observations and exact replay sequences
  Given bounded typed starts terminal updates and accepted pending notices
  When observation order filters or replay contents vary
  Then failures and completions are counted once without bypassing policy or losing uncertainty

## Out of Scope

Domain authority qualification physical workspace ownership launch policy global
scheduling execution-worker attachment live models approval application durable
status outbox Matrix routing and sends remain separate gates. Run identities are
host-established and in-memory only; no persistence reset or recovery import.
