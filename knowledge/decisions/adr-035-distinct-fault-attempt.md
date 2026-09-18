---
kind: decision
id: ADR-035
title: Preserve consumed fault attempts with explicit new recovery lineage
status: Accepted
tags: [herdr, recovery, evidence]
---

## Context and decision

Q3's observer claimed interrupt03 but never reached a fault window before its
parent/watch expired. The old single-attempt directory remains consumed. The
operator authorized completing the remaining goal, which requires a distinct
explicit recovery attempt without removing old guards or rewriting failures.

Use a pinned readonly recovery contract and predecessor pins to create a fresh
UUID namespace. Preserve the exact fixed Python observer and its single-use
stage guard. A no-replace hard link brings the current trace inode under the
new control root so existing trace-under-control validation continues to work;
copying a stale trace or using a prohibited symlink is not an alternative.

Preparation does not signal anything or prove current readiness. Fresh process,
goal/turn/protected facts and observer readiness remain mandatory before a
middle-owned stage release. The original goal/checkpoints and all original
fault/monitor/receipt/Matrix acceptance conditions remain intact.
