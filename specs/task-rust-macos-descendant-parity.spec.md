spec: task
name: "Port the working TS descendant-stop path to native macOS"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-LOCAL-THREE-RUNNER-QUALIFICATION, REQ-THREE-LAYER-COMPLETION]
tags: [active, rust, platform]
---

## Intent

Unblock the existing local Codex message-to-reply path by porting the behavior of
router/src/runner-guardian.ts and owned-process-tree.ts, using native lifetime
identities and their existing process regression cases. Do not substitute leader
exit for descendant observation or claim kernel crash containment.

## Constraints

### Must
- Start the actual executable suspended, bind its native identity and baseline
  census before allowing user code to run, without a shell or extra inherited fd.
- Track descendant ancestry continuously and retain it after reparenting.
- Signal only original lifetime identities; PID metadata alone grants no authority.
- Stop and inspect detached descendants while preserving unrelated processes.
- Keep lost inspection, unclassified new ancestry, capacity and deadline outcomes
  explicitly unknown. No fresh census repairs an earlier tracking gap.
- Preserve Linux subreaper and Windows Job behavior and existing bounded waits.
- Run actual local native process fixtures and then the local runtime path.

### Must Not
- Do not contact models/Palpo from ordinary tests, change sandbox defaults,
  claim a task completed from process exit, or hide existing qualification failures.
- Do not treat unsupported kqueue NOTE_TRACK as a working native facility.

## Boundaries

### Allowed Changes
- native/hagency-platform/src/lib.rs
- native/hagency-platform/src/unix.rs
- native/hagency-platform/src/supervisor/unix.rs
- native/hagency-platform/src/supervisor/unix/macos.rs
- native/hagency-platform/src/supervisor/unix/macos/**
- native/hagency-platform/src/bin/hagency-platform-probe.rs
- native/hagency-platform/tests/guardian.rs
- native/hagency-platform/tests/descendants.rs
- native/**/tests/**
- native/hagency-runtime/src/bin/claude_probe/**
- native/hagency-runtime/src/codex/session.rs
- native/hagency-runtime/src/codex/session/state.rs
- native/hagency-runtime/src/codex/session/driver.rs
- native/hagency-runtime/src/codex/session/task_mcp.rs
- native/hagency-runtime/src/codex/session/hooks.rs
- knowledge/decisions/adr-036-native-codex-session.md
- native/hagency-execution/qualification/source_digests.rs
- native/hagency-execution/qualification/codex-sandbox-0.154.0.json
- knowledge/decisions/adr-029-native-process-scopes.md
- specs/task-rust-macos-descendant-parity.spec.md
- docs/progress.md
- docs/agent-knowledge.md
- docs/design/native-execution-parity.md

## Acceptance Criteria

Scenario: Native tracking preserves ancestry and refuses missing evidence
  Test: native_macos_descendant_tracking
  Given baseline foreign processes original root and later descendant observations
  When parents exit or identities change or a census loses ancestry
  Then only proven descendants are owned and uncertainty remains sticky

Scenario: Native launch establishes identity before work and seals descriptors
  Test: native_guardian_start_stop
  Given explicit Unicode argv cwd and inherited foreign descriptors
  When the original guardian starts and stops work
  Then exact arguments arrive with no guardian channel and unrelated work survives

Scenario: Detached and reparented tools stop with their original owner
  Test: native_macos_descendant_stop
  Given the TS still-parented and already-reparented descendant cases
  When the native owner cancels its process tree
  Then all owned progress stops and the unrelated process remains alive

Scenario: Missing ancestry cannot produce a positive stop receipt
  Test: native_macos_descendant_unknown
  Given a deliberately lost discovery observation
  When original known processes are stopped
  Then leader exit remains separate from unproven descendant cleanup

Scenario: Local operator configuration cannot widen the requested thread policy
  Test: native_codex_session_settings
  Given provider-owned login and existing operator sandbox settings
  When the native host opens or resumes the same scoped thread
  Then it explicitly disables workspace network access and extra writable roots
  And reported policy widening still fails before a prompt is sent

Scenario: Local Codex hook notices preserve their exact thread and turn
  Test: native_codex_session_hook_notices
  Given the installed 0.154.0 synchronous hook notification schema
  When notices race a thread or turn response or contain substituted scope
  Then bounded matching notices are diagnostic only and foreign scope fails

## Decisions

Extend ADR029 with the TS behavior baseline and native macOS implementation.
Process observations are not a filesystem/network sandbox or a guarantee against
guardian death. The real local Palpo/Robrix run remains required after this link.
