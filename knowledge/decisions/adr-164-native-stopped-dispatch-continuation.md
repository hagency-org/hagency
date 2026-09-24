---
kind: decision
id: ADR-164
title: Continue an inspected stopped failure with a distinct instruction
status: Accepted
requirements: [REQ-RUST-MIGRATION-EXECUTION, REQ-LOCAL-THREE-RUNNER-QUALIFICATION, REQ-THREE-LAYER-COMPLETION]
---

The retained reference is router/src/store.ts::resolveOutcomeUnknown's continue
action: explicit operator review, a new instruction, a distinct dispatch, and
the original outcome_unknown retained. ADR148's orphan route cannot be extended
to silently ignore a dispatch_stops row. ADR162 receipts prove the original
physical owner stopped; they do not certify safety of repeating external work.

Provide separate lifecycle-scoped console routes for reading the exact original
receipt and continuing an owned_runner_failure. Bind the route's agent in the
repository operation. Require the original fence and immutable receipt digest,
operator review note, identical task/session/resource bindings, and a nonempty
instruction different from the original. Conversation cancellation and membership/route
retirement stop reasons remain outside this path. Missing proof remains a refusal, including
the earlier live failure. The receipt never grants a runner any authority.

Use one SQLite transaction for validation, the existing stop-settlement kernel,
input transfer, replacement enqueue and recovery receipt. A failed enqueue rolls
back the stop settlement. Keep original input assignments until the existing
recovery transfer moves them; the ordinary host stop path still releases them.
An exact replay is content-bound to original/fence/digest/replacement/note in the
existing dispatch_recoveries evidence. Changed content conflicts, including a
changed payload under the same replacement ID. No schema migration is needed.

The operator must review current workspace and external effects against the
historical inventory under the existing host-exclusive workspace premise. This
API does not rescan paths or claim hostile same-user filesystem isolation.
The stored digest is an explicit review reference, not a bearer credential or
a time-limited TS inspection token. This bounded slice ports continuation;
accept_completed, keep_blocked and the full TS inspection-token workflow remain
open. Task outcomes, final delivery and actual cleanup remain separate facts.

Inspection data is private and requires lifecycle scope even on GET; generic
roster reads remain unchanged. This adds API error codes but no rendered text,
so no browser translation keys or layout changes are needed.
