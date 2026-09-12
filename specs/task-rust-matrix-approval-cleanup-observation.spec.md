spec: task
name: "Observe original approval setup and repository destruction"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, matrix, diagnostics]
---

## Intent

Distinguish original approval SDK setup and teardown and actual domain connection
and ownership-file destruction after the failed Windows1da8f1b suite. Preserve
the original verdicts; their backend causes remain unproven.

## Constraints

### Must
- Attach finite existing traces to the original approval operation and SDK owner with fixed callsite and variant labels.
- Extend only the existing optional per-shutdown snapshot with monotonic connection and ownership-file drop timestamps.
- Preserve connection-before-ownership drop order including unwind cleanup and acknowledgement after both drops.
- Keep normal shutdown free of Probe allocation and keep deterministic pauses private to test builds.
- Preserve all six original approval selectors and exact errors and deadlines.

### Must Not
- Do not add SQLite queries checkpoints waits retries workers queue capacity changes or timeout changes to production cleanup.
- Do not use global latest-operation state payloads credentials room IDs paths or variable backend error messages as observations.
- Do not replace any original failure with a later successful diagnostic or qualify positive Windows staging from typed refusal.

## Boundaries

### Allowed Changes
- native/hagency-matrix/src/approval_intake.rs
- native/hagency-matrix/tests/approval_intake/mod.rs
- native/hagency-store/src/domain.rs
- native/hagency-store/src/domain_worker.rs
- native/hagency-store/src/shutdown.rs
- knowledge/decisions/adr-099-native-matrix-approval-cleanup-observation.md
- specs/task-rust-matrix-approval-cleanup-observation.spec.md
- docs/agent-knowledge.md
- docs/progress.md

## Acceptance Criteria

Scenario: Wrong-device cleanup retains its original shutdown result
  Level: integration
  Test Double: actual authenticated local TLS refusal and domain writer
  Test: native_matrix_approval_identity_wrong_device_fences_only_approval_and_purpose_cannot_adopt
  Given the original approval identity and ordinary-route assertions
  When original approval cleanup returns or fails
  Then its fixed SDK and repository destruction observations accompany the unchanged result

Scenario: Framing refusal identifies original SDK bootstrap and fixed variant
  Level: integration
  Test Double: actual approval SDK and original local TLS framing cases
  Test: native_matrix_approval_bounds_sync_framing_and_body_limits_do_not_advance_crypto_cursor
  Given the four original framing cases
  When their original fixture opens its SDK owner
  Then opening evidence names that same bounded variant without changing refusal assertions

Scenario: Corrupt protected history identifies the original SDK close
  Level: integration
  Test Double: actual corrupted encrypted approval journal
  Test: native_matrix_approval_bounds_corrupt_encrypted_journal_cannot_resume_authority
  Given the nine original corruption cases
  When their original owner closes before restoration
  Then the fixed variant and original close phases remain separate from the later restoration refusal

Scenario: Journal acknowledgement rollback retains original recovery assertions
  Level: integration
  Test Double: original SDK journal rollback and exact domain receipt
  Test: native_matrix_approval_recovery_ack_journal_rollback_requires_reopen_and_exact_receipt
  Given original approval bootstrap and recovery
  When setup or teardown fails
  Then only fixed original-operation evidence is added and the result is unchanged

Scenario: Busy negative room recovery retains original refusal assertions
  Level: integration
  Test Double: original busy writer and negative room observation
  Test: native_matrix_approval_recovery_busy_then_negative_room_settles_unaccepted_without_network
  Given original approval bootstrap and recovery
  When setup or teardown fails
  Then only fixed original-operation evidence is added and the result is unchanged

Scenario: Lost commit recovery retains original private rotation assertions
  Level: integration
  Test Double: original lost domain reply and rotated private approval owner
  Test: native_matrix_approval_recovery_lost_commit_reopens_exact_receipt_after_private_rotation
  Given original approval bootstrap and recovery
  When setup or teardown fails
  Then only fixed original-operation evidence is added and the result is unchanged

Scenario: Approval lifecycle evidence follows the actual retained SDK owner
  Level: integration
  Test Double: actual SDK owner paused at original open and close boundaries
  Test: native_matrix_approval_observation_owner_lifecycle
  Given an actual approval SDK owner and fixed operation traces
  When original opening or spawned Collector close is held at its existing SDK boundary
  Then original phase observations agree with retained private lock ownership and release only with original completion

Scenario: Connection destruction remains inside the original retained owner
  Level: integration
  Test Double: actual domain writer paused before connection drop
  Test: native_domain_shutdown_connection_drop
  Given an original queued shutdown whose writer has reached connection destruction
  When the original caller deadline expires while that boundary is held
  Then the original uncertainty and snapshot remain unchanged while release later permits a real repository reopen

Scenario: Ownership release follows completed connection destruction
  Level: integration
  Test Double: actual domain writer paused before ownership-file drop
  Test: native_domain_shutdown_ownership_drop
  Given the original connection drop has completed
  When the original writer is held before ownership-file drop
  Then a competing repository remains locked and only original cleanup releases ownership after the unchanged timeout

Scenario: Original enqueue and pre-pickup uncertainty remain distinct
  Level: integration
  Test Double: actual held domain writer and bounded full queue
  Test: native_domain_shutdown_queue
  Given original bounded shutdown queue admission
  When enqueue or pre-pickup waiting exceeds the original deadline
  Then missing destruction phases remain unobserved and no retry replaces the original result

Scenario: Original repository and acknowledgement observations remain intact
  Level: integration
  Test Double: actual writer held at original repository or acknowledgement boundary
  Test: native_domain_shutdown_phases
  Given original repository drop and acknowledgement phase evidence
  When the original caller loses its acknowledgement
  Then the extended snapshot preserves all prior phase meaning and the exact uncertain result

Scenario: Extended snapshots remain finite and independent
  Level: unit
  Test Double: original fixed snapshot publication fixture
  Test: native_domain_shutdown_snapshot
  Given independent original shutdown probes
  When generic worker phases are published without domain field markers
  Then domain-specific fields remain absent and the snapshot fits its explicit208-byte bound

## Out of Scope

Production timing corrections backend root-cause claims broader approval intake
instrumentation hosted reruns workflow changes and Windows durability upgrades.
