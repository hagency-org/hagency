spec: task
name: "Download bounded encrypted media through the configured Matrix origin"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, matrix, files, transport]
---

## Intent

Fetch complete bounded ciphertext through authenticated homeserver HTTPS and
return checked SDK plaintext without inventing event, sender or file authority.

## Decisions

This contract retains the design boundaries in [ADR-027](../knowledge/decisions/adr-027-session-file-delivery.md).

## Constraints

### Must
- Admit each transfer under clone-shared finite capacity before any network request.
- Keep the configured HTTPS origin and bearer credential separate from the typed MXC repository path.
- Refuse redirects proxies automatic retries and token query parameters.
- Validate exact bounded MXC server and media components without path normalization ambiguity.
- Bound response headers ciphertext bytes header wait body idle and one absolute operation deadline.
- Refuse duplicate or conflicting framing and content encoding headers before reading a body.
- Consume HTTP body EOF and validate the exact descriptor hash before exposing any plaintext.
- Retain codec result permits until actual checked bytes are dropped and recover transfer capacity after failure or cancellation.
- Keep descriptors plaintext tokens paths and raw remote errors out of Debug and error projections.
- Preserve all existing JSON transport behavior.

### Must Not
- Do not treat an MXC or descriptor as authenticated sender event dispatch or room authority.
- Do not add media upload room sending plaintext fallback event admission schema changes or service enablement.
- Do not use external live services or credentials in tests.
- Do not invent Snapshot source custody from downloaded bytes or promise hard cancellation of synchronous codec CPU work.

## Boundaries

### Allowed Changes
- ./Cargo.lock
- native/hagency-matrix/Cargo.toml
- native/hagency-matrix/src/lib.rs
- native/hagency-matrix/src/http.rs
- native/hagency-matrix/src/media_download.rs
- native/hagency-matrix/tests/media_download.rs
- native/hagency-matrix/tests/common/mod.rs
- knowledge/decisions/adr-068-native-matrix-media-download.md
- specs/task-rust-matrix-media-download.spec.md
- docs/agent-knowledge.md
- docs/progress.md

## Acceptance Criteria

Scenario: HTTPS fetches a remote repository path only through the configured host
  Test: native_matrix_media_origin
  Given a real local TLS homeserver and checked encrypted fixture bytes
  When a typed remote MXC is downloaded with the host credential
  Then only the configured host receives the bearer and complete bytes decrypt correctly without event authority

Scenario: Media identifiers cannot select or normalize a transport path
  Test: native_matrix_media_identifiers
  Given bounded valid and adversarial MXC components
  When host code constructs a media identifier
  Then malformed empty escaped slash query fragment and dot segments are refused without a network request

Scenario: Framing and allocation boundaries fail closed
  Test: native_matrix_media_headers_bounds
  Given actual TLS replies with duplicate conflicting encoded or oversized framing
  When the bounded binary reader processes them
  Then no plaintext is returned and header byte count and payload limits remain enforced

Scenario: Complete ciphertext is required for checked plaintext
  Test: native_matrix_media_integrity_eof
  Given actual chunked close-delimited truncated extended or corrupted ciphertext replies
  When the download reaches HTTP body EOF
  Then only complete exact ciphertext returns checked bytes and partial content remains unavailable

Scenario: Cancellation and deadlines retain finite work
  Test: native_matrix_media_deadline_cancel
  Given actual delayed headers slow bodies and a cancelled or dropped operation
  When a header idle or absolute deadline expires
  Then the operation returns a static failure and releases its finite transfer capacity without retry

Scenario: Held plaintext and concurrent transfers consume shared capacity
  Test: native_matrix_media_capacity
  Given cloned downloaders with finite active and codec result permits
  When requests overlap or callers retain checked bytes
  Then capacity stays bounded and actual drops restore admission without manufacturing source snapshots

## Out of Scope

Authenticated collector attachment provenance, current-dispatch receive_file,
durable downloaded-file staging, media upload, Matrix event sending, proxy/CDN
redirect support, hard CPU deadlines and live service qualification.
