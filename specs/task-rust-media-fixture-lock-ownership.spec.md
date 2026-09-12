spec: task
name: "Inspect private media fixtures without escaping their journal lock owner"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, media, windows, tests]
---

## Intent

Correct independent-handle test reads rejected by Windows mandatory journal
locking while preserving actual byte-level persistence and custody assertions.

## Constraints

### Must
- Read a live journal through its original locked Store handle and restore the cursor.
- Preserve exact ciphertext identity corruption and incomplete-write assertions.
- Retain explicit unsupported sync outcomes and original failed CI evidence.

### Must Not
- Do not unlock production files alter sharing rights add retries or weaken assertions.
- Do not classify actual Windows qualification from cross-compilation or other platform tests.
- Do not claim to fix unrelated Matrix timeouts or Palpo shutdown uncertainty.

## Boundaries

### Allowed Changes
- native/hagency-media-store/src/tests.rs
- native/hagency-media-store/src/tests/restoration.rs
- native/hagency-media-store/src/tests/preparation.rs
- knowledge/decisions/adr-081-native-media-fixture-lock-ownership.md
- specs/task-rust-media-fixture-lock-ownership.spec.md
- docs/progress.md
- docs/agent-knowledge.md

## Acceptance Criteria

Scenario: Exact encrypted restoration keeps the original lock and bytes
  Test: native_media_restore_exact_ciphertext
  Given actual SDK encrypted content committed in a private locked Store
  When the fixture checks journal bytes while the owner is alive and after reopen
  Then checks use the existing handle and preserve exact restored content or actual sync refusal

Scenario: Identity refusal cannot change a live locked journal
  Test: native_media_restore_identity_refusal
  Given distinct committed operations in the original locked Store
  When wrong namespace operation kind or digest is requested
  Then the original bytes remain equal under owner-handle inspection

Scenario: Actual corrupt and interrupted journals retain their evidence
  Test: native_media_restore_corruption_and_pending
  Given actual byte corruption or interrupted staging
  When the original owner and validated reopened owner inspect their journal
  Then existing recovery refusals and exact unchanged bytes are asserted

Scenario: Prepared commitment precedes mutation under the original owner
  Test: native_media_prepare_exact_identity
  Given actual encrypted material prepared before journal IO
  When another record intervenes before the prepared write
  Then owner-handle comparisons retain the original identity and exact ciphertext

Scenario: Prepared capacity refusal preserves locked storage
  Test: native_media_prepare_capacity
  Given an exhausted result pool or journal capacity
  When prepared admission or staging refuses
  Then original media and journal bytes remain unchanged

Scenario: Another owner cannot consume prepared staging
  Test: native_media_prepare_owner_identity
  Given a prepared operation from the original Store
  When another or reopened Store attempts to consume it
  Then refusal preserves each locked journal through its own original handle

Scenario: Possible write failures retain their original custody
  Test: native_media_prepare_failure_custody
  Given later incomplete storage or a real refused OS write
  When staging the original prepared material
  Then returned versus retained material remains explicit without unlocking the journal

## Out of Scope

Production storage behavior new sync evidence Matrix transport Palpo worker
timeouts upload activation and complete migration parity.
