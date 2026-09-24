spec: task
name: "Bind original upload claims and retained staging metadata"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, custody, media]
---

## Intent

Let the consuming upload owner compare the original sealed send with its exact
claim and the retained encrypted material with its committed storage namespace.

## Constraints

### Must
- Compare the complete private UploadIdentity and exact fence when associating a send with a claim.
- Return only a borrowed storage namespace digest from actual RestoredEncrypted custody.
- Preserve independent current domain validation after association and refuse expired or retired claims through the existing validation path.
- Demonstrate two actual issued uploads at the same fence cannot substitute for each other.
- Preserve nonrearmable sends and the existing qualified restoration requirement.

### Must Not
- Do not recreate send grants or source objects from lookup data or add raw constructors serialization or Clone to sealed send or restored material.
- Do not treat association or namespace equality as proof of current execution authority.
- Do not change schema SDK HTTP service settings or Windows durability classification.

## Boundaries

### Allowed Changes
- native/hagency-store/src/domain/uploads.rs
- native/hagency-store/tests/file_uploads.rs
- native/hagency-media-store/src/types.rs
- native/hagency-media-store/src/tests/restoration.rs
- knowledge/decisions/adr-090-native-upload-input-association.md
- specs/task-rust-upload-input-association.spec.md
- docs/agent-knowledge.md
- docs/progress.md

## Acceptance Criteria

Scenario: The original claim cannot be substituted with another valid upload
  Test: native_upload_send_claim_association
  Level: integration
  Test Double: actual domain upload reservation claim and begin transactions
  Given two staged uploads whose actual claims have the same fence
  When the host compares each issued send with original and unrelated claims
  Then only the complete original identity and fence match
  And association remains historical while current validation refuses retirement

Scenario: Retained encrypted custody exposes its original namespace commitment
  Test: native_media_restore_exact_ciphertext
  Level: integration
  Test Double: actual private media journal codec and original source replacement
  Given qualified encrypted staging and its exact original receipt
  When original encrypted custody is restored after journal reopen
  Then the borrowed namespace digest matches the actual original storage partition
  And original ciphertext and descriptor remain unchanged after the source changes

Scenario: Restoration cannot manufacture an executable grant
  Test: native_media_restore_identity_refusal
  Level: integration
  Test Double: actual private media journal and negative identity fixtures
  Given missing or changed stored media
  When encrypted restoration is requested
  Then the existing exact identity refusals remain intact

Scenario: Qualified restoration preserves its finite custody and durability gate
  Test: native_media_restore_capacity_and_durability
  Level: integration
  Test Double: actual held journal results and platform sync evidence
  Given exhausted result custody or unconfirmed directory sync
  When encrypted restoration is requested
  Then the original finite capacity and durability refusals remain unchanged

## Out of Scope

Actual upload coordination SDK acceptance HTTP sends file events runtime tools
physical workspace provisioning and production parity.
