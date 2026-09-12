spec: task
name: "Native outbound registration custody survives transport rotation"
inherits: project
satisfies: [REQ-PALPO-OUTBOUND, REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, transport, custody]
---

## Intent

Extend the single bounded custody writer with host-only durable outbound intake,
exact lease observations, processing ownership and frozen publication receipts.
Machine credential rotation is distinct from Matrix registration replacement.

## Constraints

### Must
- Host registration identity must pin side fleet fingerprint and registration generation before managed intake.
- Retain full bounded JSON payload before returning custody and before an ACK attempt.
- Match current scope poll ticket lane delivery ID and lease token when recording responses.
- Keep acknowledged and uncertain Matrix and request custody across machine rotation without fabricating a remote ACK.
- Fence retired probes and publications so they cannot prove a new connection.
- Persist claim start result and inspection transitions with content-bound receipts and conservative unknown recovery.
- Advance the submitted host clock by monotonic queue elapsed time before checking processing authority at execution.
- Preserve Matrix arrival order while the independent work lane can progress.
- Retain arbitrary-ID tombstones and reject finite capacity without evicting pending work or dedup history.
- Freeze publication bytes sequence and original observation timestamps across lost responses.
- Commit schema upgrades transactionally and refuse legacy fixture adoption or registration replacement.

### Must Not
- Do not deserialize managed authority from the fixture HTTP intake.
- Do not approve a domain request create an Agent call a model contact a homeserver or deploy a service.
- Do not expose lease secrets scope capabilities or private payload through public or Debug projections.

## Boundaries

### Allowed Changes
- native/hagency-core/src/canonical.rs
- native/hagency-store/**
- native/fixtures/outbound-custody.json
- specs/task-rust-outbound-custody.spec.md
- knowledge/decisions/adr-037-native-outbound-custody.md
- docs/**

### Forbidden
- Domain schema or runtime approval policy production JavaScript Palpo source credentials and other worktrees.

## Acceptance Criteria

Scenario: Exact scoped transport custody precedes acknowledgement
  Test: native_outbound_custody_intake
  Given host-bound registration and fixture-only intake plus overlapping polls and leases
  When complete fractional Matrix payloads arrive and responses are replaced or lost
  Then only current exact content is retained and stale responses cannot acknowledge replacements

Scenario: Processing survives interruption without duplicate effects
  Test: native_outbound_custody_processing
  Given ordered Matrix and independent work deliveries with claimed or started processing
  When deadlines expire the store restarts or responses disappear
  Then unstarted work can retry while started work requires inspected reconciliation and exact result replay

Scenario: Credential rotation preserves registration custody
  Test: native_outbound_custody_rotation
  Given acknowledged uncertain and probe work plus a frozen publication
  When the machine generation changes under an unchanged Matrix registration
  Then retained work preserves original payload and consumer while old scopes probes and publications are fenced

Scenario: Publication retries preserve bytes and monotonic receipts
  Test: native_outbound_custody_publication
  Given bounded host observations and an outbound update
  When update responses are lost duplicated rejected or delayed
  Then sequence content and observation timestamps stay frozen and accepted responses reconcile exactly

Scenario: Migration rollback and finite custody limits retain evidence
  Test: native_outbound_custody_storage
  Given schema-one fixtures failure injection and exhausted record byte or attempt budgets
  When migration or a lifecycle transaction fails
  Then previous schema and custody remain intact and no pending or duplicate history is dropped

Scenario: The bounded worker serializes concurrent host lifecycle commands
  Test: native_outbound_custody_worker
  Given concurrent host requests and a bounded custody writer
  When scopes and delivery identities race or queue responses time out
  Then only exact current commands commit and unknown responses remain reconcilable

## Out of Scope

No HTTP collector TLS credentials SDK authentication Matrix provenance adapter
domain admission proof or live integration is provided by this lifecycle kernel.
Completed result receipts describe host adapter handoff only not canonical tasks.
Publication authority and status freshness must be verified by the future host
adapter; frozen retries do not claim a fresh observation. Continuous retention
beyond finite tombstone capacity remains an explicit release gate.
