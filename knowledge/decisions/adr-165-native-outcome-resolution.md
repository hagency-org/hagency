---
kind: decision
id: ADR-165
title: Resolve inspected stopped outcomes with expiring operator credentials
status: Accepted
requirements: [REQ-RUST-MIGRATION-EXECUTION, REQ-LOCAL-THREE-RUNNER-QUALIFICATION, REQ-THREE-LAYER-COMPLETION]
---

Port router/src/store.ts beginOutcomeInspection / resolveOutcomeUnknown through
separate lifecycle-scoped native routes. The operator obtains a private 15-minute
inspection credential (configurable 1..60 minutes), reviews the retained inventory
and actual workspace/external effects, then chooses continue, accept_completed or
keep_blocked. An inspection credential proves review scope, not physical cleanup;
the exact original ADR162 host receipt remains mandatory. Older failures without
that receipt remain unresolved.

Schema 35 stores hashed inspection credentials and content-bound resolution
receipts. A token binds the original fence/receipt, task state and current owned
scope. The writer revalidates current authority, retained leases, dirty workspace,
other owners and pending media effects. Resolution is one transaction: token
consumption, existing stop settlement, task update or replacement enqueue, and
request receipt. Exact replay is checked before token expiry and later state;
changing any request content conflicts. Plaintext secrets are neither persisted
nor returned by reads. Issuance is bounded and expired unused rows are reclaimed.

accept_completed updates the canonical task exactly once, without manufacturing
accepted runner output, graph result or Matrix delivery. keep_blocked records the
operator's decision without scheduling work. Original authenticated inputs stay
assigned and unprocessed for both terminal decisions. Older queued instructions
are superseded. Graph terminal actions refuse until their independent graph
resolution contract exists; continuation reuses the graph-aware recovery kernel.

ADR164's explicit receipt-bound continuation remains supported. The new workflow
adds expiring bearer credentials without pretending the historical inventory read
or the compatibility API issued one. No browser text or layout is changed, so no
translation keys are introduced. Live qualification still requires actual runners
and Matrix evidence; these offline gates establish only the operator state machine.
