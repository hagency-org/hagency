spec: task
name: "Validate original file publication content and staging associations"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREE-LAYER-COMPLETION]
tags: [active, rust, files, custody]
---

## Intent

Implement ADR100's bounded independent association checks for the ADR098 publisher.
Compare immutable historical domain content and the original encrypted frame
commitment without reconstructing source custody or current send authority.

## Constraints

### Must
- Reuse exact historical locator restoration and compare every original request and capture field before returning settlement custody.
- Validate bounded owned inputs before queue admission and retain the existing finite writer limits.
- Keep unknown historical locators absent and reject changed bounded content with Conflict.
- Share the existing frame identity algorithm and preserve its exact persisted bytes.
- Compare namespace operation ciphertext length and actual descriptor bytes including its ciphertext hash against the original receipt digest.
- Preserve original preparation staging restoration sync and nonreissue semantics.

### Must Not
- Do not create current send grants source custody verified SDK proof or receipt constructors.
- Do not claim ciphertext IO authentication durability or Windows qualification from matching digest data.
- Do not add SDK service MCP endpoints schema changes automatic retry or live external calls.

## Boundaries

### Allowed Changes
- native/hagency-store/src/domain/file_delivery.rs
- native/hagency-store/src/domain_worker.rs
- native/hagency-store/tests/file_delivery.rs
- native/hagency-store/tests/file_delivery/worker.rs
- native/hagency-media/src/descriptor.rs
- native/hagency-media-store/src/frame.rs
- native/hagency-media-store/src/lib.rs
- native/hagency-media-store/src/tests.rs
- native/hagency-media-store/src/tests/association.rs
- knowledge/decisions/adr-100-native-file-publication-association.md
- specs/task-rust-file-publication-association.spec.md
- docs/agent-knowledge.md
- docs/progress.md

### Forbidden
- Matrix publisher bootstrap file-service MCP Node production and persisted schema remain outside this contract.

## Acceptance Criteria

Scenario: Historical settlement checks original immutable content
  Test: native_file_publication_content_association
  Given an actual possible publication with original metadata and captured facts
  When bounded content is coherently replaced or exact historical content is restored after retirement and reopen
  Then substitutions conflict while exact settlement remains historical and never issues another send

Scenario: The real writer bounds historical content lookup
  Test: native_file_publication_content_writer
  Given the actual finite domain writer and an original possible publication
  When invalid inputs arrive behind a blocked writer or a historical lookup result is discarded
  Then invalid inputs refuse before queueing and exact later inspection preserves uncertainty without recreating authority

Scenario: The shared frame identity preserves actual staging receipts
  Test: native_media_encrypted_receipt_association
  Given actual SDK encrypted source custody and an original staging operation
  When preparation ordinary staging and reopen compute or inspect its identity
  Then the exact association matches unchanged frame bytes while unqualified preparation remains refused

Scenario: Coherent encrypted content substitutions cannot match the original receipt
  Test: native_media_encrypted_receipt_substitution
  Given a receipt committed from actual encrypted material
  When valid descriptor key IV hash encoding namespace operation or ciphertext length is changed
  Then the association refuses every changed commitment and creates no receipt or storage authority

## Out of Scope

Actual SDK journal acceptance and independent recipient decryption belong to ADR098.
Physical source truth actual service and MCP delivery remain separate integration
gates. Metadata and hash association alone cannot qualify any of those boundaries.
