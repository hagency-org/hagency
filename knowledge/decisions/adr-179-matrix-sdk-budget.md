---
kind: decision
id: ADR-179
title: Select the original Matrix SDK budget before startup
status: Accepted
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-MATRIX-DM-PRIVACY]
---

Root7's original approval enrollment cancelled20s after entering its SDK job
under1s shared HTTP pacing. The service exited1 before any runner task. Preserve
the original failed account, SDK, request and receipts; this is not a successful
fleet or a reason to replay an incomplete enrollment.

Expose optional `matrix_sdk_timeout_ms` in the private native host configuration.
It sets the existing `Limits.sdk` before constructing any owner; default20s and
the existing10..60000ms limits remain. Cloned limits carry the same SDK budget and
shared request cadence to the approval collector, coordinator and factory agents.
HTTP per-request budgets and Codex initialization/operation/approval waits remain
unchanged. No active operation can request a deadline extension. This is an
explicit future-startup budget, not an automatic timeout retry or custody repair.

The actual configured local-TLS fixture must retain the negative default-budget
case and prove fresh paced enrollment with a separate60s original budget.
No UI text or translation keys change. Live success remains an evidence gate.

Validation: the original20s paced attempt reproduces the live cancellation with
no tasks; its separate60s attempt completes all five original enrollment writes
and closes successfully (53.11s for both cases). Configuration bounds2, native
library46, CLI8 and existing configured fleet5 pass. Bootstrap19/20 passed in a
concurrent run; the plaintext-refusal roundtrip hit its original command-approval
timeout under load, then passed in isolation without an assertion/budget change.
This is retained as an intermittent test limit, not a timeout fix. Strict Clippy,
locked browser-feature build, production caller audit and diff check pass.
