---
kind: decision
id: ADR-155
title: Claude stream session retains native IO and process ownership
status: Accepted
requirements: [REQ-RUST-MIGRATION-EXECUTION, REQ-LOCAL-THREE-RUNNER-QUALIFICATION]
---

## Decision

Extend ADR154 with a native, single-prompt session driver and an owned-process
wrapper. The existing platform guardian owns the child; the driver owns all
three pipes without spawned readers. Each polled operation has a cancellation
guard. Failure or future drop closes the stream driver permanently and the owned
wrapper requests bounded stop through its original guardian. An unpolled future
has done nothing and does not independently stop the owner.

Initialization accepts only its exact request-tagged success. A submitted prompt
is only flushed bytes, not execution acknowledgement. The first system/init
event establishes this disposable stream's upstream session ID. All following
events must match it; duplicate init and unsolicited control replies refuse.
This binding is an upstream observation, not a Hagency task or account binding.
Permission requests/cancellations remain untrusted observations without a reply
API. Production Host continues to refuse Claude until original private approval,
scoped task MCP, usage, account and lifecycle gates are actually integrated.

Use one MiB codec frames, at most sixteen queued messages and two MiB queued
wire bytes, sixteen KiB private stderr tail, finite write/event/lifetime timers
and the codec's ten-second partial-frame deadline. Pump all streams while writes
are blocked. Preserve accepted/total byte counts on failure; neither zero nor
full counts provide a retry grant. All diagnostic errors contain fixed labels.

A result transitions to ResultObserved, not Done, stopped or reusable. It does
not stop the owned process automatically: Claude may still have background
activity. The host must reconcile the task and explicitly stop the owner. No
second prompt is admitted. Platform cleanup is returned verbatim; macOS's
unproven whole-tree observation remains false. No live-model test is added.

## Evidence boundaries

Offline memory peers establish protocol/IO invariants. Offline native child
fixtures establish retained pipe/guardian integration on the tested platform.
Neither is real Claude execution, effective sandbox, approval, authenticated
usage, Matrix delivery or three-runner soak evidence. ADR154's upstream source
references define the wire shapes; installed-CLI compatibility remains a separate
operator diagnostic and eventually a complete native-Host qualification.
