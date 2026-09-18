spec: task
name: "Run the original approval SDK enrollment and owner verdict pump in the native service"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-OWNER-UI-APPROVAL, REQ-EXECUTION-AUTHORIZATION, REQ-MATRIX-DM-PRIVACY]
tags: [active, rust, bootstrap, approvals, matrix]
---

## Objective

Complete the native service's private approval round trip: configured fresh bot
SDK enrollment, original run notice, encrypted card, authenticated encrypted owner
verdict intake, and the original runtime callback response. PC-C0 delivered only
the card and required test-side SDK enrollment; neither proves this round trip.

## Constraints

- Use the configured separate approval bot token/device/SDK and external anchors.
  Startup drives the existing observe and enroll_fresh_account operations before
  starting the driver. Original Complete enrollment is validated, not replaced.
  This is Matrix crypto enrollment, not ADR114 provider login or credential entry.
- Keep polling the actual HTTP service during startup enrollment. A failed or
  unknown enrollment prevents dispatch. Preserve the original collector on a
  failed/unknown close; no replacement, reset, retry or success inference.
- Keep the original single-consumer ApprovalRequests on the service runtime.
  Re-read cards from the domain; hold at most 64 distinct pending notice IDs.
  Only those current pending IDs form the original HostApprovalPlan. No request
  discovery, caller-supplied proofs, runtime capability or workspace enters it.
- Poll the original ApprovalCollector serially while requests remain pending,
  yielding between polls and preserving its existing finite journal capacities.
  Any refused/unknown intake stops this pump; do not retry a retained batch or
  invent an owner decision. Channel end and cancellation end the pump.
- Card acceptance is not permission. Only the existing authenticated SDK proof
  and atomic domain admission decide; only the original runtime applies its
  callback. Plaintext, bad anchors and stale scopes must remain fail-closed.
- No increased deadlines/capacities, alternate SDK, new dependencies, weakened
  sandbox, live services in tests, production test switches or seeded bot keys.
- Executable tests use the real hagency child, pinned callback probe, fake TLS
  Matrix peer and independent owner OlmMachine. The fresh test starts without an
  approval SDK or approval binding; production writes create both. Preserve the
  existing pre-enrolled delivery-only regression and its limited claims.

## Allowed changes

- native/hagency/src/bootstrap.rs
- native/hagency/src/bootstrap/approval.rs
- native/hagency/tests/bootstrap/approval.rs
- native/hagency/tests/support/approval_enrollment.rs
- native/hagency/tests/fixtures/approval_mcp_peer.rs
- native/hagency/tests/fixtures/matrix_crypto_peer.rs
- knowledge/decisions/adr-064-native-matrix-approval-intake.md
- knowledge/decisions/adr-112-native-private-approval-delivery.md
- this spec, docs/progress.md, docs/agent-knowledge.md

## Scenarios

Scenario: Fresh native startup carries a real encrypted owner decision to its callback
  Test: native_private_approval_roundtrip_encrypted_owner
  Given the actual service with a fresh configured approval SDK and a pinned callback probe
  When its independent owner decrypts the card and returns an encrypted exact structured verdict
  Then original HTTPS enrollment creates the bot keys, the original store admits the verdict, and the probe receives the matching approval callback without a second attempt

Scenario: A plaintext verdict cannot decide the native request
  Test: native_private_approval_roundtrip_plaintext_refused
  Given the same actual service and an owner card
  When a plaintext structured verdict is polled before the encrypted owner verdict
  Then it is durably rejected while the request remains pending, and only the subsequent authenticated encrypted verdict produces the callback

Scenario: Wrong external anchor prevents any dispatch
  Test: native_private_approval_startup_wrong_anchor
  Given fresh native approval configuration with a different valid external public master
  When actual startup observes the room and queries the independent owner identity
  Then enrollment refuses without dispatch or a private card

Scenario: Failed close cannot consume the original approval owner
  Test: native_private_approval_close_retains_original
  Given a real configured collector whose domain authority is unavailable at close
  When the bootstrap observes its original negative-fencing failure twice
  Then both calls remain unknown with the same collector retained and both database locks held, without HTTP, SDK creation or a synthetic successful close

## Remaining goal scope

These offline executable checks are not real Codex sandbox qualification, full
fleet/multi-agent scheduling, Robrix click qualification, sustained Palpo soaking,
whole-port parity or production cutover. All remain owed by the full goal.
