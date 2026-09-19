spec: task
name: "Qualify explicit completion through real native MCP and owned child pipes"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREAD-SCOPED-SESSIONS, REQ-THREE-LAYER-COMPLETION]
tags: [active, rust, mcp, completion]
---

## Intent

Drive the new explicit finish through generated native helper configuration and
the real private HTTP writer boundary, keeping runner exit and task outcomes separate.

## Constraints

### Must
- Require exact scoped task identity and immutable receipt replay.
- Preserve on-request workspace-write default network policy and reject approval requests.
- Observe real native helper traffic with no live model or service credentials.
- Keep incomplete macOS cleanup as refusal and qualify positive cleanup only on actual platforms.

### Must Not
- Do not turn protocol text into canonical Done or send permission.
- Do not claim helper acknowledgement or exit when the retirement race prevents observation.
- Do not expose private capability values or route data in tool receipts.

## Boundaries

### Allowed Changes
- native/hagency-core/**
- native/hagency-store/**
- native/hagency-execution/**
- native/hagency/src/runner/**
- native/hagency/src/runner.rs
- native/hagency/src/task_client/**
- native/hagency/src/task_client.rs
- native/hagency/src/mcp/**
- native/hagency/src/mcp.rs
- native/hagency/tests/**
- native/hagency-runtime/src/codex/session/task_mcp.rs
- specs/task-rust-owned-completion.spec.md
- specs/task-rust-owned-completion-integration.spec.md
- knowledge/decisions/adr-060-native-owned-completion.md
- docs/**

## Acceptance Criteria

Scenario: Actual native helper finish retains cleanup gates
  Test: native_owned_mcp_real_finish
  Level: integration
  Test Double: offline native app-server peer invoking the real native MCP helper and fresh writer
  Given generated fixed on-request workspace-write network-disabled helper settings and an owned native child
  When the helper explicitly marks the assigned task Done with full final content
  Then only observed whole_tree_stopped leader_exited and signals_accepted admit that exact original-room reply while canonical Done remains true when those observations are missing

Scenario: Invalid historical capability and conflicting content are rejected
  Test: native_owned_completion_mcp_scope_replay
  Level: integration
  Test Double: native MCP session over actual private loopback HTTP against the fresh canonical writer
  Given a committed explicit finish and its fenced old capability
  When identical receipt replay conflicting body foreign fields generic task calls and oversized frames are submitted
  Then only exact receipt replay succeeds without exposing body route or capability

Scenario: Task-only Done still fences the old execution epoch
  Test: native_owned_mcp_real_done_epoch
  Level: integration
  Test Double: real native helper through an offline owned app-server peer
  Given plain task-only Done
  When fresh renewal sees the epoch change
  Then canonical Done remains true with no final reply and no resumed execution

## Out of Scope

Actual model execution, native service enablement, Matrix delivery, effective
sandbox qualification and automatic ownerless reporting recovery.
