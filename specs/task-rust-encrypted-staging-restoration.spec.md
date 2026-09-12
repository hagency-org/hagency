spec: task
name: "Restore exact qualified encrypted staging as distinct custody"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, media, custody]
---

## Intent

Preserve original encrypted bytes after validated storage recovery without
manufacturing a source Snapshot or permission to retry a possible upload.

## Constraints

### Must
- Require exact original operation and receipt digest plus committed encrypted kind in the opened namespace.
- Require clean recovery and acknowledged file plus directory sync before typed restoration.
- Preserve original ciphertext descriptor and receipt with the existing bounded read-result permit.
- Keep Windows unconfirmed directory sync explicit and ordinary historical reads available.
- Refuse corruption wrong identity pending tails and exhausted result capacity without replacing data.

### Must Not
- Do not expose raw byte constructors Debug serialization or original Snapshot reconstruction.
- Do not add upload preparation network work runtime tools or automatic retry.
- Do not claim hardware durability or that missing storage proves an unsent upload.

## Boundaries

### Allowed Changes
- native/hagency-media-store/src/lib.rs
- native/hagency-media-store/src/types.rs
- native/hagency-media-store/src/restoration.rs
- native/hagency-media-store/src/tests.rs
- native/hagency-media-store/src/tests/restoration.rs
- knowledge/decisions/adr-077-native-encrypted-staging-restoration.md
- specs/task-rust-encrypted-staging-restoration.spec.md
- docs/plan.md
- docs/progress.md
- docs/agent-knowledge.md
- native/README.md

## Acceptance Criteria

Scenario: Qualified recovery preserves exact original encrypted content
  Test: native_media_restore_exact_ciphertext
  Given an actual source snapshot encrypted by the SDK and committed to private staging
  When the original Store closes and the exact operation is restored after validated reopen
  Then qualifying platforms return the original ciphertext descriptor receipt and namespace
  And unconfirmed Windows directory sync refuses typed restoration without claiming a round trip

Scenario: Invalid namespace operation kind or receipt digest never rebinds custody
  Test: native_media_restore_identity_refusal
  Given distinct committed encrypted and snapshot records
  When an incorrect namespace operation kind or expected receipt is selected
  Then restoration refuses without changing the original committed records

Scenario: Generic and typed results share finite held capacity
  Test: native_media_restore_capacity_and_durability
  Given a finite private read-result pool and actual sync evidence
  When generic and typed results overlap or directory sync is unconfirmed
  Then the same pool refuses excess results and unqualified typed restoration remains unavailable

Scenario: Corruption and incomplete writes cannot create qualified restored content
  Test: native_media_restore_corruption_and_pending
  Given actual corrupted bytes or an interrupted later staging operation
  When typed restoration inspects the original committed record
  Then corruption and pending recovery fail closed without trimming or manufacturing replacement content

## Out of Scope

Upload attempt recovery durable domain send intent current runner authorization
media downloads cache paths file tools physical provisioning and production cutover.
