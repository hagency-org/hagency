spec: task
name: "Publish native encrypted files with original upload custody"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREAD-SCOPED-SESSIONS, REQ-MATRIX-DM-PRIVACY, REQ-THREE-LAYER-COMPLETION]
tags: [active, rust, matrix, files]
---

## Intent

Implement ADR098 using the original accepted upload operation and existing private
SDK outgoing engine. Keep domain upload acceptance distinct from room delivery.

## Constraints

### Must
- Consume original retained upload claim stage media permit and unique publication send with exact association checks before any await.
- Return original inputs on admission refusal and retain unresolved jobs independently of caller futures.
- Construct secret file content only inside the private SDK from its actual accepted upload record.
- Preserve exact immutable metadata captured length and thread or private route.
- Validate current identity privacy and original publication authority after the last SDK await before each write.
- Recover first historical Delivered only from actual protected complete event acceptance without replay.
- Release a settled job without an active SDK attempt only through exact original already-Delivered acceptance replay with its actual private SDK receipt.
- Update the original retained operation outcome on recovery and keep unmatched retained jobs unknown and blocking close.
- Use ADR100 original stage and full domain content associations so coherent substitutions cannot settle.
- Keep existing finite memory journal event receipt and network bounds.

### Must Not
- Do not export MXC descriptor keys capability or private receipt commitments.
- Do not repeat encryption or writes after uncertainty or conflate upload success with delivery or task completion.
- Do not advertise service MCP production activation or positive Windows durability qualification.

## Boundaries

### Allowed Changes
- native/hagency-matrix/src/lib.rs
- native/hagency-matrix/src/upload.rs
- native/hagency-matrix/src/upload/state.rs
- native/hagency-matrix/src/upload/publication.rs
- native/hagency-matrix/src/outgoing.rs
- native/hagency-matrix/src/outgoing/state.rs
- native/hagency-matrix/src/sdk.rs
- native/hagency-matrix/src/sdk/outgoing.rs
- native/hagency-matrix/src/sdk/file_publication.rs
- native/hagency-matrix/tests/file_publication/mod.rs
- native/hagency-matrix/tests/file_publication/fixture.rs
- native/hagency-matrix/tests/file_publication/recovery.rs
- native/hagency-matrix/tests/outgoing/crypto_fixture.rs
- native/hagency-matrix/tests/staged_upload/mod.rs
- native/hagency-matrix/tests/staged_upload/fixture.rs
- knowledge/decisions/adr-098-native-encrypted-file-publication.md
- specs/task-rust-file-publication.spec.md
- docs/agent-knowledge.md
- docs/progress.md

### Forbidden
- Domain schema source capture service bootstrap MCP and Node production code are outside this contract.

## Acceptance Criteria

Scenario: Original accepted file reaches a real recipient
  Test: native_file_publication_recipient
  Given real captured encrypted staged bytes and their actual accepted upload
  When the original owner publishes into an encrypted thread or direct room
  Then the recipient SDK decrypts exact bytes filename caption and original relation and only event acceptance reports Delivered

Scenario: Different inputs cannot replace original custody
  Test: native_file_publication_association
  Given original upload and separate publication inputs
  When another operation claim stage or captured length is substituted
  Then admission returns exact original inputs and starts no network write

Scenario: Caller loss preserves the original finite owner
  Test: native_file_publication_custody
  Given two admitted original media owners
  When a caller drops during publication or SDK ownership is reopened
  Then original media permits remain held and no third job or repeated run is admitted

Scenario: Current authority is checked at the actual write boundary
  Test: native_file_publication_current_scope
  Given a prepared SDK file publication
  When task authority expires or is retired after SDK persistence
  Then no new network write starts and the original possible state cannot rearm

Scenario: Identity and private membership changes fence publication
  Test: native_file_publication_privacy
  Given a frozen encrypted private route and authenticated transport
  When current token identity or private room membership changes
  Then publication refuses and negative observations persist without exposing file content

Scenario: Uncertain writes cannot replay
  Test: native_file_publication_uncertain
  Given an actual begun publication
  When a write or acceptance response is lost and custody is inspected or reopened
  Then no new encryption or PUT occurs and Delivered is not invented

Scenario: Protected acceptance settles for the first time after restart
  Test: native_file_publication_historical
  Given actual SDK Complete event acceptance before a failed first domain settlement
  When a fresh process opens only protected SDK and domain state
  Then it records the first Delivered without original capability media or any network replay

Scenario: Protected content and upload correlation remain inseparable
  Test: native_file_publication_journal
  Given protected file attempts and original upload acceptance
  When file content metadata receipt or locator is inconsistent or retention is full
  Then the SDK refuses without another write or public secret projection

## Out of Scope

The FileService and MCP workflow, actual runtime sandbox qualification, live Matrix
services and production cutover remain separate mandatory migration gates.
