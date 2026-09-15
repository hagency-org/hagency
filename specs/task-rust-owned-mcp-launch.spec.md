spec: task
name: "Configure one native task MCP helper from owned host dispatch authority"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREAD-SCOPED-SESSIONS, REQ-THREE-LAYER-COMPLETION]
tags: [active, rust, mcp, runner]
---

## Intent

Generate a fixed native task helper configuration using exact host dispatch context,
and verify real helper task maintenance separately from upstream and cleanup outcomes.

## Constraints

### Must
- Use the pinned Codex 0.153.4 configuration and environment-forwarding semantics.
- Derive task identity and capability from the exact owned dispatch scope and claim.
- Keep capability values only in private inherited launch environment, never config or prompt bytes.
- Preserve fixed on-request workspace-write network-disabled policy and deny unsupported approvals.
- Keep canonical Done and its epoch increment separate from upstream completion or lease release.
- Require actual helper response readback and exit for the heartbeat fixture; record Done acknowledgement and exit as observed or unknown.
- Keep the exact existing fingerprint and negative historical fence after task epoch changes.

### Must Not
- Do not accept arbitrary model-supplied helper command arguments environment URL or identity.
- Do not enable a live runner or claim actual model sandbox or hostile-runtime confidentiality.
- Do not add schema changes Matrix replies or task retries after Done.

## Boundaries

### Allowed Changes
- ./Cargo.lock
- native/hagency/Cargo.toml
- native/hagency/tests/owned_mcp.rs
- native/hagency-execution/**
- native/hagency-runtime/src/codex/session.rs
- native/hagency-runtime/src/codex/session/task_mcp.rs
- native/hagency/tests/fixtures/owned_mcp_peer.rs
- knowledge/decisions/adr-057-native-owned-mcp-launch.md
- specs/task-rust-owned-mcp-launch.spec.md
- docs/**

## Acceptance Criteria

Scenario: Helper configuration carries names rather than capability values
  Test: native_task_mcp_host_configuration
  Level: unit
  Test Double: generated settings inspected against pinned upstream field semantics
  Given an exact native helper path and assigned task
  When typed settings generate thread configuration
  Then only fixed command tools environment names and task guidance are emitted with unchanged execution policy

Scenario: Host helper admission refuses foreign context
  Test: native_owned_mcp_configuration_admission
  Given an explicit loopback address and owned dispatch claim
  When helper host inputs are malformed or prepopulate reserved context names
  Then admission fails before the native child starts

Scenario: Actual native helper maintains the assigned task
  Test: native_owned_mcp_real_heartbeat
  Level: integration
  Test Double: offline app-server peer launching actual native hagency mcp against a fresh local writer
  Given a valid generated helper configuration
  When real MCP heartbeat and readback succeed and the helper exits
  Then upstream completion remains separate from canonical InProgress and whole-tree stop evidence

Scenario: Canonical Done advances epoch without releasing custody
  Test: native_owned_mcp_real_done_epoch
  Level: integration
  Test Double: offline app-server peer launching actual native hagency mcp against a fresh local writer
  Given an actual helper transition durably committed as Done
  When the coordinator observes its frozen epoch no longer matches
  Then canonical Done remains true while the dispatch is fenced with its lease retained and interrupted helper acknowledgement remains unknown

## Out of Scope

Actual Codex model execution, portable sandbox confidentiality, physical workspace
provisioning, warm reuse, approval application, final Matrix delivery and service enablement.
