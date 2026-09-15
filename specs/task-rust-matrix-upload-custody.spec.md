spec: task
name: "Protect exact historical Matrix upload acceptance in SDK custody"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-MATRIX-DM-PRIVACY]
tags: [active, rust, matrix, custody]
---

## Intent

Persist original bounded upload response bodies under the existing SDK owner and
encryption without reconstructing upload authority or wiring a network workflow.

## Constraints

### Must
- Consume the original domain UploadSend and bind its exact id fence full route and StageCommitment to actual persisted SDK identity and configured origin account device binding.
- Reserve one of64 permanent response slots before returning a unique ephemeral-owner LivePermit and persist private WritePossible before consuming that permit.
- Admit only a sealed actual UploadResponse and acquire finite queue and shared memory custody before copying its original bounded body.
- Persist raw response bytes checked MXC digest and deterministic receipt in one separately encrypted bounded SDK custom value before acknowledging acceptance.
- Preserve exact historical acceptance after cancellation or revocation without restoring current permission or granting an HTTP send.
- Refuse missing enabled keys torn bootstrap identity mismatch corruption and invalid phase relationships without regenerating history.
- Keep failed acceptance bytes in the poisoned owner until explicit close and preserve committed evidence through validated reopen.
- Return only bounded phase and receipt commitments from inspection and never reconstruct a live permit on replay or restart.
- Restore an opaque historical reference only by a bounded exact id after protected record validation with borrowed original fence stage and route metadata for future domain settlement.
- Execute deliberate process-wide memory exhaustion in an exact-selector child process so concurrent upload and file-publication scenarios retain independent capacity; child failure or timeout must fail the parent scenario.

### Must Not
- Do not introduce a process-global response map automatic unknown retry HTTP service coordinator runtime tool media adapter or production activation.
- Do not claim uncommitted bytes survive explicit owner close process death hardware power loss or unqualified Windows directory persistence.
- Do not claim the primitive binds the sealed response to its actual POST operation; the future coordinator must retain the original attempt and enforce that association and final current authority.

## Boundaries

### Allowed Changes
- native/hagency-matrix/Cargo.toml
- native/hagency-matrix/src/sdk.rs
- native/hagency-matrix/src/upload_custody.rs
- native/hagency-matrix/src/sdk/upload_custody.rs
- native/hagency-matrix/tests/upload_custody/mod.rs
- knowledge/decisions/adr-084-native-matrix-upload-custody.md
- specs/task-rust-matrix-upload-custody.spec.md
- docs/agent-knowledge.md
- docs/progress.md

## Acceptance Criteria

Scenario: Actual sealed response survives protected SDK reopen
  Test: native_matrix_upload_custody_persistence
  Level: integration
  Test Double: local TLS response and actual SDK SQLite encryption
  Given an actual domain UploadSend and sealed local TLS response with original whitespace and escaping
  When the owner commits private Possible and acceptance then reopens
  Then exact original bytes MXC body hash and receipt remain protected and inspectable as commitments
  And an exact id restores only the original historical reference after the original reference is dropped

Scenario: Replay and owner replacement cannot rearm a possible upload
  Test: native_matrix_upload_custody_nonrearmable
  Level: integration
  Test Double: actual domain registry and SDK owner
  Given a consumed send a lost SDK acknowledgement and a held original owner permit
  When the owner closes or acceptance commits without its acknowledgement
  Then inspection returns historical facts only and old owner permits cannot execute in the reopened owner

Scenario: Identity and historical acceptance remain exact
  Test: native_matrix_upload_custody_scope
  Level: integration
  Test Double: actual domain registry local TLS and private negative identity fixtures
  Given changed fence stage route SDK origin or body and a cancelled original domain upload
  When private acceptance and historical domain settlement are attempted
  Then changed identity is refused and exact original acceptance settles without restoring domain authority

Scenario: Real persistence failure retains finite owner custody
  Test: native_matrix_upload_custody_storage_failure
  Level: integration
  Test Double: actual SQLite abort trigger and sealed local TLS response
  Given a private Possible record and a real database write abort
  When acceptance persistence fails
  Then original bytes remain in the poisoned owner and durable reopen remains unknown without rearming

Scenario: Bootstrap and encrypted history fail closed
  Test: native_matrix_upload_custody_corruption
  Level: integration
  Test Double: actual SDK private custom value corruption and missing bootstrap artifacts
  Given missing enabled history torn initialization changed key or malformed protected records
  When existing SDK custody is opened
  Then ambiguous history is refused without empty replacement initialization

Scenario: Reserved response and copy capacity remain finite
  Test: native_matrix_upload_custody_capacity
  Level: integration
  Test Double: actual SDK commits and held shared memory permits in an isolated exact-selector process
  Given64 retained records independently growing intake and exhausted shared body custody
  When new admission or response copying is attempted
  Then new work is refused before body copying and existing exact receipt inspection remains available across owner reopen

## Out of Scope

Current domain revalidation immediately before POST, authenticated whoami, actual
response-to-operation association, durable staged-media transport, raw historical
export, process-global response recovery, file-event publication and full upload
workflow are future coordinator obligations. SQLite acknowledgement does not
qualify hardware crash durability or Windows directory sync.
