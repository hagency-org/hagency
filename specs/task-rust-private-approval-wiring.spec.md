spec: task
name: "Wire the private approval delivery path under host authority"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, approvals, delivery, bootstrap]
---

## Intent

Bind PC-C0 of the private-card plan v4: wire the existing private-approval-card
delivery path into the service composition — a pump that owns the single-consumer
`ApprovalRequests` handed over by the driver's one added value parameter, drives
`ApprovalCollector::send_private_approval_card` to completion under a cancellation
token, and is invisible over HTTP (ADR-110's "no public endpoint"). The pump runs
under host/service authority, never runner scope; this is the wiring ADR-110 and
ADR-112 both mandate, and this spec is the wiring slice those ADRs' "no bootstrap
wiring" clauses defer to.

## Constraints

### Must
- Add the pump as a new module `native/hagency/src/bootstrap/approval.rs` (sibling of `driver.rs`, wired by a plain `mod approval;` in `bootstrap.rs`), taking the `ApprovalRequests` value through exactly one new `Driver::start` parameter — the one buildable handoff (Q1): the driver has no usable `&mut` window after `Box::pin(operation.wait_boxed())`, so it forwards the value and the pump owns the single-consumer receiver.
- Run the pump on the service's multi-threaded runtime — host/service scope — and terminate it by the channel's own end (`recv() == None` when the worker's `finish_notices` drops the sender); no runner capability, workspace access or driver runtime may reach the pump.
- Build all four construction deliverables: `HostApprovalConfig::new` over the approval bot's own `HostConfig`/`HostIdentity`; `with_fresh_account_enrollment(anchors)`; `ApprovalCollector::new(HostApprovalConfig, DomainStore)`; and `Host::with_approvals(ApprovalHost)` on the `Host` the operation is built from — without which `Operation::start_mode` creates no channel at all.
- Carry the approval credential in `bootstrap/config.rs`'s `Prepared` (a second credential set: the pooled ordinary `HostConfig` is neither `Clone` nor admissible — `Collector::new` refuses `approval == true`).
- Re-read each card from the admitted domain request; never trust caller-supplied Matrix path or content.
- Expose the send outcome (the fail-closed policy is D-PC-FC and a later slice; C0 wires, it does not decide the denial).

### Must Not
- Do not add any route, console surface or MCP tool; no HTTP visibility.
- Do not merge the two collector purposes: `Collector::new`'s refusal of an approval config (`collector.rs:58-60`) stays.
- Do not change `Frozen::validate` (byte-for-byte), the `#[cfg(test)]` fault-seam gating, `ApprovalHost`'s capacity semantics (`capacity.rs:16-47`), `card.rs`'s `MAX_CARD` 48 KiB (`hagency-store/src/domain/approvals/card.rs:4`), or the three existing bootstrap failure labels (`bootstrap.rs:194-196`).
- Do not drain on the driver's runtime or from inside `execute`.

## Boundaries

### Allowed Changes
- native/hagency/src/bootstrap.rs
- native/hagency/src/bootstrap/approval.rs
- native/hagency/src/bootstrap/driver.rs
- native/hagency/src/bootstrap/config.rs
- native/hagency/tests/bootstrap.rs
- native/hagency/tests/bootstrap/approval.rs
- native/hagency-matrix/src/approval_delivery.rs
- knowledge/decisions/adr-110-native-private-approval-card.md
- knowledge/decisions/adr-112-native-private-approval-delivery.md
- specs/task-rust-private-approval-wiring.spec.md
- docs/progress.md
- docs/agent-knowledge.md

### Forbidden
- Live services, live homeservers, credentials in tree and deployed state.
- native/hagency/src/console.rs; native/hagency-store/src/domain/approvals/card.rs; native/hagency-matrix/src/collector.rs.

## Acceptance Criteria

Scenario: A service-composed run delivers a request end to end
  Test: native_private_approval_delivery_is_wired
  Level: integration
  Test Double: the real TLS fake peer with the approval bot's own enrollment; no live homeserver
  Given a fresh approval enrollment on the bot's own HostConfig and a host with ApprovalHost attached so start_mode creates the notices channel
  When an owned run raises an approval notice and the pump takes the single-consumer receiver
  Then the card is built from the admitted domain request and send_private_approval_card completes under the cancellation token
  And the pump terminates by itself when the worker drops the sender at run end
  And no runner capability workspace access or driver runtime reaches the pump

Scenario: The pump refuses without a fresh approval enrollment
  Test: native_private_approval_delivery_wiring_refuses_without_enrollment
  Level: integration
  Test Double: the same fixture with the enrollment anchors absent
  Given the service composition without with_fresh_account_enrollment
  When the pump is constructed
  Then it refuses with a named error and sends no card
  And a collector construction from a config the ordinary Collector refuses is never attempted through the pooled owner

## Out of Scope

The fail-closed denial policy (D-PC-FC, PC-C1), the public status notice and `PublicFrozen` (PC-C1), the operator observation surface (PC-C2), the oracle vectors (PC-C4), and any change to the existing approval-delivery or approval-store specs' selectors.
