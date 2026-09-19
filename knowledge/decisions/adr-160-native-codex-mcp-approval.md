---
kind: decision
id: ADR-160
title: Correlate native Codex MCP tool approvals before private owner custody
status: Accepted
requirements: [REQ-RUST-MIGRATION-EXECUTION, REQ-LOCAL-THREE-RUNNER-QUALIFICATION]
---

The real local Codex file task reached `mcpServer/elicitation/request`, which the
TS runner supports but the native parser did not. This request has no item ID or
start timestamp; those cannot be fabricated from its display text. Port the TS
empty-form, `codex_approval_kind=mcp_tool_call` profile and match exact server and
structural arguments to one active, unconsumed scoped `mcpToolCall` item.

Keep native wire parameters unchanged for response binding. Project a separate
host-only envelope containing current thread/turn, the correlated actual item ID,
the original native request and exact tool server/name/arguments. The existing
store digests this envelope and the host context. No reusable MCP scope is
derived, so only the existing Once/Deny owner choices apply. The four existing
task-helper autoapproval exceptions remain unchanged; file tools are not added.

Duplicate, completed, ambiguous, wrong-scope and mismatched-argument candidates
cannot be consumed. A refused recognized elicitation returns cancel before
closing the session; other unsupported RPC families retain protocol rejection.
Completion while a response is still unsent invalidates that
response. Limits cover item count and total retained arguments; overflow is an
explicit failure, not a truncated approval description or permissive fallback.

Reference: `router/src/runner.ts`, `tests/router-codex-mcp-approval.test.js`, the
installed Codex 0.154.0 generated schema, and the official
[App Server elicitation protocol](https://learn.chatgpt.com/docs/app-server).
This fixes a path-specific parity gap; it is not full local runner/soak acceptance.

Startup additionally accepts `approval_owner_wait_ms`, defaulting to the prior
1000 ms for existing profiles. Explicit waits must fit the original operation
budget together with the response reserve; no callback renews that deadline.
The current 30-second operation ceiling is still not TS long-running-task parity.
