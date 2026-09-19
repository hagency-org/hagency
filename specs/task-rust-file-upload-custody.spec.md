spec: task
name: "Persist exact native encrypted upload preparation and possible-write custody"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, matrix, files]
---

## Intent

Keep original encrypted file staging and a single possible upload bound to its
actual Started dispatch without inventing file bytes or room delivery.

## Constraints

### Must
- Derive frozen encrypted Matrix route and canonical task epoch from the exact current Started capability and exclusive workspace lease.
- Issue preparation only with the first committed reservation and never recreate it on exact replay or restart.
- Bind original stage identity before storage IO and distinguish host historical storage evidence from current execution authority.
- Durably record WritePossible before returning a send grant and never rearm it after cancellation uncertainty expiry or restart.
- Preserve exact historical acceptance after authority retirement without allowing another POST.
- Keep finite global and per-dispatch records with content-bound replay and atomic rollback.

### Must Not
- Do not store descriptors keys plaintext paths or media URLs in domain projections.
- Do not expose host observation setters through runtime HTTP or infer storage proof from serialized booleans.
- Do not enable filesystem capture HTTP upload Matrix event sending or live services.

## Boundaries

### Allowed Changes
- native/hagency-core/src/uploads.rs
- native/hagency-core/src/lib.rs
- native/hagency-store/src/domain/uploads.rs
- native/hagency-store/src/domain.rs
- native/hagency-store/src/domain_worker.rs
- native/hagency-store/src/lib.rs
- native/hagency-store/src/migrations/019-file-uploads.sql
- native/hagency-store/tests/file_uploads.rs
- native/hagency-store/tests/file_uploads/worker.rs
- native/hagency-store/tests/common/mod.rs
- native/hagency-store/tests/approvals.rs
- native/hagency-store/tests/replies.rs
- native/hagency-store/tests/conversations.rs
- native/hagency-store/tests/usage.rs
- native/hagency-store/tests/owned_completion.rs
- native/hagency-store/tests/workflows/mod.rs
- native/hagency-store/tests/workflows/custody.rs
- native/hagency-store/tests/verified_ingress/notice_custody.rs
- native/hagency-store/tests/verified_ingress/attachments.rs
- knowledge/decisions/adr-078-native-file-upload-custody.md
- specs/task-rust-file-upload-custody.spec.md
- docs/progress.md
- docs/agent-knowledge.md

## Acceptance Criteria

Scenario: Reservation never duplicates original preparation
  Test: native_upload_reservation
  Given a current Started dispatch and exact request identity
  When reservation commits loses its response replays conflicts or reopens
  Then preparation is issued once and exact historical receipt remains inspectable

Scenario: Staging and upload custody remain exact
  Test: native_upload_staging_and_send
  Given an original preparation and immutable encrypted staging commitment
  When staging is observed claims race expire or begin is repeated
  Then only a single current claim can durably begin and stale tokens never authorize upload

Scenario: Current authority cannot be restored from historical facts
  Test: native_upload_scope_fencing
  Given pending or possible uploads and a frozen direct route
  When task epoch lease membership transport or engagement changes
  Then current execution refuses while original possible-write uncertainty survives

Scenario: Historical acceptance settles without retry
  Test: native_upload_historical_settlement
  Given a durable possible upload and original send identity
  When cancellation restart late acceptance or conflicting evidence arrives
  Then exact positive facts may settle once without creating another send grant

Scenario: Capacity and database errors preserve existing facts
  Test: native_upload_atomic_bounds
  Given finite upload records and injected actual database failures
  When a new reservation or state transition cannot commit
  Then state and grants do not partially advance and exact existing replay survives capacity

Scenario: Schema migration preserves empty authority
  Test: native_upload_schema_migration
  Given an actual schema18 database
  When schema19 installs verifies and reopens
  Then old dispatches gain no fabricated upload preparation or send state

Scenario: Worker queue outcomes never create a second grant
  Test: native_upload_worker
  Given the actual bounded DomainStore writer
  When replies are discarded concurrent starts race or authority expires while queued
  Then receipt inspection is exact and at most one possible-write grant exists

## Out of Scope

Actual file capture encrypted staging IO upload transport accepted MXC storage
file event delivery runtime endpoints MCP tools filesystem provisioning and cutover.
