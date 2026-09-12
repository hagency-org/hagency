spec: task
name: "Bound the private store close path"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, sqlite, windows]
---

## Intent

Remove the optional close-time checkpoint and auxiliary-file unlinks from the
private SQLite stores so that the observed hosted Windows shutdown stall loses
every mechanism it has been placed in, without changing any shutdown budget,
verdict or phase meaning (ADR-120).

## Constraints

### Must
- Set checkpoints-on-close off through the pinned safe rusqlite configuration API on the original connection immediately after open.
- Keep every shutdown wait phase meaning outcome value destruction order and the OutcomeUnknown verdict unchanged.
- Prove with a real repository that a completed close leaves a non-empty WAL and the SHM in place and that reopening replays them to the same canonical state.
- Keep committed durability on synchronous FULL at commit and say so.

### Must Not
- Do not widen a deadline add a retry serialize tests ignore tests or fabricate an acknowledgement.
- Do not use file-control FFI change SQLite versions flags packages or the unsafe policy.
- Do not claim the hosted Windows stall is fixed before a whole-package probe shows it absent.

## Boundaries

### Allowed Changes
- native/hagency-store/src/database.rs
- native/hagency-store/tests/close_path.rs
- knowledge/decisions/adr-120-native-bounded-close-path.md
- specs/task-rust-native-bounded-close-path.spec.md
- docs/progress.md
- docs/agent-knowledge.md

### Forbidden
- Shutdown budgets phases verdicts the ownership lock the schema migrations another checkout and live data.

## Acceptance Criteria

Scenario: A completed close leaves the WAL for replay instead of checkpointing
  Test: native_store_close_leaves_wal_for_replay
  Level: integration
  Test Double: real private domain repository in a temporary directory
  Given a repository with committed registration and resource rows
  When the repository is dropped normally
  Then the WAL holds committed frames and the SHM is still present
  And reopening the same state directory reports the same catalog

## Verification

The hosted whole-package Windows probe (`.github/workflows/windows-probe.yml`,
every `hagency` test target, eight threads, at least four iterations) is the
reproduction vehicle. Its verdict is evidence for or against ADR-120; a
surviving stall points at the SHM unmap and its process-global mutex.
