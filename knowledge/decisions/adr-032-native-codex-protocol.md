---
kind: decision
id: ADR-032
title: Bound Codex protocol observations before integrating runner authority
status: Accepted
requirements: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREAD-SCOPED-SESSIONS]
---

## Context

Codex wire data needs a bounded protocol and request-correlation boundary before it can participate in native runner authorization.

## Decision

### Scope and evidence

M4 starts a separate `hagency-runtime` crate for runner adapters. Its first slice
contains an IO-free Codex App Server wire codec and connection state. It does not
depend on the domain store or platform crate and is not linked into the native
server. Native Agent execution remains unavailable.

The installed `codex-cli 0.153.4` exported the protocol with
`codex app-server generate-json-schema --out <temporary-cache>`. This tooling
command did not start an Agent, load a project or invoke a model. The versioned
fixture under `native/hagency-runtime/tests/fixtures/` includes selected generated
schemas, their original file SHA-256 digests, and reusable envelope vectors. The
tests consume the vectors offline; they do not depend on an installed Codex binary.
This establishes a protocol baseline, not a supported production runtime matrix.

The [official App Server documentation](https://learn.chatgpt.com/docs/app-server)
describes newline-delimited JSON over stdio, omitted `jsonrpc` headers, one
initialize handshake per connection, and separate interrupt and turn events.
The installed schema also has optional request trace metadata and signed int64
or string IDs. We follow those shapes while imposing narrower finite string,
nesting and frame limits. Other runtime versions require requalification.

### Wire and memory boundaries

- At most 1 MiB before each LF, including a possible CR; one completed message
  per receive call. The caller processes that message before feeding the suffix.
  No vector or queue accumulates arbitrary notifications. Outbound serialization
  writes through a bounded buffer before returning bytes.
- At most 64 JSON value levels, plus the parser's enabled recursion bound. Duplicate
  object keys are refused at every depth. Invalid UTF-8, batches, scalar envelopes,
  invalid IDs, mixed response/error/request shapes and unknown envelope fields
  close the connection. Primitive/null params and explicit null results remain
  valid. String and integer request IDs retain separate namespaces.
- Methods are bounded to 128 bytes; string request IDs and correlation identifiers
  to 256 bytes. Locally generated errors contain no input fragments. Peer error
  messages and other payloads remain untrusted private data; receiving them does
  not make them suitable for console projection or logs.
- A partial line expires after 10 seconds from its first byte. Whitespace and
  other fragments do not extend that deadline. The host supplies monotonic elapsed
  milliseconds; a backwards clock closes the connection. The future IO adapter
  must call `tick` from a bounded timer during silence as well as traffic.

### Connection and request custody

Initialize is the only host request admitted in the new state. A response must
match its numeric ID and contain the generated schema's required string fields.
Those fields are observations, not proof of effective sandbox or platform support.
The host then obtains the initialized notification before issuing other requests.
All writes must remain ordered; returning bytes is not evidence that the child
received them. Write failure must close the connection, never replay a partial
request into another process.

Host request IDs increase monotonically and never reuse a completed ID. At most
32 requests may await responses. Each has an absolute deadline of 1 through
1,200,000 ms supplied by the host. Unrelated events cannot extend it. Out-of-order
responses resolve only their matching request; missing, duplicate, unknown or
type-substituted IDs terminate the connection. A matched ordinary RPC error is
returned as data and does not automatically retry its operation.

Server requests use a separate ID namespace from host requests. Up to 32 may be
outstanding and up to 1,024 IDs are retained for the connection's entire lifetime.
Exceeding either server limit closes the connection instead of evicting a
tombstone. A server resolution retires its pending request; a resolution that
arrives before its request records a tombstone too. Resolution of a pending
thread request must match that request's thread. Resolved or rejected IDs cannot
become actionable again. The one-shot runner must finish or fail before exhausting
these finite limits; it must not reset the connection to continue ambiguous work.

The only implemented server response is an explicit unsupported-handler error
for an outstanding ID. There is no allow/success response constructor on
`Connection`. `Message` is a wire representation, not a permission decision;
its standalone encoder grants nothing. M6 must add typed current-scope approval
correlation and durably applied decisions before an adapter can deliver allow.

Timeout, malformed input, EOF with partial data or pending requests, and transport
failure make the connection permanently inert. Clean frame-boundary EOF with no
pending RPCs only means the protocol stream ended cleanly; it does not establish
that an upstream turn, child tree, dispatch or canonical task completed. There is
no reconnect, resume, resend or retry API in this slice.

### Cancellation and remaining integration

`TurnScope` is a host-supplied pair of upstream correlation strings. It is not an
authenticated Hagency session, task, dispatch, approval or process identity. Its
typed interrupt method serializes exactly that pair and rejects simultaneous
duplicate cancellation requests. A matched empty-object response yields
`InterruptAcknowledged`; an RPC error yields `InterruptRejected`. Arbitrary
results fail. None of these events terminate a process or settle a task.

All runtime notifications, including `turn/completed`, remain untrusted events.
The future typed adapter must validate exact upstream thread, turn, item, current
capability and operation digest before using them. Model-authored `done` or
approval prose can never substitute for structured durable task/approval calls.

Still required before enabling native Codex execution: frozen dispatch-to-thread
binding; strict thread/turn request schemas and effective sandbox observations;
complete stdin acceptance acknowledgement; bounded stdout, stderr and write
deadlines; owner approval parking and expiration; MCP operation correlation;
guardian launch/termination and inspection; resource/dirty-state custody; output,
usage and activity projection; explicit task completion; actual supported-runtime
tests on Linux, macOS and Windows. This protocol foundation proves none of those
integration gates and changes no production configuration or deployment.

## Consequences

The IO-free codec supplies protocol observations only. Dispatch binding, stream ownership, sandbox enforcement and durable approval or task authority remain separate integration gates.

## Alternatives Considered

Treating runtime notifications or model-authored completion and approval prose as authority would bypass structured host checks. Unbounded frames or unresolved request history would undermine finite connection custody.
