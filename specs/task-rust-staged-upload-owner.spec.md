spec: task
name: "Own one staged encrypted upload through private historical settlement"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, matrix, media, custody]
---

## Intent

Join actual retained encrypted staging, one current upload send grant, bounded
HTTPS transport and private SDK acceptance into a host-driven single attempt.

## Constraints

### Must
- Consume actual RestoredEncrypted and UploadSend and match the original full claim identity and staging commitment before admission.
- Bound caller-mutable capability identifiers and secret before any retained admission or clone.
- Retain finite attempt custody across dropped futures SDK closure lost acknowledgements and exact historical resubmission.
- Require authenticated prior collector identity and current dispatch authority after the last SDK possible-write acknowledgement before polling HTTP.
- Release the SDK owner and collector busy permit during network waits so negative observations can advance.
- Persist only actual sealed HTTP response evidence in the private SDK journal before historical domain acceptance.
- Preserve cancellation uncertainty and original response bytes without rearming a possible upload.
- Keep storage namespace and accepted media URI separate from runtime room or task authority.

### Must Not
- Do not create plaintext upload arbitrary restored transport constructors new send grants file events or canonical Done from upload acceptance.
- Do not spawn unbounded jobs or release an active attempt slot because its caller dropped a future.
- Do not infer successful durable staging on a platform with unconfirmed directory synchronization.
- Do not expose descriptors response bodies URIs paths or capability secrets in safe results or Debug.
- Do not change domain schema media staging production services or live credentials.

## Boundaries

### Allowed Changes
- native/hagency-matrix/src/upload.rs
- native/hagency-matrix/src/upload/state.rs
- native/hagency-matrix/src/upload/operation.rs
- native/hagency-matrix/src/collector.rs
- native/hagency-matrix/src/lib.rs
- native/hagency-matrix/src/http.rs
- native/hagency-matrix/src/upload_custody.rs
- native/hagency-matrix/src/sdk/upload_custody.rs
- native/hagency-matrix/tests/staged_upload/mod.rs
- native/hagency-matrix/tests/staged_upload/fixture.rs
- native/hagency-matrix/Cargo.toml
- ./Cargo.lock
- knowledge/decisions/adr-089-native-staged-upload-owner.md
- specs/task-rust-staged-upload-owner.spec.md
- docs/progress.md
- docs/agent-knowledge.md

## Acceptance Criteria

Scenario: Actual staged ciphertext completes one upload and historical settlement
  Level: integration
  Test Double: real local file codec private staging SDK SQLite and local TLS server
  Test: native_staged_upload_complete
  Given actual encrypted material and an exact current started dispatch send grant
  When the host drives the retained upload once
  Then qualified staging uploads only original ciphertext once and the private SDK receipt settles the exact domain record
  And unqualified directory sync instead returns exact encrypted custody and refuses POST with a distinct qualification diagnostic
  And neither outcome claims file event delivery or task completion
  And a fresh process can replay accepted protected history without original runner secrets

Scenario: Association privacy and stage mismatch refuse before network
  Level: integration
  Test Double: actual distinct grants restored files and authenticated collector observations
  Test: native_staged_upload_scope
  Given unrelated claims wrong staging identity or retired private scope
  When the host admits or starts an upload
  Then exact original custody is preserved and no POST begins
  And unqualified directory sync instead proves exact returned custody and no POST without claiming the positive workflow

Scenario: Final current authority is checked after the private possible-write journal
  Level: integration
  Test Double: deterministic pause after actual SDK journal acknowledgement and real domain invalidation
  Test: native_staged_upload_fence
  Given a retained original upload paused after its SDK possible-write record
  When its transport or private route retires before final writer validation
  Then upload remains nonrearmable and no POST occurs
  And unqualified directory sync instead proves exact returned custody and no POST without claiming the positive workflow

Scenario: Cancellation and dropped futures retain finite unknown custody
  Level: integration
  Test Double: actual paused local TLS request and bounded host operation owner
  Test: native_staged_upload_cancellation
  Given an admitted upload whose HTTP request may have reached the server
  When cancellation or future abandonment stops the operation
  Then the original owner retains its ciphertext and finite slot and cannot repeat POST
  And unqualified directory sync instead proves exact returned custody and no POST without claiming the positive workflow

Scenario: Private persistence failure can settle the retained exact response without POST
  Level: integration
  Test Double: real SQLite failure trigger SDK owner reopen and original sealed HTTP response
  Test: native_staged_upload_recovery
  Given a complete checked response whose SDK persistence failed
  When the host reopens only the private SDK owner and retries exact historical settlement
  Then original acceptance is recorded even after revocation without another network request
  And a fresh process performs first domain acceptance after an actual private SDK commit and failed domain UPDATE
  And unqualified directory sync instead proves exact returned custody and no POST without claiming the positive workflow

Scenario: Retained job and response limits survive owner replacement
  Level: integration
  Test Double: actual owned staged inputs held operations and SDK reopen
  Test: native_staged_upload_capacity
  Given the finite two-slot result pool is held across clones or owner reopen
  When another original upload is admitted
  Then admission refuses with its original input intact and no hidden background queue
  And unqualified directory sync instead proves exact returned custody and no POST without claiming the positive workflow

## Out of Scope

Physical workspace capture binding staging worker orchestration file event metadata
MCP tools room sending automatic startup selector discovery live services and
production readiness. Windows directory durability may explicitly refuse positive
staging and therefore the end-to-end network branch remains platform qualified.
