---
kind: decision
id: ADR-174
title: Bounded retries for explicitly rate-limited Matrix observations
status: Accepted
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-LOCAL-THREE-RUNNER-QUALIFICATION]
---

The new local factory service and the prior successful Codex service both received
Remote(429) at21:33:15 UTC. Their continuous drivers stopped and transports were
fenced unavailable. Preserve both original failures. Their exact failing endpoint
was not retained; a later direct sync GET succeeded and does not identify it.

The low-level JSON GET path may now repeat only after a complete429 response.
Use at most four attempts under the original request deadline and existing body,
header, TLS and cancellation bounds. Honor valid integer Retry-After seconds and
retry_after_ms with the larger value, a10ms minimum, and exponential backoff.
With neither hint, start at1second. Invalid/unrepresentable hints or a delay that
cannot fit the remaining budget return the original429 without an early retry.
A final failure still follows the existing conservative fencing path.

No transport error, malformed200, authentication error or other status retries.
POST, PUT and binary transfers remain single-attempt; possible external effects
are never reconstructed from a status or resent. Fresh200 still passes the normal
identity/membership/session validation before any authority or scheduling.
Retired live transports need explicit new generation provisioning, as before.

Offline actual TLS tests distinguish delayed successful reads, both retry hints,
missing/malformed hints, count/deadline/cancel bounds and no mutation/uncertain
transport retry. These are not live service recovery evidence by themselves.

## Verification

TLS3 pass within Matrix library193. With transport9/token-provision9/upload7/
download6, all224 Matrix tests pass. Strict all-target native/Matrix/execution
Clippy and locked build pass. Explicit fresh-generation recovery restores both
isolated services with their original SDK stores; root3 then passes actual
two-agent/two-round private-DM execution in Robrix. Earlier429 failures remain
recorded. This is recovery after the fix, not a controlled live429 injection or
a claim that every server quota can fit the bounded retry interval.
