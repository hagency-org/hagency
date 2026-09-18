---
kind: decision
id: ADR-162
title: Retain exact original stopped-owner workspace evidence before shutdown
status: Accepted
requirements: [REQ-RUST-MIGRATION-EXECUTION, REQ-LOCAL-THREE-RUNNER-QUALIFICATION, REQ-THREE-LAYER-COMPLETION]
---

The local Codex file failure exposed a missing recovery prerequisite: its real
whole-tree stop observation survived only in the execution Report. Shutdown
dropped that original owner; the remaining stop row cannot prove physical cleanup
or workspace inspection. Neither the generic orphan recovery endpoint nor a
formatted status record may invent the missing proof.

The original owned worker now records a bounded content inventory only after
actual full stop and an acknowledged negative fence. It uses the original
Started scope and retained root, not a reopened configured path as authority.
No-follow traversal, finite limits and two matching passes detect inconsistent
observations under the existing host-exclusive stable-path premise. Symlinks are
recorded but never followed. This does not claim hostile same-user isolation or
semantic safety of repeating effects outside the workspace.

Schema34 stores one immutable, host-only stop inspection per dispatch/fence.
The original capability and private Started marker authenticate the historical
attempt, including after its execution authority expired. Exact replay checks
the original committed digest; conflicting evidence refuses. Runtime output,
public status JSON and caller edits to a returned Report cannot mint a receipt.
An unknown recording response retains the same pending evidence for explicit
retry, not a fresh scan or new execution.

The receipt is evidence for a later explicit operator recovery flow. Recording
it never settles the stop or frees any lease, task input or quarantine; it does
not make G8 or full recovery complete. Existing failure and completion outcomes
remain unchanged. Old attempts without this original observation remain without
proof; no live database is patched to manufacture one.

## Verification

Actual offline owned subprocess EOF records a content-hash receipt, preserved on
reopen, while its original task remains InProgress and its lease/dirty/quarantine
remain held. Missing-process and oversized-workspace cases cannot mint a receipt
through mutated Report fields. Store tests cover pre-Started/foreign capability
refusal, conflicting content, historical replay, and real writer reply loss both
before and after commit. Filesystem tests cover exact hashes, outside symlinks,
unsupported sockets, root replacement and entry/byte/deadline bounds. Schema33
upgrade preserves existing resource configuration and does not create evidence
for old attempts. Full store352/execution83 pass; strict all-target Clippy passes.
This verifies evidence custody, not stopped-task resume or complete E2E.

ADR163 consumes the exact receipt only to distinguish stopped physical occupancy
from an unknown physical owner at claim time. It does not consume or alter the
receipt, settle the stop, clear the lease/quarantine or authorize a retry.
