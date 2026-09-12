spec: task
name: "Bound Matrix attachment encryption and checked decryption"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, files, matrix, crypto]
---

## Intent

Use the pinned Matrix SDK attachment primitive on retained workspace snapshots
and bounded downloaded bytes, without treating cryptography as room authority
or message delivery.

## Decisions

This contract retains the design boundaries in [ADR-027](../knowledge/decisions/adr-027-session-file-delivery.md).

## Constraints

### Must
- Encrypt actual immutable snapshots through matrix-sdk-crypto 0.18.0.
- Keep original snapshot and one shared codec permit until encrypted bytes are dropped.
- Validate bounded unique v2 encryption metadata before SDK decoding.
- Consume and check the complete ciphertext before exposing any plaintext.
- Preserve explicit ciphertext integrity versus authenticated descriptor provenance.
- Limit each operation to 16 MiB and each codec owner to eight retained results.
- Keep all plaintext keys descriptors and paths out of Debug error strings and automatic serialization.
- Test interoperability against actual existing Node Matrix attachment crypto and fixed independent ciphertext vectors.

### Must Not
- Do not implement a new cipher or room crypto engine.
- Do not infer correct plaintext or sender identity from the ciphertext hash alone.
- Do not add plaintext fallback networking media uploads download paths schema changes service enablement or file tool authority.
- Do not claim atomic source versions durable staging secure key erasure or cancellable disk reads.

## Boundaries

### Allowed Changes
- ./Cargo.toml
- ./Cargo.lock
- native/hagency-media/**
- native/scripts/media-vectors.mjs
- native/fixtures/media.json
- .github/workflows/ci.yml
- knowledge/decisions/adr-061-native-media-codec.md
- specs/task-rust-media-codec.spec.md
- docs/**

## Acceptance Criteria

Scenario: Existing attachment crypto interoperates with native checked bytes
  Test: native_media_interoperability
  Level: integration
  Test Double: fixed Node AES ciphertext validated by actual Matrix crypto plus native SDK encryption and decryption
  Given bounded empty binary Unicode and chunk-boundary file bytes
  When the native codec encrypts a real workspace snapshot or decrypts the shared vectors
  Then actual SDK ciphertext and plaintext agree without a live homeserver

Scenario: Invalid bytes never expose partial plaintext
  Test: native_media_integrity
  Given truncated extended corrupted or oversized ciphertext
  When checked decryption consumes the bytes
  Then no checked plaintext result is returned
  And changed keys with unchanged ciphertext hashes are explicitly not authenticated by this primitive

Scenario: Unsupported descriptors fail before decryption
  Test: native_media_descriptors
  Given duplicate malformed oversized or unsupported key version algorithm hash or IV metadata
  When the bounded descriptor parser runs
  Then it refuses the metadata with static errors and no secret details

Scenario: Retained results hold bounded file and codec custody
  Test: native_media_custody
  Given a finite codec and a workspace with finite snapshots
  When encryption or decryption holds results and original paths change
  Then immutable bytes remain stable capacity stays occupied and dropping results releases their permits

## Out of Scope

Descriptor sender authentication, durable staging, encrypted file outboxes,
upload/download transport, thumbnails, filename policy, scoped MCP tools and
actual Matrix event delivery remain separate integrations under ADR027.
