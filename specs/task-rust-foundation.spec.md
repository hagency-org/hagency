spec: task
name: "Native Rust foundation with durable custody"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-PALPO-OUTBOUND]
tags: [active, rust, persistence, security]
---

## Intent

Start the migration in an isolated checkout with a runnable native Salvo service
and durable, bounded command custody. Preserve the deployed JS runtime during
development and make unfinished migration boundaries visible.

## Constraints

### Must
- Require explicit fresh state and operator credentials; bind only to loopback initially.
- Keep database work on a dedicated bounded worker and reject overload explicitly.
- Commit inbox payload and receipt together before acknowledging custody.
- Bind retries to their complete content and recover committed data after restart.
- Reject corrupt or newer schemas and concurrent state owners.
- Match the pinned JavaScript canonicalization for supported signed DTO values.
- Report unsupported runtime, transport and crypto capabilities honestly.

### Must Not
- Do not read live credentials or runtime stores or restart deployed services.
- Do not equate durable receipt with approval, provisioning, or external execution.
- Do not publish incomplete APIs as wire-compatible production replacements.

## Boundaries

### Allowed Changes
- ./Cargo.toml
- ./Cargo.lock
- ./rust-toolchain.toml
- ./.gitattributes
- native/**
- .github/workflows/rust.yml
- scripts/check-spec-bindings.js
- tests/spec-bindings.test.js
- ./.gitignore
- specs/project.spec.md
- specs/task-rust-foundation.spec.md
- knowledge/**
- docs/**

### Forbidden
- Existing JS/TS implementation, generated workspace entry files, live state and website files.

## Acceptance Criteria

Scenario: Custody survives restart without admitting work
  Test: custody_survives_restart
  Given a validated delivery and exclusive fresh state
  When native custody is acknowledged and the repository is reopened
  Then its payload and receipt remain committed and it remains unprocessed

Scenario: Retries cannot change committed content
  Test: retries_are_content_bound
  Given a committed request identifier
  When concurrent retries contain equal or different payloads
  Then equal retries return the original receipt and different content is refused

Scenario: State has one compatible owner
  Test: state_ownership_and_schema_fail_closed
  Given a repository that is locked corrupt or newer than supported
  When a service attempts to open it
  Then startup fails without rewriting the state

Scenario: HTTP authority and limits fail closed
  Test: http_auth_and_limits
  Given absent incorrect or browser-origin operator authority
  When a protected HTTP request is submitted
  Then it is refused before any state mutation and large bodies are rejected

Scenario: Private files reject broader local access
  Test: private_storage_rejects_public_access
  Given a state credential whose permissions allow another local user to read it
  When the native credential reader opens it
  Then it refuses the credential without reading its contents

Scenario: Background work does not stall control requests
  Test: bounded_work_keeps_health_responsive
  Given a stalled database worker and a full bounded queue
  When health and additional custody requests arrive
  Then health responds promptly and excess work returns a busy response

Scenario: Signing bytes preserve existing wire semantics
  Test: canonical_vectors_match_javascript
  Given sanitized Unicode null integer and property-order vectors
  When the Rust canonical encoder runs
  Then its bytes and digests match the pinned JavaScript implementation

Scenario: Fresh Rust crypto state retains its device and room key
  Test: crypto_device_survives_restart
  Given a fresh encrypted SDK store and a fixture room
  When its native device encrypts a message and reopens the store
  Then the same device decrypts the message and a different device identity is refused

Scenario: Incomplete migration capabilities are explicit
  Test: unimplemented_capabilities_are_explicit
  Given the native foundation service
  When its authenticated capabilities are read
  Then Agent execution transport and production API parity remain unavailable

## Out of Scope

The spec-binding build tool may distinguish native Cargo contracts from Vitest
contracts. Both catalogs remain mandatory in their corresponding CI jobs.

Production cutover, compatibility aliases, real Agent execution and automatic
approval. Later task contracts port these behaviors without relaxing the project
invariants. An isolated foundation is not a parity release.
