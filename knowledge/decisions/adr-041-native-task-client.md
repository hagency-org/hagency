---
kind: decision
id: ADR-041
title: "Native task maintenance uses the existing scoped runner API"
status: Accepted
tags: [rust, cli, tasks, security]
---

## Context

A native task helper needs to maintain its assigned canonical task through the existing runner API without acquiring operator or database authority.

## Decision

The native task client reads a complete runner capability, literal loopback socket
and canonical task ID from its inherited environment. It never loads operator
credentials, opens domain state, writes runner configuration or chooses another
task from a list. The server still checks exact dispatch, runner, fence, secret,
lease, Started state and task binding at writer execution.

Each maintenance invocation sends one canonical mutation and requires an explicit
call ID. A caller can retry that same ID and content after an unknown response;
the server owns the durable idempotency receipt. The client has no automatic retry,
redirect, proxy, arbitrary-DNS or TLS-to-plaintext fallback. Socket, HTTP headers,
body and total operation lifetime are finite. Lost mutation replies remain
unknown; local connection failure before sending is unavailable.

The native host starts the assigned task when it starts the dispatch. Consequently
the helper's `start` is an alias for a heartbeat of that existing task, and never
creates work or changes the assignment. `wait`, `resume` and `done` submit explicit
state transitions. Failed or inappropriate transitions remain server refusals;
the client does not manufacture a desired local state. Read and mutation responses
are checked against the context task ID before returning structured task output.

This is an executable client integration, not host environment provisioning or
full replacement of legacy persistent-home task writers. The native runner
launcher, MCP/helpers and installation workflow must provision it before cutover.

## Consequences

The helper submits exact scoped calls and leaves idempotent receipts with the server. Unknown responses remain unknown, and launch-time context provisioning remains a separate integration obligation.

## Alternatives Considered

Opening the domain database, discovering another task or automatically retrying mutations would bypass the existing scoped writer boundary. Treating helper start as task creation would change the host-owned assignment.
