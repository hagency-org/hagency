spec: task
name: "Restore exact historical upload settlement without execution grants"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, custody, media]
---

## Intent

Restore a historical upload association from existing protected row commitments
when original process memory and runner capability are unavailable.

## Constraints

### Must
- Compare exact upload ID fence staging commitment and complete frozen reply route against the existing protected row.
- Restore only staged WritePossible or Accepted rows into a sealed historical settlement type.
- Recheck the exact row in the acceptance transaction and preserve sticky cancellation and exact receipt replay.
- Keep lookup data distinct from proof of SDK or HTTP acceptance and from current execution authority.
- Demonstrate actual process exit and a fresh process using only persisted locator and opaque receipt data.

### Must Not
- Do not recreate a runner capability preparation claim or send grant from historical data.
- Do not expose raw constructors Clone Debug serde or UploadIdentity conversion on UploadSettlement.
- Do not change schema transport SDK runtime endpoints service settings or existing current authority gates.
- Do not treat missing evidence or a possible upload as unsent or safely retryable.

## Boundaries

### Allowed Changes
- native/hagency-store/src/domain/uploads.rs
- native/hagency-store/src/domain_worker.rs
- native/hagency-store/src/lib.rs
- native/hagency-store/tests/file_uploads.rs
- native/hagency-store/tests/file_uploads/settlement.rs
- knowledge/decisions/adr-085-native-upload-settlement.md
- specs/task-rust-upload-settlement.spec.md
- docs/progress.md
- docs/agent-knowledge.md

## Acceptance Criteria

Scenario: Exact historical association has no execution grant
  Level: integration
  Test Double: actual private repository and dispatch upload lifecycle
  Test: native_upload_settlement_exact
  Given a staged upload whose possible POST is recorded
  When the host restores exact persisted locator fence stage and route
  Then only historical inspection and acceptance are available
  And missing pending claimed and mismatched records never restore

Scenario: Full process exit does not require original runner secrets
  Level: integration
  Test Double: two actual native child processes sharing fixture persisted state
  Test: native_upload_settlement_process
  Given a process that committed a possible upload and then exited
  When a fresh process reads only persisted locator fence stage route and opaque receipt data
  Then the bounded writer settles the original historical record without cap request or send reconstruction

Scenario: Retired execution does not block or revive historical settlement
  Level: integration
  Test Double: actual host revocation privacy and task APIs
  Test: native_upload_settlement_retirement
  Given cancelled revoked promoted completed or expired execution
  When the original exact historical acceptance is recorded
  Then cancellation remains sticky and current upload execution stays refused

Scenario: Historical receipt replay is content bound and atomic
  Level: integration
  Test Double: real SQLite failure trigger and persisted receipt replay
  Test: native_upload_settlement_atomic
  Given an existing exact settlement handle
  When acceptance storage fails or a later receipt conflicts
  Then no partial acceptance or replacement record is committed
  And the same receipt can be acknowledged after actual successful commit

Scenario: The writer preserves exact association across queued calls
  Level: integration
  Test Double: actual bounded DomainStore worker
  Test: native_upload_settlement_worker
  Given two restored handles for the same original possible upload
  When the host records identical historical evidence concurrently
  Then one commits and the other acknowledges without rearming execution

## Out of Scope

SDK receipt provenance selector inventory actual HTTP transport upload coordinator
file event sending models runtime endpoints service activation and production parity.
