spec: task
name: "Enroll the original approval SDK and deliver private owner cards"
inherits: project
satisfies: [REQ-OWNER-UI-APPROVAL, REQ-EXECUTION-AUTHORIZATION, REQ-MATRIX-DM-PRIVACY, REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, matrix, approval, crypto, custody]
---

## Intent

Implement accepted ADR112 on ADR110 private card metadata using the original
approval-purpose SDK, explicit fresh-account enrollment and one encrypted send.
Keep current send admission separate from historical delivery and owner verdicts.

## Constraints

### Must
- Use explicit checked approval-purpose enrollment and original private owner SDK without constructing an Agent ReplyRoute session or transport authority.
- Retain the original Arc PrivateApprovalCard and immutable owner cutoff before awaits and recheck exact card in the original domain writer after queued custody immediately before every secret-bearing HTTP write.
- Require fresh exact whoami device invite-only encrypted owner-and-bot membership and unchanged verified signed key response before each original PUT.
- Preserve original Complete enrollment identity and sessions through existing protected marker and ledger; new sends require explicitly enabled fresh approval enrollment.
- Preserve the actual completed HTTP response in an owned reserved SDK command before any cancellation await.
- Keep separate protected card attempt and at most 64 immutable receipts with exact purpose target content digest transaction bytes phases and acceptance shapes.
- Keep one original retained job and actual close result across caller loss panic timeout and SDK shutdown failure.
- Keep resumed historical custody network-free and incomplete enrollment intake or card crypto unavailable rather than rearming it.
- Retain existing SDK HTTP and operation limits and original absolute owner cutoff without extending deadlines.
- Keep card content at most 48 KiB encrypted room body at most 60 KiB query at most 256 KiB reply at most 4 KiB serialized attempt at most 1 MiB and encrypted shared journal at most 16 MiB.
- Reject foreign-purpose journal fields and current negative identity room key domain expiry task or binding evidence with exact existing fencing.
- Qualify fresh SDK sends and independent owner decryption through actual local HTTPS separately from test-only recipient provisioning and cross-compilation.

### Must Not
- Do not weaken ordinary outgoing or approval-intake purpose privacy and authentication guards.
- Do not treat a card a stored target a public master or an Accepted Matrix response as approval grant native application read receipt or runtime resumption.
- Do not regenerate enrollment requests or uncertain card attempts change original transaction bytes reset identities import service signing seeds or use plaintext fallback.
- Do not cancel an accepted SDK operation to make room for a new one or interpret an absent owner after failed close as success.
- Do not add automatic card discovery registration recovery SDK replacement bootstrap service wiring client parser changes or live network tests.
- Do not change database schema dependencies deadlines retry policy or active hosted CI.

## Boundaries

### Allowed Changes
- native/hagency-matrix/src/lib.rs
- native/hagency-matrix/src/approval_intake.rs
- native/hagency-matrix/src/approval_delivery.rs
- native/hagency-matrix/src/approval_delivery/enrollment.rs
- native/hagency-matrix/src/approval_delivery/jobs.rs
- native/hagency-matrix/src/approval_delivery/state.rs
- native/hagency-matrix/src/enrollment.rs
- native/hagency-matrix/src/sdk.rs
- native/hagency-matrix/src/sdk/enrollment.rs
- native/hagency-matrix/src/sdk/approval_intake.rs
- native/hagency-matrix/src/sdk/approval_delivery.rs
- native/hagency-matrix/src/sdk/encrypted_message.rs
- native/hagency-matrix/src/sdk/outgoing.rs
- native/hagency-matrix/tests/approval_delivery/mod.rs
- native/hagency-matrix/tests/approval_delivery/fixture.rs
- native/hagency-matrix/tests/approval_delivery/enrollment.rs
- native/hagency-matrix/tests/approval_delivery/privacy.rs
- native/hagency-matrix/tests/approval_delivery/custody.rs
- native/hagency-matrix/tests/approval_delivery/journal.rs
- native/hagency/tests/fixtures/matrix_crypto_peer.rs
- knowledge/decisions/adr-112-native-private-approval-delivery.md
- knowledge/decisions/adr-064-native-matrix-approval-intake.md
- knowledge/decisions/adr-059-native-matrix-outgoing.md
- knowledge/decisions/adr-102-native-matrix-trust-session-enrollment.md
- knowledge/context/native-private-approval-delivery.md
- specs/task-rust-native-private-approval-delivery.spec.md
- docs/agent-knowledge.md
- docs/progress.md

## Acceptance Criteria

Scenario: The fresh approval bot delivers exact encrypted private content
  Test: native_private_approval_fresh_enrollment_and_delivery
  Given a new approval-purpose SDK with externally anchored independent recipient keys
  When the real enrollment and encrypted delivery run through local HTTPS
  Then the original owner decrypts exact card fields and replay performs no second send

Scenario: Enrollment refuses unqualified identity and preserves original requests
  Test: native_private_approval_enrollment_refusals
  Given missing invalid changed cross-purpose or interrupted enrollment evidence
  When the original approval SDK attempts fresh enrollment or protected reopen
  Then no replacement identity claim or upload can rearm unknown work

Scenario: Current private authority is checked after original custody waits
  Test: native_private_approval_current_private_authority
  Given an original card with a held SDK write-possible boundary
  When private membership identity keys registration binding task or expiry changes
  Then the exact current original-domain and HTTPS checks prevent the next secret-bearing PUT

Scenario: Response loss and cancellation retain the original attempt
  Test: native_private_approval_send_cancellation_and_loss
  Given actual encrypted HTTP responses and original SDK mutation custody
  When the caller disappears cancellation occurs or a response is truncated
  Then accepted evidence is retained and no unknown enrollment intake or card is replaced

Scenario: Restart and finite history are inspection only
  Test: native_private_approval_historical_restart_and_capacity
  Given protected complete incomplete malformed conflicting or capacity-exhausted card history
  When the same original SDK is reopened for historical settlement
  Then no HTTP write occurs and original receipts remain bounded immutable and purpose separated

Scenario: Original shutdown custody remains observable
  Test: native_private_approval_retained_close
  Given the original approval SDK shutdown is held or returns an actual error
  When close loses its caller and later close intake or observe is requested
  Then the original result survives and no failed or unknown closure is counted as success

## Out of Scope

Agent authority changes domain schema or grant admission automatic registration
or identity recovery ongoing key maintenance executable service wiring Robrix
parser qualification live deployments and full migration acceptance. An admitted
HTTP write can race later remote or domain changes and cannot be recalled.
Windows cross-compilation cannot replace actual Windows execution evidence.
