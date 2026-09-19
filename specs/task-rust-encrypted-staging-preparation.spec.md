spec: task
name: "Retain exact encrypted staging identity before first journal write"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, media, custody]
---

## Intent

Let host orchestration commit the original encrypted storage identity before IO
without recreating keys or inferring upload absence from later storage reads.

## Constraints

### Must
- Accept only real codec Encrypted custody and require current clean qualified private storage.
- Compute the original stable frame identity before mutation and retain original bytes and descriptor.
- Bind preparation to the original Store owner and share its finite held-result pool.
- Revalidate owner identity content recovery and capacity before writing the original material.
- Preserve existing returned-unadmitted versus retained-by-Store failure custody.

### Must Not
- Do not add raw byte constructors serialization source reconstruction or automatic retry.
- Do not infer durable upload permission or unsent state from any staging observation.
- Do not fabricate positive platform sync evidence or change ordinary staging semantics.

## Boundaries

### Allowed Changes
- native/hagency-media-store/src/lib.rs
- native/hagency-media-store/src/types.rs
- native/hagency-media-store/src/preparation.rs
- native/hagency-media-store/src/tests.rs
- native/hagency-media-store/src/tests/preparation.rs
- knowledge/decisions/adr-079-native-encrypted-staging-preparation.md
- specs/task-rust-encrypted-staging-preparation.spec.md
- docs/agent-knowledge.md
- docs/progress.md

## Acceptance Criteria

Scenario: Original commitment precedes IO and survives an intervening append
  Test: native_media_prepare_exact_identity
  Given actual SDK encrypted source custody and a qualified clean private Store
  When the host prepares an operation then appends another record and stages the prepared material
  Then preparation leaves journal bytes unchanged and the committed identity matches the original plan
  And platforms without qualified sync explicitly refuse preparation with original material returned

Scenario: Prepared results share finite held capacity and retain original material
  Test: native_media_prepare_capacity
  Given overlapping generic reads and prepared encrypted material
  When their shared held-result pool or journal capacity is exhausted
  Then excess admission refuses and dropping the exact holder restores only its capacity

Scenario: Preparation cannot transfer to another Store owner
  Test: native_media_prepare_owner_identity
  Given prepared encrypted material from an original Store
  When a distinct same-namespace Store or reopened owner attempts staging
  Then it refuses without modifying either journal and returns the original encrypted material

Scenario: Later recovery and write failures preserve exact custody
  Test: native_media_prepare_failure_custody
  Given a prepared encrypted operation and later incomplete storage or a real OS write refusal
  When the original owner attempts the prepared write
  Then pre-write refusal returns original material while possible writes retain Store uncertainty

## Out of Scope

Domain upload persistence HTTP sending cache paths runtime tools physical
workspace provisioning service cutover and whole migration parity.
