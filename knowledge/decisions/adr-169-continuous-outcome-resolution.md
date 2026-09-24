---
kind: decision
id: ADR-169
title: Retain the continuous worker across an explicitly resolvable stopped outcome
status: Accepted
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-LOCAL-THREE-RUNNER-QUALIFICATION]
---

The isolated Codex soak exposed a production join missing from ADR165: its
continuation commits, but the continuous worker returned its failed report and
waited only for Close. A resolvable failure now retains the report and worker
while awaiting an explicit durable resolution. A private writer observation
binds the original capability and fence, original inspection, settled stop and
resolution/recovery receipt, and confirms that original leases are gone.

Only an original report with stopped physical custody and a recorded inspection
can wait this way. Cancellation preserves normal retained-owner close. Resolution
retires only that workspace binding before ordinary claiming resumes; current
route/account/resource checks still govern the distinct next dispatch. This is
not automatic retry, a success verdict for the old attempt, or restart recovery.

### Amended: no blocking wait for a resolution (2026-09-23, ADR-182)

"Only an original report with stopped physical custody and a recorded
inspection can wait this way" described a worker that parked; the worker now
continues after any failed attempt and the quarantined session simply yields
no work until the operator resolves it. An unproven tree additionally fences
the agent (ADR-182 decision 3). Resolution semantics are unchanged.
