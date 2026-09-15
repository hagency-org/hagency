---
id: ADR-051
title: Scoped native MCP coordination through the existing runner API
status: accepted
date: 2026-09-10
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREE-LAYER-COMPLETION, REQ-THREAD-SCOPED-SESSIONS]
---

# Scoped native MCP coordination through the existing runner API

## Context

Native MCP coordination needs canonical conversation, delegation and graph operations without allowing the helper to choose its actor or transport authority.

## Decision

Extend ADR-049's dedicated helper with fourteen implemented coordination tools.
Keep the five assigned-task tools and their exact task scope. Every operation
uses the existing canonical runner API and inherited Context; the helper opens
no store and cannot select an actor, capability, URL, path, HTTP method or host
observation. This is a bounded M3/M6 integration checkpoint, not MCP parity or a
change to generated runtime configuration.

| Tools | Existing native runner resource | Scope and result |
| --- | --- | --- |
| delegate_task | POST delegations | Current canonical source/task to an eligible project engagement; returns a durable task intent, initially pending host notice delivery |
| open_conversation / get_conversation | POST conversations / GET conversations/{id} | Caller-derived creator, active project engagements and exact current internal participant sessions |
| update_conversation_members / close_conversation | POST conversations/{id}/operations | Exact creator session and expected revision; existing membership retirement remains canonical |
| send_peer_message / read_peer_inbox | POST peer-messages / GET peer-inbox | Exact current recipient sessions; reads only this dispatch's frozen peer inputs |
| create_graph / get_graph / list_graphs | POST graphs / GET graphs/{id} / GET graphs | Exact creator session and admitted conversation; node assignees are participant session IDs, not engagement IDs |
| cancel_graph | POST graphs/{id}/cancel | Creator-only cancellation, retaining host stop custody for started work |
| report_graph_result | POST graphs/{id}/results | Exact node task/session/epoch; Complete requires canonical Done, Failed requires Blocked and fences the worker |
| read_graph_dependencies / read_graph_dependency | GET / POST graphs/{id}/dependencies | Bounded authorized references and one committed result; POST hydration is a read |

Delegation and graph creation admit canonical work; they do not start processes
or assert that a Matrix notice was delivered. The fixture host activates notices
and claims/starts dispatches separately. An ordinary peer message cannot turn an
unready graph node into runnable work. The child result does not complete the
parent. No tool mints a report grant, recovers an unknown process, observes a
host, approves permissions, provisions an allocation or sends a Matrix event.

The closed operation enum shares ADR-041's private local HTTP transport. Task
requests retain their 16 KiB serialized-body limit; coordination requests have a
32 KiB serialized-body limit. MCP frames remain 32 KiB, output 256 KiB, HTTP
response bodies 64 KiB, headers 16 KiB/32 fields, and each operation has the
existing five-second absolute deadline. The caller cannot increase these limits.
A graph, input or result allowed by the domain can exceed this helper's smaller
wire bound and is then refused explicitly; no data is truncated or split into
hidden requests. Existing sequential framing, finite request-ID history and
stdio watchdog behavior are unchanged.

Read pages default to eight items and permit at most 32. The helper requires
strictly increasing unique cursors greater than the requested cursor and never
accepts more items than requested. Arrays are returned inside objects named
`messages`, `graphs` or `dependencies`, compatible with MCP structuredContent.
Large result pages fail visibly at 64 KiB; clients must request smaller pages.
Single dependency hydration has the same finite response ceiling. Responses use
strict duplicate-key/trailing-document/depth parsing, existing typed DTO
projections and matching requested resource/node/task IDs. Unknown top-level
response fields are not forwarded; opaque peer/result data remain data.

Every mutation requires an explicit stable call_id, forwarded unchanged into
the domain's content-bound receipt. JSON-RPC IDs remain connection-local only.
A lost, malformed or oversized mutation response is OutcomeUnknown after possible
submission. Reconnection can retry the identical call_id and payload using a
still-current capability; there is no automatic retry. Credential/fence/session
changes cannot adopt a prior actor's authority. Dependency hydration remains a
read despite its POST method, so a failed read never claims a mutation outcome.
Failed graph reporting intentionally fences the capability; an identical retry
from that fenced worker is refused and the creator can inspect terminal state.

Offline fixtures run the actual `hagency mcp` subprocess with rmcp 1.8.0 and real
Salvo/SQLite APIs. They cover delegation replay, cross-project and same-Agent
wrong-session rejection, member removal, noncreator control, graph readiness,
Done/epoch/exact-node results, lost graph and conversation responses, corrupt
receipts, bounded strict projections and sanitized errors. Synthetic host
capabilities and scripted local HTTP servers are test doubles, not transport or
runtime readiness evidence.

Agent discovery, files/attachments, Matrix history, progress hooks, generated MCP
configuration and runtime launch/cutover remain separate gates. No live model,
Matrix account, Palpo server or deployment is exercised. Windows/Linux and broader
integrated migration acceptance require their own runs after integration.

## Consequences

Implemented tools reuse current runner API checks and durable receipts. Discovery, files, generated runtime configuration and production MCP parity remain separate gates.

## Alternatives Considered

Accepting arbitrary URLs, methods or capabilities in tool arguments would turn scoped coordination into a generic host endpoint. Direct store access would create a second authority path outside the runner writer.

## Amendment (2026-09-12): each helper refusal names its own class

A hosted failure of this helper was reported as `Error: Protocol` with exit 1.
At HEAD the helper funnels **21** `Error::Protocol` constructions into that one
verdict — 17 in `native/hagency/src/mcp.rs`, 3 in `native/hagency/src/mcp/json.rs`
and 1 in `native/hagency/src/mcp/stdio.rs` — of which **15** are on the
pre-`tools/list` path (11 / 3 / 1 respectively; `mcp.rs`'s twelfth,
"session already closed", is unreachable from the stdio loop, which returns on the
first refusal). The refusals are now named, and each class carries its own process
exit, so a spawning test attributes a load failure from the status alone — stderr
is only drained after the exit and cannot be load-bearing.

Five of those sites were reclassified from `Protocol` to `Framing` (three in
`stdio.rs`, two in `mcp.rs`); the other sixteen keep `Protocol`, each with a
`&'static str` tag naming its check.

- **`Framing` (exit 70)**: a bounded stdio frame was refused before it became a
  request — EOF with a partial frame, a frame over `FRAME_LIMIT` (32 KiB), or a
  response over `OUTPUT_LIMIT` (256 KiB). Records the bound and the observed
  size. Two conditions share this code deliberately: an EOF mid-frame and a frame
  over the advertised limit are both **stream faults the helper cannot answer** —
  in neither case is there a complete request to refuse, so neither can be
  reported as a protocol verdict, and both mean the peer's byte stream, not the
  host's session, is the thing that failed. The `detail` string separates them
  (`"stdin reached EOF with a partial frame"` vs `"frame exceeds FRAME_LIMIT"` vs
  `"response exceeds OUTPUT_LIMIT"`) so the exit code names the class and stderr
  names the site.
- **`Protocol` (exit 71)**: a well-formed frame outside the current MCP
  lifecycle or schema. Each site carries a `&'static str` tag naming the check
  (unrecognized notification, request-id shape, initialize params, projection),
  so the verdict is distinguishable without new variants.
- **`Io` (exit 72)** and **`Context` (exit 73)**: unchanged refusals, now also
  distinguishable by code.
- **74** remains the stdio watchdog's exit, unchanged; it is documented as a
  fourth value rather than folded into the enum, because it is not an `Error`.
- **101** is named too, by `exit_code_name` only: it is the Rust runtime's fixed
  exit for a panic that unwound out of `main`, a distinct and load-relevant class
  for a helper under a loaded host.

`main`'s `Mcp` arm previously propagated the `Err` through `?` to Rust's default
handler. That handler prints the **`Debug`** form — `Error: Protocol`, the bare
variant name — and always exits 1, which is exactly the captured hosted failure
and why its stderr named no check: the descriptive message was never printed. It
now prints the `Display` form (`native MCP framing refused (… bound N bytes;
observed M bytes)`) and exits with `Error::exit_code()`, so both the code and the
text name the cause.

The codes are **diagnostic only and carry no authority**. They exist so a
spawning test can attribute a refusal from the status; nothing branches on them,
no peer or protocol handshake may negotiate them, and they are not an operator
contract. The same clause covers the `detail` tags.

One residual is recorded rather than fixed. `process::exit` runs no destructors,
so the new path skips the `stdout` flush that `StdoutLock`'s drop would have
performed. No response can be lost by that in the normal case: the loop flushes
explicitly (`stdio.rs:139-140`) before reading the next frame, so a returned
`write_all`/`flush` never coexists with buffered response bytes. The one window is
a **failed** `output.flush()` at `stdio.rs:140` on the exit path: the old drop
would have attempted a second flush before the handler printed, the new path exits
72 immediately. That is the same broken-pipe condition that just failed the flush,
so recovering a response there is close to impossible — but it is the only
byte-loss difference, and it is stated here rather than left implicit.

No deadline, no limit value and no existing refusal changes: every refusal that
refused before still refuses.
