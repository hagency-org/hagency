spec: task
name: "Drive native Codex IO with bounded transport custody"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREAD-SCOPED-SESSIONS]
tags: [active, rust, runner, transport]
---

## Intent

Connect the native Codex protocol to host-owned asynchronous stdin, stdout and
stderr streams without starting a runtime or assuming dispatch authority.

## Constraints

### Must
- Own all supplied streams and drop them on transport failure or cancellation.
- Return a transport write receipt only after the complete frame and flush succeed.
- Keep transport write acceptance separate from RPC response and domain acknowledgement.
- Enforce absolute Instant-derived protocol write wait and connection deadlines during silence and continuous traffic.
- Bound read buffers and queued event count plus complete payload and metadata bytes.
- Refuse event overflow without silently discarding server requests.
- Drain stderr concurrently into a bounded private diagnostic tail with an explicit total-byte count.
- Close the connection when an in-flight operation future is dropped and retain an observable unresolved termination record.
- Yield cooperatively during always-ready traffic so timer and cancellation polling remain possible.
- Use offline duplex stream fixtures for partial writes blocked writers partial frames EOF floods and cancellation.

### Must Not
- Do not spawn models or assert child termination canonical completion or released domain leases.
- Do not introduce hidden background tasks unbounded channels replay reconnect or automatic approval.
- Do not project stderr into console output or logs.

## Boundaries

### Allowed Changes
- ./Cargo.lock
- native/hagency-runtime/**
- specs/task-rust-codex-transport.spec.md
- knowledge/decisions/adr-034-native-codex-transport.md
- docs/**

### Forbidden
- Live services, credentials and the original dirty checkout.

## Acceptance Criteria

Scenario: Complete byte delivery remains distinct from higher acknowledgement
  Test: native_codex_transport_write
  Given owned asynchronous streams and a responding fixture
  When frames are written and responses arrive before or after flush completion
  Then write receipts describe only complete byte delivery
  And protocol events remain separate untrusted observations

Scenario: Failed and cancelled transport operations never replay
  Test: native_codex_transport_failures
  Given a partial or blocked writer and a pending operation
  When timeout cancellation broken IO or EOF occurs
  Then streams close the connection stays inert and unresolved progress remains inspectable
  And no background task channel replay reconnect or automatic approval continues after cancellation

Scenario: Absolute clocks survive silence partial input and traffic floods
  Test: native_codex_transport_deadlines
  Given finite operation request and connection deadlines
  When silence partial frames or always-ready traffic continues
  Then the original deadline closes the transport without extension
  And other asynchronous work can still poll cancellation

Scenario: Diagnostic and event pressure remain bounded
  Test: native_codex_transport_pressure
  Given stderr floods and queued protocol events containing large payloads
  When a writer is blocked or the host consumes input
  Then stderr is drained into a bounded private tail with a total count
  And event count or complete byte overflow closes without a silent actionable-request drop

## Out of Scope

Model spawn and guardian handoff, actual child stdin identity, domain payload ACK,
thread/turn/item authority, approvals, sandbox policy, process inspection, usage,
Matrix delivery, production integration and supported-runtime qualification.
