spec: task
name: "Admit authenticated attachment metadata with frozen dispatch visibility"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, matrix, attachments]
---

## Intent

Keep file metadata and access authority tied to actual admitted events and the
current dispatch without leaking media secrets or expanding frozen input.

## Constraints

### Must
- Persist safe metadata and opaque SDK manifest identity atomically with authenticated encrypted file ingress.
- Preserve exact receipt replay and reject changed metadata or later annotation of an existing text-only receipt.
- Freeze the highest actually selected inbox source sequence and independent session projection sequence at dispatch creation.
- Project historical attachments only through verified task-input provenance.
- Require current Started capability and exact current Matrix route before ticket issue and revalidation.
- Keep receipts visibility and windows bounded and durable across restart.

### Must Not
- Do not accept descriptor keys MXC URLs caller-chosen room or download authority in runner input.
- Do not enable downloads file tools live services or production cutover.
- Do not fabricate file visibility for pre-migration dispatches or erase receipts for capacity.

## Boundaries

### Allowed Changes
- native/hagency-core/src/attachments.rs
- native/hagency-core/src/lib.rs
- native/hagency-store/src/lib.rs
- native/hagency-store/tests/approvals.rs
- native/hagency-store/tests/replies.rs
- native/hagency-store/tests/conversations.rs
- native/hagency-store/tests/usage.rs
- native/hagency-store/tests/owned_completion.rs
- native/hagency-store/tests/verified_ingress/notice_custody.rs
- native/hagency-store/tests/workflows/mod.rs
- native/hagency-store/tests/workflows/custody.rs
- native/hagency-store/tests/common/mod.rs
- native/hagency-store/src/domain.rs
- native/hagency-store/src/domain/attachments.rs
- native/hagency-store/src/domain/verified_ingress.rs
- native/hagency-store/src/domain/messages.rs
- native/hagency-store/src/domain/execution.rs
- native/hagency-store/src/domain/task_intents.rs
- native/hagency-store/src/domain_worker.rs
- native/hagency-store/src/migrations/018-attachment-visibility.sql
- native/hagency-store/tests/verified_ingress.rs
- native/hagency-store/tests/verified_ingress/attachments.rs
- native/hagency-store/tests/schema_fixtures.rs
- knowledge/decisions/adr-073-native-attachment-visibility.md
- specs/task-rust-attachment-visibility.spec.md
- docs/plan.md
- docs/progress.md
- docs/agent-knowledge.md
- native/README.md

## Acceptance Criteria

Scenario: Exact authenticated attachment admission is atomic and bounded
  Test: native_attachment_admission
  Given authenticated encrypted file metadata and a current route
  When the host admits replays changes or exhausts capacity
  Then immutable metadata and original event commit together or neither does
  And public input messages contain no manifest descriptor or media URL

Scenario: Dispatch visibility never expands after enqueue
  Test: native_attachment_frozen_visibility
  Given earlier source messages and independently sequenced session projections
  When later files or late projections arrive after dispatch creation
  Then only files inside both frozen boundaries can issue a ticket

Scenario: Historical follow-ups retain exact session provenance
  Test: native_attachment_followup_visibility
  Given verified task activation and later follow-up dispatches
  When historical attachment references are projected into their canonical session
  Then the current input and earlier authorized files remain available without granting unrelated room history

Scenario: Revocation promotion and restart preserve authority boundaries
  Test: native_attachment_ticket_revalidation
  Given an issued ticket and a durable repository
  When the capability expires the route changes or the repository restarts
  Then stale tickets never authorize exposure and historical exact receipts do not revive authority

Scenario: Schema upgrades never fabricate historical file authority
  Test: native_attachment_schema_migration
  Given a schema17 database with existing dispatches
  When the schema upgrades and reopens
  Then attachment tables start empty and old dispatches have no file window

## Out of Scope

SDK manifest retention downloads local cache materialization MCP tools plaintext
files thumbnails retention eviction durable upload recovery and live deployment.
