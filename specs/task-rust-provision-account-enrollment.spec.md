spec: task
name: "Enroll the original provisioned account before engagement activation"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREAD-SCOPED-SESSIONS, REQ-MATRIX-DM-PRIVACY]
tags: [active, rust, matrix, provisioning, enrollment, custody]
---

## Objective

Remove the physical factory's activation/enrollment cycle. The opaque account
observed by the original inline owner must be able to enroll its actual SDK
while that same provision effect is Started and its engagement Reserved.
This is a prerequisite within the full ADR-147 factory, not an alternative
pre-created-account product or a replacement for room/home/runtime provisioning.
The existing bootstrap account-only profile remains account-only. Both ADR-016
profiles and the original completion/session-route gates remain required.

## Constraints

- Only the opaque account carrying the original inline DomainStore/effect and
  registration may enter this operation. Standalone account observations cannot.
- Read the original payload's validated ProjectRequest. Bind exactly its project
  Group and a distinct encrypted owner+agent Direct room, never its approval DM,
  the reception room, another owner/project, or an arbitrary replacement room set.
- Fresh whoami and actual room state are required before creating the SDK, before
  every key write and after the final SDK acknowledgement. Require joined owner
  and agent, exact Direct privacy, and exact project owner/id binding and invite
  authority. Configured rooms and successful room-creation responses alone are
  not membership evidence. Every joined peer still requires its independent anchor.
- The original DomainStore checks actual Started effect/fence/payload, Reserved
  engagement and registration after the last room-read await at write boundaries.
  Do not publish active transport/room observations, Applied, sessions or routes.
- Keep the unchanged Agent-purpose protected enrollment ledger, actual SDK
  requests/acknowledgements/session associations, unknown refusal and budgets.
  Normal Agent enrollment must still require Active transport; do not loosen it.
- Admit one immutable retained account enrollment job before any await. Keep its
  Collector, original owner and result after outer caller loss. Changed profiles
  cannot replace it. Repeated successful calls revalidate current scope without
  uploads/claims; failed or running jobs remain spent. No owner/task Arc cycle.
- Explicit closure settles only the retained SDK custody, with its own owned
  acknowledgement and sticky failed/pending closure. It does not claim canonical
  lifecycle/transport/route cleanup, and the closed job cannot reopen as readiness.
- Cargo tests are offline local TLS/private filesystem/independent recipient SDK.
  No live server writes, fixture seeding of the service SDK, formatter, commit,
  PR, new dependency, driver JSON profile or release qualification in this slice.

## Allowed changes

- native/hagency-matrix/src/token_provision.rs
- native/hagency-matrix/src/provisioning.rs
- native/hagency-matrix/src/enrollment.rs
- native/hagency-matrix/src/enrollment/provisioning.rs
- native/hagency-matrix/tests/intake/provisioning.rs
- native/hagency-matrix/tests/enrollment/fixture.rs
- native/hagency-matrix/tests/enrollment/mod.rs
- native/hagency-matrix/tests/enrollment/provisioning.rs
- native/hagency-matrix/tests/token_provision.rs
- this spec, docs/agent-knowledge.md, docs/progress.md

## Scenarios

Scenario: The original observed account enrolls without premature activation
  Test: native_provisioning_account_enrollment
  Level: integration
  Test Double: original inline Collector/domain writer/local TLS and independent actual recipient SDK
  Given canonical approval physically registers and observes the opaque account
  When its actual project and encrypted owner DM are observed and SDK enrollment completes
  Then original signing uploads and session claim execute, the independent recipient decrypts a real room key/event through those persisted sessions, exact protected Agent enrollment reopens, and the effect stays Started with no Active transport/session route

Scenario: Account enrollment requires the original immutable scope
  Test: native_provisioning_account_enrollment_refusals
  Given a changed profile, wrong owner/project/binding, unsafe Direct room or revoked original effect
  When enrollment reaches its actual current-scope boundary
  Then it refuses before the next key write and never publishes active authority

Scenario: Account enrollment custody survives caller loss and unknown writes
  Test: native_provisioning_account_enrollment_custody
  Level: integration
  Test Double: held actual TLS key upload and lost response with original private SDK
  Given one admitted original account enrollment
  When its outer caller drops or its admitted POST loses the response
  Then its finite retained owner settles or retains uncertainty, no second original write starts, and retained state cannot rearm

Scenario: Completed account enrollment does not survive scope change as readiness
  Test: native_provisioning_account_enrollment_replay
  Given original protected Complete and its persisted sessions
  When called again or actual registration/engagement/room scope changes
  Then only unchanged current scope passes read-only revalidation, no uploads/claims repeat, and changed scope grants no readiness

Scenario: Revocation during the final real room read prevents the original key POST
  Test: native_provisioning_account_enrollment_scope_change
  Level: integration
  Test Double: held local TLS Direct state read after actual SDK Preparing/Possible; real canonical revocation
  Given the original SDK request is prepared and possible but its last room GET is held
  When the original engagement is revoked before that GET responds
  Then the post-GET original writer check refuses before the actual key POST, and the retained job never retries
