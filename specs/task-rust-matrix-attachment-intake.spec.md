spec: task
name: "Retain authenticated encrypted attachment manifests before native admission"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, matrix, media, privacy]
---

## Intent

Admit verified encrypted file metadata while preserving original private descriptor
custody independently of completed intake batches and avoiding eager downloads.

## Decisions

This contract retains the design boundaries in [ADR-027](../knowledge/decisions/adr-027-session-file-delivery.md).

## Constraints

### Must
- Derive attachments only after actual owned SDK verified sender device and session checks.
- Bind exact original source digest decrypted content SDK identity and frozen route.
- Persist finite independent private manifests before any domain attachment admission.
- Use new domain attachment admission and historical receipt APIs atomically with safe metadata.
- Preserve terminal source refusals and exact replay across lost acknowledgements and restart.
- Expose descriptors only through finite opaque host handles and exact scoped lookup.
- Bound manifest count per record bytes total journal size and held results.

### Must Not
- Do not download media during intake or history processing.
- Do not expose MXCs keys descriptors or raw events in model metadata or public status.
- Do not accept plaintext attachment fallback arbitrary URL input or forged verified flags.
- Do not edit domain schemas core types service settings or existing task truth.
- Do not turn a restored old manifest into current dispatch or room authority.

## Boundaries

### Allowed Changes
- native/hagency-matrix/src/collector.rs
- native/hagency-matrix/src/attachments.rs
- native/hagency-matrix/src/event_batch.rs
- native/hagency-matrix/src/intake.rs
- native/hagency-matrix/src/lib.rs
- native/hagency-matrix/src/sdk.rs
- native/hagency-matrix/src/sdk/attachments.rs
- native/hagency-matrix/tests/intake/mod.rs
- native/hagency-matrix/tests/intake/crypto_fixture.rs
- native/hagency-matrix/tests/intake/attachments.rs
- knowledge/decisions/adr-074-native-matrix-attachment-intake.md
- specs/task-rust-matrix-attachment-intake.spec.md
- docs/progress.md
- docs/agent-knowledge.md

### Forbidden
- Core store schemas workspace manifests lockfiles live services and another worktree.

## Acceptance Criteria

Scenario: Verified file metadata is admitted without fetching media
  Test: native_matrix_attachment_verified_metadata
  Given actual SDK encrypted human file and image events
  When the collector admits their frozen current scope
  Then only safe metadata reaches domain inputs and DM or mention wake rules remain intact
  And no download request occurs and private descriptors remain retained

Scenario: Complete manifests survive lost results and restart
  Test: native_matrix_attachment_restart_replay
  Given manifests persisted before domain handoff
  When a domain result is lost and the SDK owner restarts
  Then exact historical acknowledgement completes without duplicate input
  And the original descriptor survives intake completion and subsequent restart

Scenario: Attachment admission preserves sender privacy and terminal refusals
  Test: native_matrix_attachment_privacy_refusals
  Given unverified plaintext malformed foreign or previously refused sources
  When attachment events enter actual SDK intake
  Then no attachment authority or plaintext fallback is created
  And a prior terminal refusal remains terminal after later key availability

Scenario: Manifest storage and inspection remain bounded
  Test: native_matrix_attachment_manifest_bounds
  Given finite retained manifests and held lookup results
  When count byte or result capacity is exhausted
  Then admission refuses without evicting original records or bypassing existing source receipts

Scenario: Scoped lookup cannot substitute another manifest or generation
  Test: native_matrix_attachment_lookup_scope
  Given original manifest and exact private host scope
  When source identity content route or current generation differs
  Then secret lookup is refused and no stale handle grants dispatch authority

Scenario: Actual manifest commit failure never creates domain attachment authority
  Test: native_matrix_attachment_storage_failure
  Given an actual SQLite journal commit abort after SDK decryption
  When manifest persistence fails before domain admission
  Then no domain input exists and restart retains explicit Applying uncertainty
  And rolled back manifests cannot be returned as durable records

## Out of Scope

Downloading decrypting or writing receive caches MCP receive_file room sends
restored uploads live keys and operational service integration remain separate.
