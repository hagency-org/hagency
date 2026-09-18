spec: task
name: "Native Codex terminal interaction progress"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-LOCAL-THREE-RUNNER-QUALIFICATION]
tags: [active, rust, codex, protocol]
---

## Intent

Investigate the real long-running command's unsupported notification against the
pinned 0.154.0 schema and port its progress semantics without granting permission.

## Constraints

- Original thread/turn and active command item must match before progress.
- Validate bounded process identity and stdin; never log or retain private stdin.
- Progress is not command completion, permission or canonical task completion.
- Unknown methods, foreign items and malformed data remain refusals.
- Offline regressions never invoke Codex. Private opt-in probes preserve failed
  attempts separately from subsequent live success and prove original cleanup.

## Allowed changes

- native/hagency-runtime/src/codex/session/**
- native/hagency-runtime/tests/session.rs
- native/hagency-runtime/tests/session/**
- specs/task-rust-codex-terminal-progress.spec.md
- knowledge/decisions/adr-171-codex-terminal-progress.md
- docs/**

## Scenarios

Scenario: Terminal polling is correlated progress only
  Test: native_codex_session_terminal_progress
  Given an active command item in the current turn
  When the pinned terminal interaction notification arrives
  Then only matching bounded progress is admitted
  And foreign, completed, malformed and unsupported events are refused
