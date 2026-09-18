---
kind: decision
id: ADR-176
title: Share optional request pacing across native Matrix fleet clients
status: Accepted
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-LOCAL-THREE-RUNNER-QUALIFICATION]
---

The ADR175 diagnostic fleet received an explicit429 on a project request and
later exhausted the existing bounded GET retry policy during coordinator intake.
Its second provisioning effect remains uncertain; no model task was submitted.
Preserve that original effect, SDK stores and service evidence.

The host may set `matrix_request_interval_ms` in10..1000. One opaque RequestPacing
owner is carried by the cloned Matrix Limits through coordinator, approval,
provisioning, per-agent SDK, representative and media HTTP clients. It is bound
to one exact normalized endpoint. There is no credential or authority inside it.
An absent option retains the existing behavior.

Before an actual HTTP attempt, wait under the caller's original deadline and
cancellation token. A cancelled waiter consumes no future slot. All JSON methods,
each allowed GET retry, encrypted upload and authenticated download use it. Writes
remain single-attempt; a pacing timeout cannot rearm already possible custody.
Existing identity, recipient, generation, journal and completion checks remain.

This is process-local load control, not knowledge of the deployed server quota.
Other processes or clients on the same IP can still cause429. It cannot settle
the earlier unknown file room write or recreate a failed factory owner.


Verification so far: focused actual TLS3, native library45 and configured local
fleet1 pass; strict all-target native/Matrix Clippy and locked build pass. The
first pacing harness observed a later mutex reservation; its two failures remain
recorded and the corrected tests pass without changing production timing. Live
paced qualification is still in progress.
