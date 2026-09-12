spec: task
name: "Optimize pinned SHA256 under the native development profile"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, startup, performance]
---

## Intent

Reduce measured unoptimized SHA256 cost without changing executable verification
or the original native startup watchdogs.

## Decisions

The original 22c4993 Linux run has four failed native children inside the actual
configured executable hash loop, before HTTP and media startup. Retain those
failures and [their bounded evidence](../knowledge/context/native-sha256-dev-profile.md).
Optimize only the pinned SHA256 package in the development profile inherited by
tests. The caller and other packages retain their existing profiles.

## Constraints

### Must
- Preserve complete original executable reads, byte and metadata bounds, SHA256 comparison, and refusal behavior.
- Keep original test parallelism, watchdogs, production deadlines, and release profile unchanged.
- Preserve actual executable configuration refusals and native/SDK crypto interoperability.
- Record exact measured local file bytes, digest, profile change, and elapsed time separately from hosted results.

### Must Not
- Do not cache or bypass a digest, truncate the hashed input, strip or substitute a test executable, or weaken assertions.
- Do not infer the missing original Linux executable byte count from a different platform's artifact.
- Do not treat a measurement or diagnostic pass as the original CI verdict.

## Boundaries

### Allowed Changes
- ./Cargo.toml
- specs/task-rust-sha256-dev-profile.spec.md
- knowledge/context/native-sha256-dev-profile.md
- docs/progress.md
- docs/agent-knowledge.md

## Acceptance Criteria

Scenario: Original executable integrity and configuration refusals remain enforced
  Test: native_bootstrap_config
  Level: integration
  Test Double: actual disposable service child with invalid original executable configurations
  Given malformed profiles and wrong, empty, oversized, or special executable inputs
  When the actual native startup validates those inputs
  Then each original refusal remains a failure to start with zero attempts and no model requests

Scenario: Valid native startup still executes through the actual protocol
  Test: native_bootstrap_executable
  Level: integration
  Test Double: actual native executable and disposable local Matrix peer
  Given a valid original configured executable and authenticated Matrix refresh
  When native startup completes and the retained driver runs
  Then the original model protocol completes exactly one attempt

Scenario: The original child observation reaches its serving boundary
  Test: native_file_service_original_observation
  Level: integration
  Test Double: actual native child and local TLS peer with the original stderr handle
  Given the original configured file protocol executable
  When the child performs its complete startup verification
  Then its unchanged initial request and retained startup observation succeed
  And a separately invalid original configuration is still refused

Scenario: Actual encrypted media remains interoperable
  Test: native_media_interoperability
  Level: integration
  Test Double: actual native codec and Matrix SDK with fixed and generated inputs
  Given the existing encrypted media vectors
  When actual encryption, SHA256 integrity and SDK decryption run
  Then all original roundtrip and descriptor checks hold

Scenario: A valid CTR equality vector keeps its integrity refusal
  Test: native_media_ctr_equal_plaintext_vector
  Level: integration
  Test Double: actual native codec and Matrix SDK with the fixed public CTR vector
  Given a valid one-byte ciphertext equal to its plaintext
  When the actual native codec and SDK decrypt it and a corrupted copy is submitted
  Then the original vector succeeds and the corrupted copy is refused

## Out of Scope

Production crypto or filesystem changes, additional telemetry, hosted workflow
dispatch, receive integration, and any migration or deployment qualification.
