spec: task
name: "Roll bounded live SDK sync receipts into protected replay history"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-MATRIX-DM-PRIVACY]
tags: [active, rust, matrix, custody]
---

## Intent

The 2026-09-16 real Palpo/Robrix soak stopped at 64 accepted SDK receipts
after six minutes. Preserve the 64-entry live journal bound while retaining
older exact-response and terminal-source evidence in owned encrypted SDK KV
storage. Rollover is not eviction, crypto reset, or restored route authority.

## Constraints

### Must
- Archive one completed oldest receipt before removing it from the live journal.
- Bind immutable archive nodes by authenticated journal root, content digest and exact SDK identity.
- Use compressed binary hash-key trie paths with strictly increasing bit indices and at most 256 branches, proving membership/absence without loading all history.
- Preserve earliest non-target/rejected source decisions across changed targets, crypto trust, and immutable event content; historical proofs grant no current event authority.
- Keep complete pending/Applying/Derived custody, encrypted state ownership and all current negative fences unchanged.
- Keep live receipt count at 64, and bound each archived record and input timeline by existing journal/event limits.
- Persist archive nodes before atomically publishing the journal root/live-cache change; rollback or lost receipts never authorize another SDK apply.
- Refuse missing, corrupt, wrong-identity or structurally invalid nodes before SDK application for the requested replay/source lookup.

### Must Not
- Do not delete historical evidence, change crypto keys, raise live receipt capacity, reconstruct unknown SDK output, invent legacy filtered coverage, or grant approval.
- Do not run live servers/models in tests or place credentials in the tree.

## Boundaries

### Allowed Changes
- native/hagency-matrix/src/sdk.rs
- native/hagency-matrix/src/sdk/sync_history.rs
- native/hagency-matrix/src/event_batch.rs
- native/hagency-matrix/src/event_batch/disposition.rs
- native/hagency-matrix/tests/intake/**
- native/hagency/examples/matrix_custody.rs (operator-only read-only evidence; no SDK/network writes)
- specs/task-rust-matrix-event-rejections.spec.md
- docs/progress.md

## Acceptance Criteria

Scenario: More than 64 exact receipts preserve oldest replay and current cursor
  Test: native_matrix_sync_history_rollover
  Level: integration
  Test Double: owned encrypted SDK SQLite, no network
  Given an exact SDK identity and more than 64 independently accepted responses
  When old receipts roll into protected history and the owner restarts
  Then oldest exact replay does not mutate the current cursor and live state remains bounded

Scenario: Historical terminal sources cannot gain authority
  Test: native_matrix_sync_history_terminal_source
  Level: integration
  Test Double: real offline SDK output and private historical records
  Given archived rejected/non-target sources
  When a source repeats with changed trust, targets or immutable content
  Then the earliest terminal decision still refuses admission

Scenario: Corruption and storage loss never silently remove replay coverage
  Test: native_matrix_sync_history_corruption
  Given an archived proof path or interrupted archive-root commit
  When missing, malformed, foreign-identity or rolled-back records are observed
  Then lookup refuses before SDK application and original completed/pending custody remains inspectable

## Out of Scope

Outgoing/approval/file receipt rollover, operator disk retention, interrupted
SDK reconstruction, transport/session recovery, fleet parity and soak success.
