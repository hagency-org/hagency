spec: task
name: "Preserve valid one-byte CTR equality in interoperability checks"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, media, crypto, regression]
---

## Intent

Correct the original ADR104 Windows interoperability failure caused by requiring
every nonempty random CTR ciphertext to differ from its plaintext. Preserve the
original failed CI result and qualify actual crypto behavior under ADR061.

## Decisions

Use the existing accepted [ADR-061](../knowledge/decisions/adr-061-native-media-codec.md)
primitive and descriptor provenance boundaries. A zero keystream byte leaves the
corresponding plaintext byte unchanged; this is valid CTR, not plaintext fallback.

## Constraints

### Must
- Keep production codec sources and crypto dependency versions unchanged.
- Preserve every original vector, actual SDK round trip and integrity rejection.
- Check actual generated ciphertext hashes and independent key and IV metadata.
- Add a fixed independently generated one-byte equality vector consumed by the actual codec and SDK.
- Preserve original Windows failure and separate local verification evidence.

### Must Not
- Do not retry randomness, alter keys or inject generated positive crypto evidence.
- Do not add a production test hook or weaken ciphertext integrity or descriptor validation.
- Do not infer file delivery, Windows FileService readiness or migration completion.

## Boundaries

### Allowed Changes
- native/hagency-media/tests/media.rs
- specs/task-rust-media-ctr-equality.spec.md
- knowledge/context/native-media-ctr-equality.md
- docs/progress.md
- docs/agent-knowledge.md

## Acceptance Criteria

Scenario: Valid equality still requires actual CTR decryption and integrity
  Test: native_media_ctr_equal_plaintext_vector
  Level: integration
  Test Double: fixed public Node AES256 CTR vector consumed by actual codec and Matrix SDK
  Given one plaintext byte and a fixed public key whose first CTR keystream byte is zero
  When the actual native codec and Matrix SDK decrypt the equal ciphertext
  Then both return the exact expected byte
  And corrupt ciphertext is rejected by the original hash check

Scenario: Generated crypto retains SDK compatibility and fresh metadata
  Test: native_media_interoperability
  Level: integration
  Test Double: existing fixed public vectors and actual random Matrix SDK encryption
  Given the original empty binary Unicode and chunk-boundary snapshots
  When native encryption and checked decryption execute once each
  Then the actual SDK round trip and ciphertext hash match
  And separate encryptions have independent key and IV metadata

Scenario: Original integrity refusals remain unchanged
  Test: native_media_integrity
  Given truncated extended corrupted or oversized ciphertext
  When the original native integrity selector runs
  Then it retains every original rejection and key-provenance distinction

## Out of Scope

Production crypto changes, FileService startup, network transport, hosted CI
reruns, Windows directory ownership and complete migration qualification.
