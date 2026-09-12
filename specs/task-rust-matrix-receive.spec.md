spec: task
name: "Receive authenticated encrypted attachments within frozen dispatch authority"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, matrix, attachments, privacy]
---

## Intent

Compose retained verified manifests and bounded HTTPS decryption with current
dispatch authorization into a finite host-local checked-byte result.

## Decisions

This contract retains the design boundaries in [ADR-027](../knowledge/decisions/adr-027-session-file-delivery.md).

## Constraints

### Must
- Derive the source ticket internally from the exact current capability and original event.
- Revalidate that same ticket after asynchronous download and before exposing bytes.
- Retain one configured-origin downloader and finite early result reservations across SDK reopen.
- Bound the entire operation by one absolute deadline and cancellation while preserving honest crypto limits.
- Release SDK owner and collector busy custody before network waits so negative observations can advance.
- Keep descriptors keys URLs and routes out of runtime-facing output and errors.

### Must Not
- Do not accept caller URL room descriptor path or verification assertions as receive authority.
- Do not download during intake or expand frozen source and projection visibility.
- Do not return partial plaintext or fabricate a cache path Snapshot or durable receipt.
- Do not enable a live service MCP file tool plaintext fallback or upload.

## Boundaries

### Allowed Changes
- native/hagency-matrix/src/receive.rs
- native/hagency-matrix/src/collector.rs
- native/hagency-matrix/src/media_download.rs
- native/hagency-matrix/src/lib.rs
- native/hagency-matrix/tests/intake/receive.rs
- native/hagency-matrix/tests/intake/mod.rs
- knowledge/decisions/adr-076-native-matrix-receive.md
- specs/task-rust-matrix-receive.spec.md
- docs/progress.md
- docs/agent-knowledge.md

## Acceptance Criteria

Scenario: Authenticated encrypted files become scoped checked bytes
  Test: native_matrix_receive_verified
  Given actual verified encrypted file and image events and selected inbox dispatches
  When the host receives the exact current visible attachment through configured HTTPS
  Then complete verified bytes and safe metadata are returned without eager intake download

Scenario: Wrong or later source authority cannot start media transport
  Test: native_matrix_receive_visibility
  Given an exact frozen dispatch and unrelated later or stale attachment authority
  When receive admission checks the source capability and manifest
  Then refusal happens before any media GET

Scenario: Async privacy and lease retirement fence checked bytes
  Test: native_matrix_receive_retirement
  Given an admitted receive paused at an actual TLS request
  When revocation promotion negative privacy or lease expiry occurs
  Then final validation refuses output and permits negative observations to advance

Scenario: Transport failure and dropped futures expose no partial bytes
  Test: native_matrix_receive_failure
  Given hash mismatch truncated ciphertext cancellation or a dropped receive
  When bounded transport and final checks complete or stop
  Then no successful object is exposed and admission capacity is recovered

Scenario: Result capacity remains finite across shared owner reopen
  Test: native_matrix_receive_capacity
  Given retained checked results and a reopened SDK owner
  When another receive would exceed held result capacity
  Then it refuses before media GET and dropping a result restores exactly one slot

Scenario: One deadline covers local queues and final checks
  Test: native_matrix_receive_deadline
  Given blocked local ownership or delayed receive work
  When cancellation or the single total deadline expires
  Then no checked result escapes and no replacement deadline is inferred

## Out of Scope

Fresh remote membership proof on every call disk cache paths staged persistence
MCP output file tools arbitrary group policy taskless sessions uploads and native
production activation remain absent. Prior actual authenticated observations and
current domain authority are required; returned bytes do not remain export authority.
