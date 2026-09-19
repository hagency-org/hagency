---
kind: decision
id: ADR-152
title: "Protected settled outgoing receipt history with bounded live cache"
status: Accepted
requirements: [REQ-RUST-MIGRATION-EXECUTION, REQ-MATRIX-DM-PRIVACY, REQ-THREAD-SCOPED-SESSIONS]
---

## Context

ADR-059 deliberately stopped after 64 settled outgoing receipts until exact
history could be preserved. Sustained real sync now has authenticated immutable
replay history, but a continuously replying service still reaches the send cap.

## Decision

Add settled outgoing receipts as a separately namespaced record in the existing
private authenticated receipt trie. The existing journal `sync_history` field
remains its root for already-deployed state compatibility; it now owns mixed
sync/source/settled-send records. Original sync node serialization is unchanged.

The settled key binds ID/fence, preserving restore's cross-kind uniqueness;
the value retains kind and accepted complete-attempt digest. Exact lookup is
bounded by the existing node/256-branch rules and returns zero/one private
receipt, never a lifetime list. A new send cannot reuse any settled key. Final
and notice callers may report original delivery without executing or activating
anything. A historical proof is not current authority.

Replay rejects a conflicting present domain row (non-Delivered or another fence).
A fresh Claimed row reusing a retained SDK key cannot borrow its old delivery.
When canonical retention has removed the row, protected SDK history can still
report its original past acceptance without creating/reconciling a domain row.
Notice replay requires its existing Delivered row. These checks are factual, not
source-proof constructors or current-route grants; compact legacy receipts cannot
backfill unavailable original content authority.

Only settled cache entries roll. Immutable nodes precede the ordinary atomic
journal root/cache/Prepared-or-Settle commit. Failed publication restores cache
and root memory, poisons the original owner and leaves uncertainty inspect-only.
Reopen uses actual committed state; orphan nodes alone are not proof. Every
original domain/current-route/per-write check and possible-send phase remains.

Network-free retained file recovery queries each bounded original job's exact
settled key rather than relying on cache residency. It still needs the original
locator/content and already-Delivered domain acceptance; a compact receipt cannot
establish first Delivered or recreate media custody. No pending receipt is rolled.

## Consequences

The finite live cache no longer permanently exhausts after 64 replies while
earliest settled keys retain duplicate-send prevention. Immutable disk history
still needs separate physical retention/budget qualification. This does not close
approval/upload/fleet/API/recovery/sandbox or entire-port gates.
