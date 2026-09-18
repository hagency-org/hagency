---
kind: decision
id: ADR-175
title: Retain bounded failure diagnostics for each original factory worker
status: Accepted
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-LOCAL-THREE-RUNNER-QUALIFICATION]
---

Four live concurrent DM tasks completed on the ADR-174 isolated two-agent
factory. The following file qualification failed: agent two stopped with a
protocol failure before file creation; agent one received actual Robrix owner
approval, uploaded its bytes, and retained an uncertain encrypted room write.
The original protected journal records accepted key sharing but no accepted
room response. Neither operation is resent or represented as successful.

Extend the authenticated factory snapshot with each registered engagement's
existing bounded Status. Record fixed owned-runner and file-publication error
categories in host logs at the original failure, before later status changes.
Use one Matrix category mapper; arbitrary unsafe-snapshot text never becomes a
diagnostic field. Public readiness continues to expose only state words.

This does not recover a historical diagnostic absent from the running binary,
classify the unknown publication as accepted or rejected, or authorize any
retry. Existing owner retention and explicit recovery rules remain unchanged.


Verification: native library44 and actual configured local-Codex fleet1 pass;
its authenticated snapshot includes both original agent statuses, unauthenticated
access returns401, and public readiness remains state-only. Strict all-target
native Clippy passes. The two legacy Linux-gated fleet selectors are ignored in
this run, not counted as passing. Live diagnosis remains pending on a fresh fleet.

Handoff amendment (2026-09-17): the paced fleet exposed another diagnostic gap:
an error returned before an Operation exists was collapsed into bootstrap Worker.
Retain its fixed execution failure category in owned_failure and log the exact
dispatch ID. Do not invent protocol, cleanup or settlement observations when no
Report was received. Worker retention and all admission behavior remain unchanged.

The actual configured executable regression initializes two local synthetic
owners, waits for registered worker custody, then revokes only their disposable
provider directory permissions. Both handoffs retain lost_authority through the
authenticated API, public readiness stays state-only, and neither peer receives
thread/start or turn/start. No replacement process or success reply appears.
Waiting merely for route rows was insufficient: that first test revoked before
the second worker was registered and correctly caused startup refusal instead.
The corrected test waits for actual registered owners before injecting the fault.
Native library45/configured fleet2 pass, with two legacy selectors ignored;
strict Clippy, locked build and production caller audit pass.
