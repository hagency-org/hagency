---
kind: decision
id: ADR-137
title: "The private approval card send and its fail-closed denial"
status: Proposed
requirements: [REQ-RUST-MIGRATION-EXECUTION]
tags: [approval, delivery, notice, fail-closed, privacy]
---

## Context

PC-C0 wires the private-approval-card delivery path (the pump, the collector,
the enrollment); the private-card plan v6's PC-C1 section owns what the wiring
is *for*: the card send's two halves — the redacted public status notice that
accompanies a pending request, and the fail-closed outcome when the private
send itself fails. Two operator decisions are in force:
**D-PC-FC — deny on a failed private send** (a failed send denies the request
rather than leaving it pending) and **D-PC-C5 — permanent-uncertain re-issue**
(a re-issue is not attempted; the undeliverable request is surfaced, not
silently retried). This ADR creates the record the backlog allocated
(`new-137`) and amends ADR-003's scope note; ADR-110 and ADR-112 are
cross-referenced only.

## Decision

**The private send fails closed, with a named reason.** When the pump's
`send_private_approval_card` does not reach its `Accepted` outcome —
transport failure, refusal, timeout — the approval is **denied**: a recorded
transition in the existing `owner_approvals` state machine (`state='decided'`,
`choice='deny'` — the deserialized values; the stored `choice` text is the
JSON-encoded `"deny"`), carrying a **named denial reason** that says the send
failed. **Under D-PC-FC a failed private send IS a deny verdict**, so the
denial mints a receipt row of **kind deny** in `approval_verdict_receipts` —
`(source_key, digest, request_id, denial_reason)` — through a **new public
store wrapper, `deny_for_failed_delivery`** on `DomainRepository` with its
`DomainStore` half, an entry point **distinct from the owner-verdict path**
(`decide_verdict` is private and takes an owner's
`OwnerVerdictObservation`, which a delivery failure is not), **at most once
per request**: a second call is idempotent on the receipt and differing
content is refused. **The reason has a storage home**: migration **032**
adds one nullable `denial_reason TEXT` to `approval_verdict_receipts` via
`ADD COLUMN` (**032 is provisional, landing order** — base head 31 + 1,
after engagements retention 030 and execution retention 031 landed; it
renumbers to 033 if MA-S2 lands first). The schema-head pin moves to 32
**in the tests that pin it**, in this slice's own commit — every
`assert_eq!(… user_version …)` site moves, every `pragma_update` rewind
stays. The request never sits `pending` after a failed send, and the denial
is recorded **where PC-C2's observation read and PC-C3's tools read it** —
the same `owner_approvals` row and the same kind-deny receipt row every
other surface serves, so the operator and the runner see one fact, not two:
**the delivery-status word the operator sees is the row's own
`state`/`choice`** (after a denial: `decided`/`deny`), not the stage enum's
intermediate word — the stage stays a transport fact, the row stays the
decision fact. Two things the denial never does: it **never retries
silently** (no re-send, no re-queue — D-PC-C5's permanent-uncertain posture
applies to the delivery, so the send is not re-attempted behind anyone's
back), and it **never reconstructs a packet** (the frozen card bytes are
validated, never rebuilt from parts to make a retry possible). The denial's
own failure is the retained caveat, stated here as in the code: if the
denial write itself fails, the request stays `pending` and the failure is
loud — one honest uncertainty, never a false `decided`.

**The public status notice is redacted and non-actionable.** The same
collector gains `send_private_approval_notice`, posting the status body
(`agent`, `project`, `state: "waiting_for_owner"`, `body ≤ 512`) to the
project room through **`PublicFrozen`**, its own validator — a sibling of
`Frozen`, never a widening of it. `PublicFrozen` requires the exact status
packet kind, **destination re-derivation** from the live rows by the same
`room_authority` path `Frozen` uses, agent/project equality, the **absence
of request material** (no request id, digest, tool name, preview, or scope),
room distinctness, and identifier checks. The notice creates **no grant and
no authority**: reading it changes nothing. `Frozen::validate` stays
byte-for-byte unchanged; `MAX_CARD` stays; `Accepted` keeps its meaning
(the send event id durably retained — never owner approval).

**ADR-003's scope note, amended.** ADR-003 promises the encrypted DM as the
approval channel and already records its one test-only exception. This
amendment names the public status notice as the **second** thing that leaves
the approval path — and states what it is not: it is not the request, not
actionable, and carries none of the request's material; it is a one-way
status word in the project room. The encrypted DM remains the only channel
that carries request content.

**Cross-references.** ADR-110's card boundary and no-packet-reconstruction
rule govern the send's content; ADR-112's delivery custody (`Accepted`'s
meaning, the stage machine) governs its outcome. Neither is changed by this
ADR; both are relied on as written.

## Consequences

Good, because a failed send has one visible terminal answer (denied, with a
reason, on every surface), the notice is provably content-free by its
validator, and no silent retry path exists to audit.
Bad, because a transient transport failure denies a request the operator
might have wanted retried — deliberate: the alternative (retry) is the
unbounded side-effect loop D-PC-C5 refuses, and the operator can re-request
through the normal path.

## Alternatives Considered

- Leave the request `pending` on a failed send — rejected by D-PC-FC in
  force: pending-after-failure is the ambiguous state every consumer must
  then guess about.
- Retry the send silently — rejected (D-PC-C5, permanent-uncertain): a
  retry loop on a side-effecting send is exactly what the custody model
  forbids.
- Reconstruct the packet for a retry — rejected: ADR-110's
  no-reconstruction rule; the card is validated as sent, never rebuilt.
