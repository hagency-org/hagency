spec: task
name: "Retain inline factory agents in the original approval collector"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-OWNER-UI-APPROVAL, REQ-EXECUTION-AUTHORIZATION, REQ-MATRIX-DM-PRIVACY]
tags: [active, rust, matrix, factory, approval, custody]
---

## Objective

Allow the original approval bot to observe, enroll, deliver and read verdicts for
agents that its original inline factory has activated. Its startup configuration
cannot predict engagement IDs minted later by authenticated Matrix ingress.

## Constraints

- Keep the original configured engagement ordering, fixed bot/device/origin/
  registration/rooms/anchors, producing writer and approval-purpose SDK.
- Extend membership only inside the original factory observation job, using its
  opaque writer-produced OwnedProvisionScope after the exact Active ACK. Validate
  that same scope on the original writer before and after authenticated private
  room observation. No public engagement-admission setter or metadata recovery.
- The original busy permit serializes scope insertion, intake, enrollment, send
  and close. Accepted work survives waiter loss. A failed observation never adds
  a member. Each operation freezes its complete membership before awaiting work.
- The existing 64-engagement bound includes static and factory membership.
  Refuse capacity before HTTP; retain members without eviction or rearming.
- Membership is admission only. Actual send still requires original Complete
  enrollment and fresh card, identity, private membership, signed keys and domain
  authority. Current revocation and negative observations continue to refuse.
- Reuse existing delivery, verdict and close paths with frozen membership. No
  alternate SDK, changed deadlines, plaintext fallback or synthetic permission.
- Offline integration must provision the target through actual factory ingress
  with both account kinds and use the original approval SDK. The independent
  owner shares its own identity across the agent and bot fixtures and hands out
  distinct original one-time keys; service keys are populated only by real
  production HTTPS uploads. No target activation/binding/session is seeded.
- These checks do not replace native startup/fleet/pump integration, effective
  sandbox qualification, actual Palpo/Robrix approval round trips or full soaking.

## Allowed changes

- native/hagency-matrix/src/approval_intake.rs
- native/hagency-matrix/src/approval_delivery.rs
- native/hagency-matrix/src/approval_delivery/enrollment.rs
- native/hagency-matrix/src/provisioning/factory.rs
- native/hagency/tests/inline_factory.rs
- native/hagency/tests/inline_factory/mod.rs
- native/hagency/tests/fixtures/matrix_crypto_peer.rs
- knowledge/decisions/adr-064-native-matrix-approval-intake.md
- knowledge/decisions/adr-112-native-private-approval-delivery.md
- knowledge/decisions/adr-147-provisioning-verdict-effect-route.md
- this spec, docs/agent-knowledge.md, docs/progress.md

## Scenarios

Scenario: A newly activated factory agent uses the same bot's encrypted card path
  Test: native_provisioning_factory_approval_delivery
  Given an actually provisioned target absent from the bot's startup engagements
  When its typed current runner request produces a private card and the original bot enrolls and sends
  Then the independent owner decrypts exactly that request's card, replay sends nothing, and no owner verdict or runtime permission is invented

Scenario: The factory's failed private observation never admits a new member
  Test: native_provisioning_factory_approval_refusal
  Given an original factory and its original bot whose owner membership changes
  When the post-ACK private observation fails
  Then the target retains Active history without an approval binding or route and is absent from subsequent bot observation

Scenario: A lost factory waiter retains the original bot membership job
  Test: native_provisioning_factory_waiter_loss
  Given the actual original factory whose caller disappears during SDK verification
  When its owned job finishes
  Then the original initialized runtime, SDK, approval membership and route are retained without replacement

Scenario: Revocation during the actual private read cannot admit membership
  Test: native_provisioning_factory_approval_revoked
  Given the original activated target at its authenticated private-room request
  When that same engagement is revoked before the actual HTTP response returns
  Then membership and routing remain absent, canonical revocation cancels and advances the effect fence while preserving its original Applied digest, and closing the original bot only drains its admitted members

Scenario: Scope membership preserves static order and finite custody
  Test: native_factory_approval_membership_bounds
  Given original configured members and retained factory admissions
  When members are inserted repeatedly or the combined bound is reached
  Then duplicates do not grow the registry, capacity refuses before any insertion, and snapshots retain configured ordering followed by admitted members
