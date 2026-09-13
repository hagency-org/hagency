spec: task
name: "Send the private approval card with its fail-closed denial and redacted notice"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, matrix, approval, notice, fail-closed]
---

## Intent

Bind PC-C1 under the operator decisions in force (D-PC-FC deny on a failed
private send; D-PC-C5 permanent-uncertain re-issue): the private card send's
two halves — the fail-closed **denial leg** (a failed send denies the pending
request with a named reason, recorded where PC-C2's observation and PC-C3's
tools read it) and the **redacted public status notice** (`PublicFrozen`, the
content-free status word in the project room). This slice rides PC-C0's
wiring — the pump, the collector and the enrollment are C0's; this is what
the wiring sends and what happens when it cannot.

## Constraints

### Must
- On a send that does not reach `Accepted` (transport failure, refusal, timeout), drive the existing `owner_approvals` state machine to a recorded denial — `state='decided'`, `choice='deny'` — carrying a named denial reason that says the send failed.
- Record the denial in the same `owner_approvals` row every other surface serves, so PC-C2's observation read and PC-C3's tools report one fact, not two.
- Post the status notice through `PublicFrozen`, its own validator: exact status packet kind (`len()==3`, `msgtype == "com.agentchat.approval.status.v1"`, `kind=="status"`, `version==1`, `state=="waiting_for_owner"`, `body ≤ 512`), destination re-derivation from the live rows by the same `room_authority` path `Frozen` uses, agent/project equality, absence of request material (no request id, digest, tool name, preview, or scope key), room distinctness, identifier checks.
- Make the failure-denial's own failure loud and honest: if the denial write fails, the request stays `pending` and the failure is visible — one uncertainty, never a false `decided`.

### Must Not
- Do not retry a failed send, silently or otherwise (D-PC-C5, permanent-uncertain); do not reconstruct a packet from parts to make a retry possible (ADR-110's no-reconstruction rule).
- Do not change `Frozen::validate`'s exact key set and action list — byte-for-byte (`state.rs:104-124`); do not change `MAX_CARD` 48 KiB; do not change `Accepted`'s meaning (the send event id durably retained, never owner approval, `adr-112:28`); do not change `check_private_approval_card`'s same-target and complete-content refusal (`card.rs:128-150`).
- Do not let the notice create a grant, an authority, or any actionable content — no owner mxid, no room id beyond the derived destination, no tool name, no preview, no request material in any byte.
- Do not touch the observation routes (PC-C2), the MCP catalog or its approval tools (PC-C3), or the C0 wiring itself.
- Do not gate any scenario by OS or feature in the binding set.

## Boundaries

### Allowed Changes
- native/hagency-matrix/src/approval_delivery.rs
- native/hagency-matrix/src/approval_delivery/state.rs
- native/hagency-matrix/src/approval_delivery/public.rs
- native/hagency-matrix/tests/
- native/hagency-store/src/domain/approvals.rs
- native/hagency-store/src/domain/approvals/
- native/hagency-store/tests/
- specs/task-rust-private-approval-send.spec.md
- knowledge/decisions/adr-137-private-approval-send-fail-closed.md
- knowledge/decisions/adr-003-two-channel-ui-approval.md
- knowledge/decisions/adr-110-native-private-approval-card.md
- knowledge/decisions/adr-112-native-private-approval-delivery.md
- docs/progress.md

### Forbidden
- Live homeservers, credentials, deployed state.
- native/hagency/src/console/** (PC-C2's observation routes); native/hagency/src/mcp/** (PC-C3's tools); native/hagency/src/bootstrap/** (PC-C0's wiring); the migration chain.

## Acceptance Criteria

Scenario: The public notice is redacted and non-actionable
  Test: native_private_approval_public_status_notice
  Level: integration
  Test Double: the real TLS fake peer with the approval bot's enrollment and a seeded pending approval
  Given a pending approval whose project room is the re-derived destination
  When send_private_approval_notice posts the status body through PublicFrozen
  Then the notice carries exactly the status packet keys with no request material in any byte
  And a notice addressed from stale or caller-influenced state is refused by the destination re-derivation
  And reading the notice confers no grant and no authority

Scenario: A private send that fails denies the pending request
  Test: native_private_approval_private_failure_denies_pending
  Level: integration
  Test Double: the fake peer refusing the send; a real domain writer
  Given a pending approval whose private card send does not reach Accepted
  When the failure is observed by the pump
  Then the approval is denied with state decided and choice deny and a named reason saying the send failed
  And the denial lands in the owner_approvals row the observation read and the MCP tools serve
  And no retry is attempted and no packet is reconstructed
  And a denial write that itself fails leaves the request pending and the failure visible

## Decisions

**Dependency: PC-C0 lands first.** The send leg rides C0's collector, pump
and enrollment — C1 has no standalone surface until they exist; the
dependency is structural (the source of the send), not merely the lane order.

**Two selectors, both the backlog's.** The row names exactly
`native_private_approval_public_status_notice` and
`native_private_approval_private_failure_denies_pending`; plan v6's own
PC-C1 scenario pair (`native_private_approval_notice_is_redacted`,
`native_private_approval_failed_send_is_denied`) is the same two behaviours
under working names, and this spec binds the backlog's — the row is
authoritative. **No third selector is added**: v6's third approval scenario
(`native_mcp_approval_failed_delivery_observes_the_denial`) is PC-C3's and
**stays deferred until this denial leg exists** — the PC-C3 spec records
that deferral and this landing is its release condition.

**Cross-reference lines only** for ADR-110 and ADR-112 (the correction's
constraint): their card boundary, `Accepted` semantics and custody rules are
relied on as written; the amendments live in ADR-137 and ADR-003.

## Out of Scope

The PC-C0 wiring, the PC-C2 observation routes and list read, the PC-C3
tools and their deferred denial-observation selector, the re-issue question
(D-PC-C5 decided permanent-uncertain; no re-issue slice exists), and any
owner-client rendering of the notice.
