spec: task
name: "Native authenticated Matrix event intake and durable handoff"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-PALPO-OUTBOUND]
tags: [active, rust, matrix, custody, ingress]
---

## Intent

Admit actual authenticated SDK timeline messages through existing native verified
ingress while retaining exact source custody across the SDK and domain owners.

## Constraints

### Must
- Capture only host-configured current Matrix session registration device transport and shared-room tickets before polling.
- Persist the complete authenticated sync response and frozen tickets before applying SDK changes.
- Derive event sender room thread mentions and encryption provenance only from owned SDK results.
- Require successful verified SDK decryption for encrypted content and refuse plaintext fallback in encrypted rooms.
- Retain interrupted SDK Applying batches as explicit inspectable OutcomeUnknown with original raw input and frozen targets.
- Freeze derived handoffs before domain admission and advance the intake cursor only after exact durable domain receipts.
- Recheck current session transport registration and shared-room scope in the domain admission transaction.
- Use historical receipt lookup only to settle an identical already-committed event without granting current admission authority.
- Retain exact handoffs after backpressure cancellation or lost domain responses without fabricating device failure.
- Preserve actual negative room and transport fencing and refuse retargeting after rotation or private-room promotion.
- Bound journal HTTP SDK timeline target receipt and queue state with explicit capacity errors instead of dropping custody.

### Must Not
- Do not accept external plaintext JSON browser assertions or caller-provided crypto verification fields as proof.
- Do not replay an interrupted SDK batch as processed merely because its next_batch token already exists.
- Do not let observation-only synchronization advance a cursor owned by active event intake.
- Do not send keys notices approvals or messages to a live homeserver or change deployed services.
- Do not erase crypto identities or add a second canonical message or task store.

## Boundaries

### Allowed Changes
- native/hagency-matrix/**
- native/hagency-core/src/ingress.rs
- native/hagency-store/src/domain/verified_ingress.rs
- native/hagency-store/src/domain_worker.rs
- native/hagency-store/tests/**
- specs/task-rust-matrix-event-intake.spec.md
- knowledge/decisions/adr-054-native-matrix-event-intake.md
- docs/agent-knowledge.md
- docs/progress.md

### Forbidden
- MCP formatting progress runtime production JavaScript another checkout live accounts credentials deployment and domain schema changes.

## Acceptance Criteria

Scenario: Real authenticated sync reaches exact native message scope
  Test: native_matrix_intake_authenticated
  Level: integration
  Test Double: local scripted HTTPS server owned SDK and real canonical SQLite
  Given current host Matrix sessions and an authenticated account
  When actual sync messages contain full MXID mentions direct messages or thread relations
  Then native ingress preserves exact sender room thread and wake rules
  And external proof fields same-localpart impostors and wrong scopes cannot authorize work

Scenario: Encrypted events require owned SDK decryption proof
  Test: native_matrix_intake_crypto
  Level: integration
  Test Double: real offline SDK crypto fixtures and scripted authenticated Matrix responses
  Given encrypted and plaintext rooms with owned persistent SDK identity
  When verified encrypted messages unknown keys unverified senders or plaintext fallback are encountered
  Then only actual verified decryption supplies encrypted event provenance
  And unsupported crypto custody remains explicit with no plaintext substitution

Scenario: SDK application interruption retains exact input custody
  Test: native_matrix_intake_sdk_custody
  Level: integration
  Test Double: owned SQLite journal with injected stage interruption
  Given a persisted authenticated response and frozen target tickets
  When SDK application or derived-journal acknowledgement is interrupted
  Then restart exposes OutcomeUnknown while retaining raw input and targets
  And the batch is not counted processed advanced or replayed into new scope

Scenario: Domain admission and handoff acknowledgement recover exactly
  Test: native_matrix_intake_handoff
  Level: integration
  Test Double: real domain commits with lost-response cancellation and queue faults
  Given frozen derived events from an authenticated SDK batch
  When domain admission commits loses its response or temporarily refuses work
  Then identical retry settles the same receipt without duplicate input
  And writer backpressure alone does not invalidate the authenticated transport

Scenario: Real negative observations fence pending event scope
  Test: native_matrix_intake_rotation
  Level: integration
  Test Double: simultaneous scripted room changes and canonical domain transactions
  Given captured direct or group target tickets and pending handoff
  When room privacy membership device registration or session generation changes
  Then current admission fails and existing committed receipts alone may settle
  And frozen events are retained without retargeting or restoring negative authority

Scenario: Intake state and responses have finite inspectable bounds
  Test: native_matrix_intake_bounds
  Level: integration
  Test Double: bounded SDK journal scripted HTTP framing and capacity fixtures
  Given finite room event target body receipt and SDK queue limits
  When data is excessive duplicate changed malformed unsupported or cancelled
  Then custody fails visibly and prior pending state remains intact
  And observation-only synchronization cannot skip pending intake work

## Out of Scope

Automatic reconstruction of an interrupted SDK Applying phase live key query
publication sharing verification media download event-history gap backfill
runtime task creation dispatch live sends deployment and migration cutover remain
separate gates. Offline fixture trust setup is not a production key lifecycle.
