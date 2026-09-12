spec: task
name: "Maintain assigned canonical tasks through native MCP stdio"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREE-LAYER-COMPLETION, REQ-THREAD-SCOPED-SESSIONS]
tags: [active, rust, mcp, tasks]
---

## Intent

Provide an executable native MCP task helper using the existing scoped task API.

## Constraints

### Must
- Bind all task tools to the inherited host capability and assigned task.
- Require a stable explicit call ID for every mutation and preserve canonical replay.
- Enforce MCP initialization and advertise only implemented tools.
- Bound frames output request identity retention and operation deadlines.
- Keep synchronous stdio custody inside the dedicated helper process.
- Use exact native API fixtures and installed SDK interoperability without live services.

### Must Not
- Do not expose operator fallback credentials or another task scope.
- Do not retry uncertain mutations or infer task completion from protocol output.
- Do not create another local store or replace deployed MCP configuration.

## Boundaries

### Allowed Changes
- ./Cargo.lock
- native/hagency/Cargo.toml
- native/hagency/src/mcp.rs
- native/hagency/src/mcp/**
- native/hagency/src/main.rs
- native/hagency/src/lib.rs
- native/hagency/src/task_client.rs
- native/hagency/tests/task_client.rs
- native/hagency/tests/task_client/**
- native/hagency/tests/mcp.rs
- native/hagency/tests/mcp/**
- native/README.md
- specs/task-rust-mcp-task-maintenance.spec.md
- knowledge/decisions/adr-049-native-mcp-task-maintenance.md
- docs/**

### Forbidden
- Live services, original dirty checkout and runtime credentials.

## Acceptance Criteria

Scenario: MCP tools mutate only the assigned canonical task
  Test: native_mcp_task_lifecycle
  Level: integration
  Test Double: native API with fresh domain store and native helper
  Given initialized MCP and a started scoped task
  When maintenance and exact call retries are submitted
  Then the canonical writer commits or replays the exact operation
  And substituted task IDs and conflicting retries are refused

Scenario: Protocol identity lifecycle and capability exposure stay bounded
  Test: native_mcp_protocol
  Level: integration
  Test Double: local framed protocol fixture
  Given malformed duplicate oversized or uninitialized requests
  When the helper handles the protocol frame
  Then no canonical operation is dispatched
  And only implemented task tools and sanitized responses are exposed

Scenario: Stdio ownership ends without an abandoned helper thread
  Test: native_mcp_stdio
  Level: integration
  Test Double: native executable and controlled local pipes
  Given EOF partial frames or a blocked consumer
  When normal shutdown or the absolute IO deadline occurs
  Then the whole dedicated helper exits without reporting canonical success

Scenario: Installed SDK can use the native task protocol
  Test: native_mcp_sdk
  Level: integration
  Test Double: installed MCP SDK fixture and local native executable
  Given an SDK client connected to the native helper
  When it initializes lists tools and reads its assigned task
  Then the supported protocol response and typed task result are accepted

## Out of Scope

Full graph peer file and approval tools, general task listing, live runner host
provisioning, generated configuration and production rollout.
