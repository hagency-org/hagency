spec: task
name: "Bound the native Codex App Server protocol"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREAD-SCOPED-SESSIONS]
tags: [active, rust, runner, protocol]
---

## Intent

Prepare an IO-free Codex App Server JSONL boundary for the native one-shot runner.
Parse bounded messages and correlate requests without promoting runtime text or
protocol acknowledgements into execution authority.

## Constraints

### Must
- Bound frames, nesting, partial-frame age, pending requests and retained server request identities.
- Follow the installed Codex 0.153.4 JSON schema for wire envelopes and preserve string versus signed integer request identities.
- Require initialize response followed by initialized notification before normal requests.
- Reject malformed or ambiguous envelopes, duplicate fields and unknown response identities permanently for the connection.
- Expire requests by an absolute host-provided monotonic clock even while unrelated notifications arrive.
- Keep a failed or closed connection inert and expose no replay operation.
- Track server request IDs without rearming resolved IDs; expose only explicit error responses until the approval adapter is implemented.
- Keep interrupt acceptance separate from turn completion, child cleanup and canonical task completion.
- Keep locally generated protocol errors sanitized and all tests offline.

### Must Not
- Do not launch a model, read credentials, introduce public endpoints or accept dispatch identity from runtime payloads.
- Do not claim complete runner, approval, sandbox or child custody parity.
- Do not grant approval from an RPC request, result or notification.

## Boundaries

### Allowed Changes
- ./Cargo.toml
- ./Cargo.lock
- native/hagency-runtime/**
- specs/task-rust-codex-protocol.spec.md
- knowledge/decisions/adr-032-native-codex-protocol.md
- docs/**

### Forbidden
- Live services, credentials and the original dirty checkout.

## Acceptance Criteria

Scenario: Incremental framing remains bounded and unambiguous
  Test: native_codex_frames
  Given fragmented UTF-8 and multiple messages in a byte stream
  When the codec receives frames or malformed input
  Then it returns one complete envelope at a time within finite limits
  And duplicate keys invalid identities excessive nesting and partial EOF permanently fail

Scenario: Initialization and response correlation fail closed
  Test: native_codex_correlation
  Given a new connection and concurrent host requests
  When responses arrive out of order with exact or substituted IDs
  Then exact requests resolve only once after the initialization handshake
  And unknown duplicate or type-substituted IDs permanently fail

Scenario: Server requests cannot rearm or authorize themselves
  Test: native_codex_server_requests
  Given server-initiated requests and resolved identities
  When the host rejects requests or receives duplicates and capacity overflow
  Then only outstanding IDs accept one error response
  And retired IDs never become actionable again

Scenario: Timeout EOF and interruption never assert completion
  Test: native_codex_failure_and_interrupt
  Level: integration
  Test Double: offline byte stream and host clock
  Given pending requests an active turn scope and arbitrary runtime notifications
  When cancellation is acknowledged or transport and absolute deadlines fail
  Then acceptance remains distinct from completion and a failed connection stays inert
  And no automatic retry or canonical task mutation occurs
  And malformed private input produces only a sanitized protocol error

## Out of Scope

Actual child transport and write acknowledgements, runtime version qualification,
thread/turn/item authority binding, approvals, sandbox observation, process cleanup,
model execution, persistence, Matrix delivery and full M4 integration.
