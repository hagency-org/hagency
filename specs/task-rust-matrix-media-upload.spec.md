spec: task
name: "Upload bounded encrypted media with retained possible-write custody"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, matrix, media, custody]
---

## Intent

Upload actual codec ciphertext through one configured authenticated HTTPS origin
without asserting event authority delivery or safe retry after an unknown write.

## Constraints

### Must
- Accept only a borrowed actual Encrypted object and retain its exact ciphertext descriptor and source custody.
- Acquire finite shared attempt and active request permits before copying ciphertext or starting network work.
- Mark possible write before the first request poll and retain uncertainty across future cancellation drop and transport errors.
- Refuse every repeat send on a possible or accepted attempt.
- Send only binary ciphertext to the configured homeserver with no filename metadata.
- Bound request bytes response bytes headers parsing time and retained receipts.
- Require full response EOF and exact unambiguous validated MXC content URI before retaining acceptance.
- Keep failed attempt state inspectable without exposing secrets or inferring remote absence.
- Preserve existing JSON and binary download HTTP behavior.

### Must Not
- Do not accept arbitrary plaintext bytes descriptor setters staging of unconfirmed durability or caller selected destination URLs.
- Do not send keys in upload bodies headers URLs or filenames.
- Do not redirect use ambient proxies automatically retry or treat URI response as authenticated room or event authority.
- Do not add room sending domain file-tool service live model or homeserver integration.
- Do not claim restart recovery durable upload idempotence or remote absence after lost response.

## Boundaries

### Allowed Changes
- ./Cargo.lock
- native/hagency-matrix/Cargo.toml
- native/hagency-matrix/src/lib.rs
- native/hagency-matrix/src/http.rs
- native/hagency-matrix/src/media_download.rs
- native/hagency-matrix/src/media_upload.rs
- native/hagency-matrix/tests/media_upload.rs
- knowledge/decisions/adr-072-native-matrix-media-upload.md
- specs/task-rust-matrix-media-upload.spec.md
- docs/agent-knowledge.md
- docs/progress.md

### Forbidden
- Runtime execution store schemas other worktrees existing live services and existing JSON HTTP policy.

## Acceptance Criteria

Scenario: Real encrypted content uploads only through the configured origin
  Test: native_matrix_upload_ciphertext_origin
  Level: integration
  Test Double: actual local TLS peer and actual retained snapshot plus SDK codec ciphertext
  Given a borrowed Encrypted object and a configured HTTPS homeserver
  When the upload attempt receives a valid bounded content URI response
  Then only original ciphertext and the homeserver bearer are sent to the upload path
  And descriptor keys and plaintext are absent from the network request

Scenario: Returned content identity requires complete bounded unambiguous evidence
  Test: native_matrix_upload_response_bounds
  Level: integration
  Test Double: actual TLS responses with malformed MXC JSON headers and body framing
  Given oversized contradictory duplicate truncated or malformed response evidence
  When upload response parsing finishes or fails
  Then no accepted receipt is exposed and possible-write custody remains unknown

Scenario: Cancellation and future drop retain uncertainty without resending
  Test: native_matrix_upload_cancellation_custody
  Level: integration
  Test Double: actual TLS peer paused after receiving ciphertext before response
  Given an admitted attempt whose send may have begun
  When cancellation deadline or future drop interrupts its wait
  Then original ciphertext descriptor and unknown attempt state remain held
  And a second send on that attempt makes no network request

Scenario: Transfer and retained result limits survive all outcomes
  Test: native_matrix_upload_capacity
  Level: integration
  Test Double: cloned uploaders concurrent local TLS requests and retained attempt handles
  Given finite shared active and attempt capacity
  When prepared accepted and uncertain attempts overlap
  Then admission refuses before exceeding limits and dropping exact handles restores capacity
  And oversized ciphertext is refused before any copy or request

Scenario: Redirect authentication failure and transport errors never imply retry safety
  Test: native_matrix_upload_refusals
  Level: integration
  Test Double: real local TLS rejection redirect and unavailable endpoint cases
  Given a remote error redirect or unconfirmed transport result
  When an upload attempt completes without valid acceptance
  Then it retains unknown state with a static reason and follows no alternate URL
  And no new upload or remote-absence assertion is created automatically

## Out of Scope

Upload attempts and receipts are in-memory host custody only. Dropping an unknown
attempt loses its local marker; the borrowed original media remains caller owned.
Matrix POST upload supplies no idempotency key. A later durable adapter must record
its operation before sending and must not infer safe retry from missing evidence.
