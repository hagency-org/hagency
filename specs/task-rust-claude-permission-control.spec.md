spec: task
name: "Drive exact one-shot Claude permission responses under retained ownership"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-LOCAL-THREE-RUNNER-QUALIFICATION, REQ-EXECUTION-AUTHORIZATION, REQ-OWNER-UI-APPROVAL]
tags: [active, rust, claude, approval]
---

## Intent

Implement the missing native Claude callback transport mechanics required by the
original owner approval coordinator. The official Agent SDK control channel is
the noninteractive boundary; it is not an owner verdict or a new public API.

## Constraints

### Must
- Enable response control explicitly on the original initialized running session.
- Retain original input internally; allow exactly that input or send a fixed deny.
- Prepare each original request once with a non-cloneable session-bound frame.
- Require finite owner and response deadlines derived from original receive time,
  bounded by the original process lifetime; no control wake may renew them.
- Continue reading stdout and stderr while a separately pinned host/domain future
  is pending; return received messages to the host before continuing writes.
- Cancelled requests and observed result frames must prevent further responses.
- Preserve partial-write custody across observed messages, never retry a frame.
- Returning a write receipt proves only full acceptance and flush, not application.
- Close streams and stop the retained owner on polled-future cancellation/error.
- Bound callback count, input retention, frame bytes and private stderr memory.

### Must Not
- Do not accept caller replacement tool input, permission rule updates, bypass
  flags, arbitrary denial text, JSON verdicts or a fabricated domain grant.
- Do not enable production Claude, add new owner authorization, change Codex
  approval semantics, copy credentials or use live services in ordinary tests.
- Do not interpret a late cancellation or successful result as process cleanup.

## Boundaries

### Allowed Changes
- native/hagency-runtime/src/claude.rs
- native/hagency-runtime/src/claude/session.rs
- native/hagency-runtime/src/claude/session/**
- native/hagency-runtime/src/owned/claude.rs
- native/hagency-runtime/src/bin/claude_probe/mod.rs
- native/hagency-runtime/tests/claude_control.rs
- native/hagency-runtime/tests/claude.rs
- native/hagency-runtime/tests/claude_owned.rs
- native/hagency-runtime/examples/claude_permission_probe.rs
- knowledge/decisions/adr-005-supported-runtime-approval-adapters.md
- knowledge/decisions/adr-156-native-claude-permission-control.md
- specs/task-rust-claude-permission-control.spec.md
- docs/progress.md
- docs/agent-knowledge.md

## Acceptance Criteria

Scenario: Responses preserve original callback input and exact association
  Test: native_claude_permission_exact_response
  Level: integration
  Test Double: bounded synthetic bidirectional Claude peer
  Given original callbacks selected allow or deny and foreign prepared frames
  When an explicitly enabled original session prepares and sends a decision
  Then only the original input and request ID are sent once with no permission updates and cross-session or repeated responses refuse

Scenario: Pending host work does not stop callback intake or extend time
  Test: native_claude_permission_control_deadlines
  Level: integration
  Test Double: deterministic clock and retained pinned host futures
  Given delayed decisions notifications cancellations and fixed original deadlines
  When host control returns or a callback expires
  Then events retain order host futures survive message returns and no wait or write deadline renews

Scenario: Cancellation and result barriers fence response bytes
  Test: native_claude_permission_write_barriers
  Level: integration
  Test Double: in-memory fragmented frames and stdin backpressure
  Given prepared responses and queued partial or complete cancellation and result frames
  When the response writer advances
  Then observed barriers reach the host first and cancelled or ended requests cannot transmit further bytes while partial-write failures remain explicit

Scenario: Callback state is bounded and started future drops close custody
  Test: native_claude_permission_bounds_and_cancel
  Level: integration
  Test Double: synthetic requests and bounded stalled streams
  Given excess input requests invalid control policies and dropped started futures
  When the driver admits or sends callbacks
  Then all bounds fail closed with no reusable frame or detached IO owner

Scenario: Original process pipes carry the same one-shot permission response
  Test: native_claude_owned_permission_roundtrip
  Level: integration
  Test Double: native local offline fixture through actual guardian pipes
  Given an owned peer requesting a tool permission
  When the native wrapper delivers allow deny or observes cancellation
  Then exact response bytes or refusal are observed and process cleanup remains the platform's separate original observation

Scenario: Operator diagnostic forwards identity without importing credentials
  Test: native_claude_probe_environment
  Level: unit
  Test Double: synthetic operator identity strings without a provider process
  Given an operator HOME USER and private temporary directory
  When the diagnostic constructs its cleared child environment
  Then only fixed OS identity paths and diagnostic flags are forwarded and no token API key or provider namespace override is imported

## Decisions

ADR156 and the native-profile amendment to ADR005 define the supported boundary.
ADR154/155 remain binding except that the explicit control API now supplies
one-shot response encoding. Production admission and domain authorization remain
separate gates; the codec/session does not manufacture either.

An explicitly activated operator-only example may exercise the installed local
Claude through these native pipes with a fixed harmless prompt, safe/restricted
mode, Bash-only tool list, an explicit Bash ask rule, deny-only responses, empty
MCP configuration, no session persistence, a private empty workspace and finite
time/cost bounds. The provider CLI may use its own existing operator login; no
credential copying or Hagency managed-account readiness is inferred. Ordinary
Cargo test never runs this example. Report only fixed booleans/counts/errors and
actual cleanup flags. Retain its workspace when whole-tree cleanup is unproven.
This diagnostic is separate from production Host, Matrix and soak acceptance.
The installed CLI adds pending_permission_requests and pending_user_dialog_requests
arrays to its initialize success envelope. Accept these optional fields only when
they are empty arrays; never silently adopt or discard preexisting pending work.
The original native_claude_stream_envelopes selector covers these actual shapes.

## Out of Scope

Production Host enablement, durable approval coordinator adaptation, scoped MCP
launch configuration, usage/account binding, Octos, containment and live soaking.
