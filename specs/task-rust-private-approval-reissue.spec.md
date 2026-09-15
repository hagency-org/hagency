spec: task
name: "Record the undeliverable card re-issue as permanent-uncertain"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, approval, reissue, fail-closed]
---

## Intent

Bind PC-C5 under D-PC-C5 in force (permanent-uncertain): an undeliverable
private card is never silently re-sent under the same request id, no fresh
card is minted by any in-product path, and the old request's fate is
**permanent-uncertain** — the stored row reads `state='decided'` with
`choice='deny'` (PC-C1's word), and "permanent-uncertain" is the **fate the
operator is told** (no re-issue, terminal for that request id), not the
stored state word. The fate is read from the row plus the denial-reason
receipt row PC-C1's denial-reason migration (currently 032) defines. A future visible re-issue is a
**new** request id linking the old one; it does not land here.

## Constraints

### Must
- Leave the store row terminal for an undeliverable request id — `state='decided'` with `choice='deny'`, the word PC-C1 writes — and state "permanent-uncertain" as the fate: no code path transitions it to a fresh card, re-queues the send, or re-sends the same packet. The stored `state=uncertain` word is the `applying → uncertain` recovery sweep's, never a failed send's.
- Read the fate from the store-side read alone — the existing `ApprovalSummary` (`hagency-core/src/approvals.rs:97-102`, carrying `state`/`choice`) plus the kind-deny receipt row PC-C1 mints (`approval_verdict_receipts.denial_reason`, joined by `request_id`) — so PC-C2's status projection renders one fact when it reads the same row. One read helper was added for exactly this, store-side only: a synchronous `DomainRepository::delivery_denial_reason(id)` returning the receipt row's `denial_reason` for the request (`None` when no delivery-failure receipt exists) — no async `DomainStore` wrapper, no new table, no second projection path.
- State the linkage rule for any future re-issue: a new request id under the same context, whose record links the old id and marks the old one permanent-uncertain — recorded here so a later slice cannot silently mint a retry.

### Must Not
- Do not re-send the same packet, rebuild the card from parts (ADR-110's no-reconstruction rule), or transition the old row to `invalidated` — `invalidated` stays an unused CHECK word until a visible re-issue follow-on lands.
- Do not change `check_private_approval_card`'s same-target and complete-content refusal, the rule that a packet read never confers send authority, or the existing `state` CHECK (`013:34`).
- Do not touch the observation routes (PC-C2's own), the MCP tools (PC-C3's own), or the send leg (PC-C1's own) — this slice only records the re-issue decision.
- Do not gate any scenario by OS or feature in the binding set.

## Boundaries

### Allowed Changes
- native/hagency-store/src/domain/approvals.rs
- native/hagency-store/src/domain/approvals/
- native/hagency-store/tests/
- specs/task-rust-private-approval-reissue.spec.md
- knowledge/decisions/adr-110-native-private-approval-card.md
- docs/progress.md

### Forbidden
- Live services, credentials, deployed state and Matrix server changes.
- native/hagency-matrix/** (PC-C0/C1's delivery); native/hagency/src/console/** (PC-C2's observation); native/hagency/src/mcp/** (PC-C3's tools); migrations other than PC-C1's already-landed denial-reason migration (currently 032).

## Acceptance Criteria

Scenario: An undeliverable card is permanently visible as uncertain, never silently re-sent
  Test: native_private_approval_lost_send_is_permanently_visible_as_uncertain
  Level: integration
  Test Double: a fixture approval whose delivery failed and whose denial receipt was minted by PC-C1's leg
  Given an undeliverable approval whose send failed and was denied
  When the fate is read through the store's ApprovalSummary and the denial receipt row
  Then the row reads state decided with choice deny and the denial reason naming the failed send — the permanent-uncertain fate is what the operator is told, not the stored word
  And no path mints a fresh card, re-queues the send, or re-sends the packet
  And the same row and receipt are the only source — one fact across every surface

## Decisions

**The binding selector is the row's permanent-uncertain name.** D-PC-C5 is
decided permanent-uncertain, so `native_private_approval_lost_send_is_
permanently_visible_as_uncertain` binds here; the backlog's alternative name
(`native_private_approval_undeliverable_reissue_uses_new_request_id`)
describes the rejected (a) arm and binds nowhere until a visible re-issue
follow-on lands.

**Depends on PC-C1.** This slice reads the kind-deny receipt row PC-C1's denial-reason migration (currently 032) defines (`approval_verdict_receipts.denial_reason`); nothing
here is implementable until that row and the send-failure denial exist. No
new migration is added — `uncertain` is already a CHECK word and
`invalidated` stays unwritten.

## Out of Scope

A visible re-issue under a new request id (the rejected (a) arm, a future
follow-on), the PC-C2 observation routes and PC-C3 tools, the PC-C1 send
leg, and any console re-request control.
