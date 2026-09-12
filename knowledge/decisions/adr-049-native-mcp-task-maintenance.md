---
id: ADR-049
title: Native MCP maintenance of the assigned canonical task
status: accepted
date: 2026-09-10
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREE-LAYER-COMPLETION, REQ-THREAD-SCOPED-SESSIONS]
---

# Native MCP maintenance of the assigned canonical task

## Context

The migration needs native MCP task maintenance through the same inherited context and scoped loopback writer as the CLI helper.

## Decision

The migration requires native MCP and task helpers. ADR-041 already provides the
scoped loopback client; use that same transport and writer for the first MCP tools.
The dedicated `hagency mcp` subprocess receives the same inherited host context.
It never opens a domain database, discovers identity via tmux, accepts operator
credentials, launches another helper or writes a PID/credential file.

Implement the MCP 2025-11-25 stdio lifecycle and a finite task tool catalog:
get_task, accept_task, transition_task, comment_task and update_task_execution.
Only the assigned task ID is accepted. Explicit stable call_id is required for
mutations and remains independent of the connection-local JSON-RPC ID. A new
connection may retry identical call_id/content; only the domain writer can replay
it. Cross-task access, delegation, graphs, attachments, approval and task creation
are not advertised by this checkpoint. No generated/live runtime configuration
is switched to the new helper yet.

The helper processes one request at a time, with kernel pipe backpressure and no
user-space request queue. Frames are newline-delimited UTF-8 JSON, at most 32 KiB,
strictly parsed with duplicate keys and ambiguous IDs refused. Request IDs retain
type and are unique within the process (4096 total, 128-byte string maximum).
The protocol requires initialize followed by notifications/initialized; unsupported
protocols negotiate this supported version and the client may disconnect. Only
tools capability is advertised. Tool failures use isError with sanitized content;
protocol errors cannot become successful tool results. Notifications receive no
response. A late cancellation cannot undo a completed canonical operation.

The dedicated executable uses synchronous stdin/stdout on its main thread, not
Tokio's uncancellable background stdio reader. One fixed watchdog thread imposes
an absolute 10-second partial-frame deadline and a 5-second output deadline.
Idle between frames has no deadline. If synchronous OS IO cannot finish, the
watchdog terminates this helper process with exit code 74; it does not kill or
signal any other process. The main loop owns all IO and shuts the watchdog down
and joins it on normal exit. A current-thread Tokio runtime is used only for the
existing bounded local HTTP operation. No IO is shared with a long-lived daemon.

Process termination can lose a mutation response: it is not rollback, task Done,
or proof of runner cleanup. Call ID receipts survive in the backend and the parent
must treat a closed connection as uncertain, inspect or retry the exact call. This
explicit helper-process boundary avoids abandoned stdio threads or OS-specific
stdin handle replacement. Runtime launch ownership and generated configuration
remain separate migration gates.

Use the official MCP lifecycle, stdio and tool contracts, with installed SDK
client interoperability as an offline fixture. The cached Rust SDK rmcp 1.8.0
AsyncRwTransport reads an unbounded line and its default codec has usize::MAX;
it is not used as the framing boundary. This small sequential adapter does not
advertise prompts, resources, logging, sampling, async MCP tasks or subscriptions.

Sources:
- https://modelcontextprotocol.io/specification/2025-11-25/basic/transports
- https://modelcontextprotocol.io/specification/2025-11-25/basic/lifecycle
- https://modelcontextprotocol.io/specification/2025-11-25/server/tools
- https://github.com/modelcontextprotocol/rust-sdk/tree/25220361d5540715294c501c289d79de4bec2bfc/crates/rmcp/src/transport

## Consequences

A dedicated bounded stdio process exposes only its implemented task tools. Database access, identity discovery and broader MCP or runtime readiness remain outside that helper's authority.

## Alternatives Considered

Using the cached SDK's unbounded default line framing as the input boundary would defeat finite admission. Opening state or accepting operator credentials inside MCP would bypass the runner API's task scope.
