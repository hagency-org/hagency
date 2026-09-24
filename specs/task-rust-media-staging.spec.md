spec: task
name: "Retain bounded private media bytes across interruption and restart"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, media, custody]
---

## Intent

Persist immutable staged media under an actual retained private directory capability
without turning storage partitions or hashes into execution or Matrix authority.

## Constraints

### Must
- Accept original Snapshot Encrypted or CheckedBytes custody and retain exact bytes and encryption descriptors.
- Bind bounded stable operation IDs to immutable storage namespace kind and content.
- Retain one directory and journal file capability with one owner and fixed relative entry name.
- Preserve incomplete tails and unknown outcomes after write or sync failure without new admission or truncation.
- Validate bounded complete frames and checksums before exposing stored bytes after restart.
- Return distinct file-and-directory versus unconfirmed-directory sync evidence without power-loss claims.
- Bound item bytes total file bytes record index results and interrupted storage before allocation or reading.
- Keep existing pathname private checks and validate owner permissions on retained descriptors without relaxing them.

### Must Not
- Do not accept external paths room routes provider verification flags or deserialized namespace authority.
- Do not manufacture original source file custody after restart or re-encrypt a replay.
- Do not add Matrix sends domain migrations service wiring cleanup eviction or HTTP tools.
- Do not log raw bytes private descriptors keys namespace values or filesystem paths.

## Boundaries

### Allowed Changes
- ./Cargo.toml
- ./Cargo.lock
- native/hagency-media-store/**
- native/hagency-store/src/private.rs
- native/hagency-store/src/private/windows.rs
- specs/task-rust-media-staging.spec.md
- knowledge/decisions/adr-066-native-media-staging.md
- docs/agent-knowledge.md
- docs/progress.md

## Acceptance Criteria

<!-- lint-ack: bdd-rule-grouping — Each fixture exercises an independent bounded storage failure boundary. -->

Scenario: Staged media preserves actual bytes and keys
  Test: native_media_stage_roundtrip
  Level: integration
  Test Double: actual local snapshots and SDK encryption with real private journal writes
  Given actual snapshots encrypted attachments and checked decrypted bytes
  When a host stages and reopens each bounded item
  Then exact bytes and descriptors survive without claiming original filesystem or sender authority

Scenario: Namespace and operation replay are content bound
  Test: native_media_stage_replay
  Given exact conflicting capacity-refused or quarantined operation IDs in storage partitions
  When the host retries restores or rejects staging
  Then original content is replayed or unadmitted custody is returned without replacing pending media

Scenario: Partial persistence remains quarantined
  Test: native_media_stage_interruptions
  Given interruption around intent payload commit and sync boundaries
  When the journal is reopened
  Then complete records are validated and incomplete tails block admission without deletion or partial-byte exposure

Scenario: Storage bounds and corruption fail closed
  Test: native_media_stage_bounds
  Given full byte record and result limits or malformed frames
  When bytes are staged restored or read
  Then capacity and integrity failures preserve admitted data and cannot cause unbounded allocation

Scenario: Actual filesystem capabilities remain private and owned
  Test: native_media_stage_platform
  Level: integration
  Test Double: actual local directories locks permissions and platform rename operations
  Given real private directories files competing owners and renamed or redirected entries
  When staging opens and retains the objects
  Then handle permissions and platform ownership behavior prevent substitution and expose directory sync evidence honestly

Scenario: Actual write failure preserves prior observations
  Test: native_media_stage_disk_failure
  Given an opened local journal whose actual next write fails
  When staging attempts to append
  Then prior valid entries remain inspectable and new admission stays quarantined while restart absence proves no upload or delivery outcome

## Out of Scope

Protected physical provisioning, malicious same-user processes or filesystems,
network filesystem qualification, hard syscall deadlines, power-loss durability,
namespace-to-domain authority, Matrix transport, media outbox and automatic cleanup.
