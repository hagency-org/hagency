spec: task
name: "Provide a native scoped task maintenance client"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREE-LAYER-COMPLETION, REQ-THREAD-SCOPED-SESSIONS]
tags: [active, rust, cli, tasks]
---

## Intent

Expose task maintenance as native Hagency subcommands using the existing private
runner HTTP API. Task truth and idempotency remain in its one domain writer.

## Constraints

### Must
- Obtain scoped dispatch credentials only from the host-provisioned environment.
- Address only a literal loopback socket and send complete capability headers.
- Keep task identity fixed by the runner context and require a call ID for each mutation.
- Send one explicit canonical operation per command and keep stable request bytes on caller retry.
- Bound connection headers response data and the whole operation deadline.
- Distinguish transport uncertainty from canonical success; never automatically retry a mutation.
- Keep credentials and raw remote failure bodies out of stdout stderr and errors.
- Exercise the real local HTTP API and canonical persistence without live external services.

### Must Not
- Do not open writable task state in the client or infer Done from model output.
- Do not follow redirects use environment proxies resolve arbitrary DNS or accept an operator token fallback.
- Do not write runner credentials into configuration files or accept secrets in CLI arguments.
- Do not enable actual runners or alter deployed scripts in this checkpoint.

## Boundaries

### Allowed Changes
- ./Cargo.lock
- native/hagency/Cargo.toml
- native/hagency/src/lib.rs
- native/hagency/src/main.rs
- native/hagency/src/task_client.rs
- native/hagency/src/task_client/**
- native/hagency/tests/task_client.rs
- native/README.md
- specs/task-rust-task-client.spec.md
- knowledge/decisions/adr-041-native-task-client.md
- docs/**

### Forbidden
- Live services, credentials and the original dirty checkout.

## Acceptance Criteria

Scenario: Native maintenance reaches the sole canonical task writer
  Test: native_task_client_lifecycle
  Level: integration
  Test Double: loopback native API with fresh domain store
  Given a started scoped runner with one canonical task
  When the native client writes heartbeat wait resume comment and done
  Then the exact task changes through the existing state machine
  And identical call retries replay while changed content conflicts

Scenario: Invalid credentials and cross-scope requests fail closed
  Test: native_task_client_scope
  Level: integration
  Test Double: loopback native API with fresh domain store
  Given missing substituted stale parked or wrong-task credentials
  When maintenance is attempted
  Then no task mutation is authorized and no operator fallback occurs

Scenario: Network failure is bounded and never rewritten as success
  Test: native_task_client_transport
  Level: integration
  Test Double: controlled local HTTP peer
  Given redirects oversized responses stalled bodies or lost mutation replies
  When a native request reaches its finite deadline or fails
  Then no automatic retry occurs and the outcome remains unavailable or unknown
  And private credentials and raw failure bodies are absent from diagnostics

Scenario: CLI credentials stay in the inherited environment
  Test: native_task_client_cli
  Level: integration
  Test Double: native binary with isolated environment and loopback fixture
  Given bounded environment context and a selected command
  When the executable validates arguments or runs a task operation
  Then no secret argument or persisted configuration is required
  And output is bounded structured task result or a sanitized error

## Out of Scope

Host runner environment provisioning, full MCP and hook parity, task creation or
allocation, persistent-home legacy task selection, remote endpoints and live rollout.
