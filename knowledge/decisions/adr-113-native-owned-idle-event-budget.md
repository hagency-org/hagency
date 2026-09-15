---
kind: decision
id: ADR-113
title: Separate acknowledged turn silence from pending RPC response time
status: Accepted
requirements: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREAD-SCOPED-SESSIONS]
---

## Context

The owned Host assigned both write_timeout_ms and event_wait_ms from response_ms.
The latter is limited to2seconds. A successfully acknowledged turn may run a
tool, perform cryptographic/file IO or compute an answer without sending another
app-server event during that interval. The lower-level transport independently
tracks outstanding RPC deadlines and its fixed connection lifetime.

Original hosted492bc59 Windows workspace execution completed with one failing
file-service test. Its actual retained observation is update/transport/timeout,
zero pending RPC/server requests and upload=true. This identifies a notification
wait failure but by itself does not distinguish its deadline from the connection
lifetime. An independent actual quiet-child regression establishes the short-wait
mapping defect before the production change. The original hosted failure remains
until the unchanged real file workflow is requalified on Windows.

## Decision

Only the owned Host maps event_wait_ms to its existing operation_ms. The original
operation has an absolute monotonic deadline, and the transport has its original
fixed lifetime; event reads and control pumping cannot extend either. RPC replies
and writes retain response_ms. Low-level framing, capacity, pending request
deadlines, cancellation, authority renewal and process custody remain unchanged.
No synthetic keepalive represents tool progress.

The native CI job receives a40-minute overall bound. The observed original job
used nearly25minutes for compilation and all tests, then its post-failure media
compile was canceled by the25-minute job limit. The extra CI budget permits
original diagnostics and release work to finish, without changing a test timeout,
removing a failure or converting a diagnostic retry into original success.

## Consequences

A valid quiet turn can use the already authorized operation budget. A stalled
operation still terminates at that same absolute bound. Local actual child tests
and real file-service fixtures do not qualify hosted Windows or real-model timing.

## Alternatives Considered

Raising the short RPC deadline would conceal a different boundary. Fake progress
messages would mask the defect, while removing all deadlines would allow an
unbounded operation. Neither is used.
