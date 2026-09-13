---
kind: decision
id: ADR-138
title: "Bounded native approval observation"
status: Proposed
requirements: [REQ-RUST-MIGRATION-EXECUTION]
tags: [console, approvals, read-only, privacy, observation]
---

## Context

The private-card plan (v5, PC-C2) found that native has **no operator surface for
approvals at all**: every `DomainStore` approval wrapper takes one id
(`observe_approval_room`, `bind_approval_context`, `request_owner_approval`,
`consume_owner_approval`, `observe_approval_application`, `approval_summary(id)`,
`private_approval(id)` — `domain_worker.rs:1513-1585`), so a **list read does not
exist**; and the console mounts no approvals route (`console.rs:62-65`). The
retained product's `publicRecord`/`matrixRecord` projections
(`backend-v2.js:105-135` area) serve owner identity (`owner_mxid`,
`owner_dm_room_id`) and tool detail (`tool_name`, `description`,
`input_preview`) — data ADR-110's card boundary and ADR-112's delivery custody
deliberately keep out of any projection that is not the owner's own DM. The
plan's answer (its A5 correction) splits the slice: **C2a** the missing store
read, **C2b** two observation routes and a page section — read-only, on the
console authority ADR-107 establishes and inside the observation posture
ADR-108's amendments already use for `engagements` and `resources`.

## Decision

**C2a — the list read, first.** A new bounded, `SELECT`-named read on the store:
`after`/`limit` with a hard cap (1..=100), mirroring `engagements`
(`domain.rs:607-615`); its `DomainStore` wrapper beside `approval_summary`
(`domain_worker.rs:1570`); a store test. This is the slice's non-trivial work —
the reason v4's "size 2" was corrected to a split.

**C2b — two observation routes, read-only.** `GET /console/api/approvals`
(list) and `GET /console/api/approvals/{id}` (single), mounted in a new
`console/approvals.rs` beside the four existing sub-routers under the API
sub-router's `authenticate` hoop, and **no new `Scope`**: a read-only ticket
installs `Grant { scope: ReadOnly, mutation: None }` (`authority.rs:151-159`)
and can reach no mutation — the same read class as `engagements` and
`resources`. No mutation route, no verdict route, no detail route.

**`ApprovalRow` — exactly seven keys, `camelCase`, `deny_unknown_fields`**,
every one a column the store holds: `id` (`owner_approvals.id`), `state` (one
of the seven CHECK words, `013:34`), `choice` (`once|task|always|deny` or
null), `reusable_scope` (`scope_key IS NOT NULL`, `approvals.rs:100`),
`expires_at`, `engagement_id` (one join to `approval_contexts`, `:28`), and
`project_room_id` (`projects.room_id` via that join, nullable). **No nested
object**, so nothing hides in a sub-object; the client's exact-key helper
(`native-api.js:10-11`) checks set **and count**.

**The withheld sets, stated on both sides.** *Retained-only, never ported:*
`owner_mxid` (`publicRecord:105`), `owner_dm_room_id` (`:106`),
`tool_name`/`description`/`input_preview` (`matrixRecord:128-135`), and the
card bytes (`PrivateApproval.params` — ADR-110's boundary). *Native columns
deliberately withheld:* `description` (`013:33`), the `config`/`application`/
`observation` JSON (`:35-36`). *No native source — served as absent, never
invented:* `decided_at`, `created_at`, `consumed_at` (no column), and the
agent **name** (native keys by engagement). **Timestamps in particular are
served as absent**: there is no column, and a null-derived guess would be the
silent-zero failure.

**The delivery status route — deferred behind PC-C0.** `GET
/console/api/approvals/delivery` (**no `{id}`**: the status is
deployment-wide) serving `PrivateApprovalDeliveryStatus` **as-is** — five
keys, `stage, receipts, writes, accepted, retained_bytes`
(`approval_delivery.rs:50-57`; `stage` is the nine-variant
`PrivateApprovalDeliveryStage`, `:38-49`) — carries no request id, no room, no
body. It is unreachable until PC-C0 constructs the collector and `App` carries
its handle, needs `lib.rs` and `bootstrap.rs`, and lands in its **own commit**
(the spec-binding rule the PC-C2 check's V2 fixed: a selector for a nonexistent
route asserts nothing).

**The page and the console rules.** A new `approvals/page.jsx` section renders
`state` and `choice` as words; for an undelivered approval there is **no
status word in C2b** (the delivery stage is the deferred route's field) — and
never a card, never a preview: a property of the key set, since no key could
carry one. The **five-document exception stays five**
(`console.rs:336-347`); `/console/approvals` is a non-document. The hoops,
the session cap of 4 and 15-minute lifetime, and the 8-permit semaphore are
untouched, as ADR-107 fixed them.

## Consequences

Good, because approvals become observable without porting the retained
projection's owner identity or tool detail, the row cannot carry what it has
no key for, and the missing list read is built once, bounded, instead of five
id-shaped wrappers being stretched.
Bad, because the operator sees no delivery status until PC-C0 lands its
collector, and the absent timestamps read as gaps rather than zeros — honest,
but sparse.

## Alternatives Considered

- Port `publicRecord` — rejected: it serves `owner_mxid` and the owner DM room,
  exactly what ADR-110/112 keep private.
- A per-id detail route with a richer row — rejected: a second row shape for
  one more field is the drift the exact-key contract exists to prevent.
- Bind the delivery selector now with a documented refusal — rejected per the
  V2 ruling: a test asserting a refusal for a nonexistent route asserts
  nothing; the route's own commit owns its selector.
