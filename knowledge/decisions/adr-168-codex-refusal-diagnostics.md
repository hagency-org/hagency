---
kind: decision
id: ADR-168
title: Preserve fixed Codex notification refusal categories in native diagnostics
status: Accepted
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-LOCAL-THREE-RUNNER-QUALIFICATION]
---

The first repeated-run pulse after successful native file receive failed with
`unsupported_event`. The runtime retained its fixed category, but the execution
report discarded it. Carry `refused_notification` through `RuntimeObservation`
and the authenticated operator status. Only static labels from the runtime's
existing classifier cross this boundary. Unknown names remain `unknown`.

This is diagnostic evidence, not an acceptance rule or authority. Preserve the
original failed dispatch, stop receipt, workspace inventory and cleanup verdict.
It cannot recover the category already lost from the historical live report.
