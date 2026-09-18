---
kind: decision
id: ADR-161
title: Carry the retained execution budget through native host custody
status: Accepted
requirements: [REQ-RUST-MIGRATION-EXECUTION, REQ-LOCAL-THREE-RUNNER-QUALIFICATION]
---

The retained `router/src/runner.ts` gives both Claude and Codex an explicit
execution timeout defaulting to twenty minutes. Native operation admission and
owned approval-context binding instead capped execution at thirty seconds, and
the application claimed a fixed sixty-second capability. The real local file
task reached its MCP approval around twenty-two seconds after intake, exposing a
usable owner wait versus operation lifetime mismatch even after ADR160's parser
fix. Raising just one timeout would leave another early failure.

Explicit native configuration may now choose up to twenty minutes. This changes
admission ceilings, not existing configured values or running deadlines. One
shared core constant owns the operation maximum. The existing native transport
already supports twenty minutes. RPC/write waits, cancellation checks, writer
receipt waits and SQLite bounds remain unchanged. The operation retains its one
absolute monotonic deadline captured before preparation and spawn.

This qualifies the ordinary configured local Codex path first. Pre-activation
warm initialization retains its separate thirty-second ceiling explicitly; it
must not inherit a longer startup allowance merely by sharing `Limits`. The
configured factory service's distinct thirty-second admission remains outside
this slice. Local provider-owned Codex does not use that factory/warm path.

The compatible host claim receives `max(60 seconds, operation + 30 seconds)` of
capability lifetime, at most twenty minutes thirty seconds. Generic claims stay
at five minutes. The initial lease remains sixty seconds, then ordinary and
parked authority checks renew only five seconds at a time, capped by the original
capability expiry. A longer capability does not bypass revocation, current route,
task epoch, account or workspace checks. It does not keep a lost lease current.

Owned approval contexts may cover that original operation, once per dispatch
fence. Individual owner waits plus response reserve may use at most the existing
ten-minute durable request ceiling and must fit the configured operation. Each
callback remains capped by the original remaining operation lifetime; a late
request never restarts its clock. Startup's one-second owner wait remains the
default; the operator may explicitly configure a useful longer wait.

This supersedes ADR053's initial thirty-second execution ceiling and sixty-second
combined join allowance. The conservative allowance is now the configured
operation budget plus thirty seconds of existing startup/stop/drop and receipt
waits, at most twenty minutes thirty seconds. Cancellation still interrupts the
original worker; the allowance is not a mandatory shutdown delay or a hard kernel
syscall bound. No process or workspace ownership is dropped at a wrapper timeout.

Bound coverage and actual offline subprocess execution are not live long-task or
soak acceptance. The original failed file attempt, stop fence and retired session
route remain preserved; this decision grants no recovery, retry or release.

## Verification

Offline runtime95 and execution lib/owned80 pass; store approval/claim44,
startup owner-wait1, final limit1 and warm-runtime6 pass. The actual subprocess
quiet turn lasts31 seconds and settles with original full-stop evidence. A
separate twenty-minute configured operation cancels promptly, and a60-second
owner window traverses the original MCP approval/response custody. Store clock
fixtures check the complete long capability interval without a live wait,
including expiry, revocation and unrenewed lease refusal. These are separately
identified deterministic clock tests, not twenty minutes of live model time.
Strict all-target Clippy passes. agent-spec/task-writer are unavailable, so
these are test records, not lifecycle success. Live qualification remains open.
