spec: task
name: "Persist native file delivery metadata and separate event custody"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREAD-SCOPED-SESSIONS, REQ-MATRIX-DM-PRIVACY, REQ-THREE-LAYER-COMPLETION]
tags: [active, rust, matrix, files]
---

## Intent

Implement ADR097's domain portion of proposed ADR092. Preserve one original upload
reservation and immutable file metadata, then require distinct current authority
for publication and exact historical receipt data for Delivered.

## Constraints

### Must
- Reserve immutable metadata and original upload preparation in one transaction with exact content-bound replay.
- Compare full request metadata and capture facts independently of the caller's request digest.
- Bind the actual host-observed captured size and hash with an equal-length original staging commitment before any recorded storage outcome.
- Require current original Started capability task epoch exclusive lease and frozen encrypted route plus accepted upload for new publication.
- Sample current time after the writer queue and SQLite transaction wait.
- Persist WritePossible before returning one nonrecreatable publication send and keep uncertainty after cancellation expiry or reopen.
- Treat historical receipt commitments as trusted host correlation data and require the future publisher to retain actual private SDK acceptance before supplying them.
- Keep finite global per-dispatch and encoded-row bounds and return only scoped safe status.

### Must Not
- Do not equate upload acceptance with event delivery or change canonical task Done.
- Do not restore preparation claim secret or current send authority from a historical settlement lookup.
- Do not add verified SDK proof constructors runtime observation endpoints or deserializable authority.
- Do not expose caption source path route descriptor MXC keys private receipt commitments or capabilities in safe receipts.
- Do not claim Matrix file-service MCP physical-source or positive Windows durability qualification from domain fixtures.

## Boundaries

### Allowed Changes
- native/hagency-core/src/file_delivery.rs
- native/hagency-core/src/lib.rs
- native/hagency-store/src/domain/file_delivery.rs
- native/hagency-store/src/domain/uploads.rs
- native/hagency-store/src/domain.rs
- native/hagency-store/src/domain_worker.rs
- native/hagency-store/src/lib.rs
- native/hagency-store/src/migrations/020-file-deliveries.sql
- native/hagency-store/tests/file_delivery.rs
- native/hagency-store/tests/file_delivery/worker.rs
- native/hagency-store/tests/common/mod.rs
- native/hagency-store/tests/file_uploads.rs
- native/hagency-store/tests/owned_completion.rs
- native/hagency-store/tests/approvals.rs
- native/hagency-store/tests/replies.rs
- native/hagency-store/tests/conversations.rs
- native/hagency-store/tests/usage.rs
- native/hagency-store/tests/workflows/mod.rs
- native/hagency-store/tests/workflows/custody.rs
- native/hagency-store/tests/verified_ingress/attachments.rs
- native/hagency-store/tests/verified_ingress/notice_custody.rs
- knowledge/decisions/adr-097-native-file-delivery-domain.md
- specs/task-rust-file-delivery-domain.spec.md
- docs/agent-knowledge.md
- docs/progress.md

### Forbidden
- Matrix SDK publisher service bootstrap MCP source-root implementation and existing Node production files remain outside this contract.

## Acceptance Criteria

Scenario: Metadata and preparation commit together exactly once
  Test: native_file_delivery_metadata_atomic
  Given a real Started capability and bounded immutable request metadata
  When transaction failure lost admission reply exact replay or changed metadata occurs
  Then metadata and original upload reserve atomically and replay never returns another preparation

Scenario: Capture facts and original stage cannot be substituted
  Test: native_file_delivery_capture_binding
  Given original delivery preparation and host-observed captured facts
  When the original stage is bound or a different preparation size hash stage or already observed upload is substituted
  Then exact immutable capture and equal-length stage commit together while substitutions and late backfilling refuse

Scenario: Publication has independent nonreissued send custody
  Test: native_file_delivery_publication_nonreissue
  Given an original upload that becomes historically accepted
  When distinct publication claims race expire begin lose their reply or are cancelled
  Then only one current claim begins and no possible write can be begun again

Scenario: Original current scope remains mandatory after queueing
  Test: native_file_delivery_current_scope
  Given an accepted upload and its original frozen route and dispatch scope
  When task lease epoch registration room transport or cancellation changes
  Then new publication and current validation refuse without erasing historical upload acceptance

Scenario: Historical settlement cannot restore authority
  Test: native_file_delivery_historical_settlement
  Given an original possible publication and exact bounded host receipt correlation data
  When settlement is restored after restart or retirement and repeated or substituted
  Then the exact historical receipt settles once while conflicts refuse and no current grant is returned

Scenario: Safe status and finite retention preserve incomplete facts
  Test: native_file_delivery_status_bounds
  Given queued cancelled unknown or delivered rows at the configured hard bounds
  When status is inspected with original or foreign scope or new work exceeds capacity
  Then safe projections distinguish upload and event outcomes without secrets and original replay stays readable

Scenario: Additive migration creates no historical delivery grants
  Test: native_file_delivery_schema_migration
  Given actual schema19 upload history and older migration fixtures
  When schema20 installs verifies and reopens
  Then original upload history remains and old rows gain no delivery metadata or publication authority

Scenario: Actual writer result loss and waits preserve custody
  Test: native_file_delivery_worker_lost_and_queued
  Given the real bounded domain writer and a paused original operation
  When committed results are discarded concurrent calls race or authority expires behind a queue or SQLite lock
  Then inspection preserves original facts and no stale claim or second preparation or send is issued

## Out of Scope

Actual source capture SDK acknowledgement verification encrypted event transport
service and MCP entry points runtime qualification live services and production
cutover remain separate mandatory integration gates. Domain test observations are
explicit host data and are not claimed to be actual SDK or filesystem evidence.
