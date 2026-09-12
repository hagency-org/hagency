spec: task
name: "Sync the retained Linux media directory through a readable handle"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, media, linux]
---

## Intent

Correct Linux media staging when cap-std directory capabilities use O_PATH handles
that cannot acknowledge fsync.

## Constraints

### Must
- Keep the original retained directory capability and its existing private checks.
- On Linux open only fixed relative dot with read access under the retained capability before creating the journal.
- Verify the readable descriptor refers to the same retained device and inode and passes private checks.
- Require actual directory sync success on Linux and preserve failure quarantine.
- Keep existing macOS and Windows sync evidence behavior unchanged.

### Must Not
- Do not reopen an ambient path or weaken permissions identity or directory flush requirements.
- Do not treat the original failing CI result or local macOS tests as Linux qualification.

## Boundaries

### Allowed Changes
- native/hagency-media-store/src/lib.rs
- native/hagency-media-store/src/tests.rs
- knowledge/decisions/adr-066-native-media-staging.md
- specs/task-rust-media-directory-sync.spec.md
- docs/progress.md
- docs/agent-knowledge.md

## Acceptance Criteria

Scenario: Retained capability supplies an actual syncable directory handle
  Test: native_media_stage_directory_sync_handle
  Given a private real directory opened through cap-std and renamed after opening
  When the store creates its journal using that retained directory
  Then Linux explicitly observes the original O_PATH fsync refusal and the corrected same-object descriptor sync succeeds
  And no journal appears under a replacement at the old pathname

Scenario: Staging still preserves interruption and private-object guards
  Test: native_media_stage_interruptions
  Test: native_media_stage_platform
  Given real private staged media and interrupted writes or replacement attempts
  When the caller retries or reopens
  Then existing quarantine private access and storage evidence checks remain enforced

## Out of Scope

Hardware power-loss guarantees namespace provisioning runtime services media network
transfer and changing Windows unconfirmed-directory evidence.
