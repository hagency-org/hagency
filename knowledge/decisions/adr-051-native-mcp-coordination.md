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
