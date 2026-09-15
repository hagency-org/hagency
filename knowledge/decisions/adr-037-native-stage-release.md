---
kind: decision
id: ADR-037
title: Coordinate one prearmed fault stage through ordered native controls
status: Accepted
tags: [herdr, recovery, control]
---

Q3 failed before03 release because hand-written control scripts misread argv,
captured request fences too late and admitted missing proof. The reviewed
native controller now owns exact native RPC and process correlation. Compose
it with prepared-attempt lineage, exact observer metadata and protected-file
checks instead of generating another script in a timed live window.

The stage coordinator claims one activation per prepared attempt and stage,
writes durable intent, exclusively publishes its release and calls each
identity-bound child operation once.04 resumes the original loop first and
waits for any natural audit to settle before resuming the original goal.
Unknown postrelease outcomes remain consumed and never trigger rollback,
loop recreation, retries or direct fault signals.

This is an execution-order tool, not the final E2E auditor. Named pinned04
prerequisite records must already have their real traces/experiments accepted
before the plan is constructed and authorized. Their hashes protect reviewed
inputs; they do not prove natural execution or erase failed attempts. Every
original interruption/restart/watch/approval/receipt/Matrix gate remains.

The first live stage validation exposed a mismatch with the frozen Python
observer: macOS instance paths may legitimately retain the system `/tmp`
alias in the actual backend argv. Stage validation permits that instance-only
spelling after verifying the root-owned direct alias to `/private/tmp` and a
canonical descendant with no additional symlink. It checks the alias identity
before and after resolution and repeats validation before controls. It preserves
the original instance and argument bytes; other input paths remain canonical.
These separate filesystem checks do not claim atomicity against a concurrent
writer. Existing frozen bundles and consumed observer attempts are not rewritten.
