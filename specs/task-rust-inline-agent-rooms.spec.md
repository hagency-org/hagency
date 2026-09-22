spec: task
name: "Create and join the original inline agent rooms before SDK enrollment"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-MATRIX-DM-PRIVACY, REQ-THREAD-SCOPED-SESSIONS]
tags: [active, rust, matrix, provisioning, rooms, custody]
---

## Objective

Continue the full ADR-147 factory with real ordinary-agent DM creation and
representative-authorized project invitation/join, then use those actual room
observations with the original pre-activation SDK owner. Do not substitute
configured existing rooms or synthetic membership for physical setup. Managed
home/runtime, both ADR-016 profiles and genuine completion/routes remain owed.

## Constraints

- Only the opaque account retaining the original inline writer/claim/registration
  may enter. Derive owner/project exclusively from its validated original request.
- Use separate protected representative and actual returned agent credentials.
  Fresh full-MXID whoami and real project binding/membership/invite powers must
  precede writes; repeat the original DomainStore guard after the last GET await.
- Create the private DM as the agent, inviting only the owner, with invite-only
  join rules, Megolm encryption and invited history. Never add a third creator,
  disable encryption, join as the owner, impersonate or complete UIA challenges.
- The representative invites the actual agent to the exact original project;
  the actual agent joins. Complete acknowledgements are separate from fresh
  observed joined membership. DM owner invitation is not joined membership.
- Reserve fixed create-only encrypted custody before each original POST. Pin
  original account/request/effect/fence/origin/representative credential and key.
  Never repeat a lost/torn/partial attempt; Complete reopen is GET-only inspection.
- Admit one immutable finite retained room job before any await. Outer caller
  loss does not discard it. Keep actual known room IDs inspectable, with no
  owner/task Arc cycle, and never clear unknown effects to relaunch.
- With actual known room creation/join, poll only the original DM state for the
  owner's real join. Running out of the attempt's limits.sdk budget is
  awaiting-owner, never unknown and never readiness: the effect stays Started
  and a later coordinator turn looks again (task-rust-owner-join-wait). A lost
  or torn POST is still unknown. Actual SDK enrollment follows through unchanged
  Agent-purpose original protocol/anchors/session custody, without publishing
  Active authority.
- Add only an explicit private registration_token_rooms_enrollment_step_v1
  marker. Credentials remain separate protected files, never driver JSON/DTOs.
  Existing account-only and absent profiles preserve their previous boundary.
- Leave the effect Started on observed rooms/SDK success. No home/runtime proof,
  Applied/Active/session route, missing-selector withdrawal, deployment/cutover,
  formatter, new dependency, commit or PR in this offline implementation slice.
- Cargo tests remain local TLS/private FS/independent real recipient SDK only.
- A negative room snapshot may finish intake before the independent owner's
  original join finishes. The fixture must continue answering that actor until
  its actual completion, without changing the refusal or extending its deadline.
- Plaintext project participants still require real authority/membership checks,
  but are not encryption recipients. Require peer anchors only for participants
  of encrypted rooms; never invent representative crypto authority for plaintext.

## Allowed changes

- native/hagency-matrix/src/token_provision.rs
- native/hagency-matrix/src/token_provision/rooms.rs
- native/hagency-matrix/src/token_provision/rooms/custody.rs
- native/hagency-matrix/src/provisioning.rs
- native/hagency-matrix/src/enrollment.rs
- native/hagency-matrix/src/enrollment/provisioning.rs
- native/hagency-matrix/tests/intake/provisioning.rs
- native/hagency-matrix/tests/enrollment/provisioning.rs
- native/hagency-matrix/tests/provision_rooms/mod.rs
- native/hagency/src/bootstrap/config.rs
- native/hagency/tests/bootstrap.rs
- knowledge/decisions/adr-147-provisioning-verdict-effect-route.md
- this spec, docs/agent-knowledge.md, docs/progress.md

## Scenarios

Scenario: Verified approval physically creates joins and enrolls the new agent
  Test: native_provisioning_inline_rooms_enrollment
  Level: integration
  Test Double: real inline Collector/writer/private files and local TLS with independent recipient SDK
  Given original representative approval and separate protected room/enrollment plan
  When the agent creates an encrypted DM, the representative invites it, the agent joins and the owner actually joins
  Then original SDK uploads/session claim complete using those real room IDs while the effect stays Started and no Active transport/route is fabricated

Scenario: Foreign credentials unsafe membership and changed authority refuse
  Test: native_provisioning_inline_rooms_refusals
  Given wrong representative identity, unsafe DM, refused invitation/join or changed original project/engagement scope
  When the actual operation reaches its next write/observation boundary
  Then it refuses without another POST, without SDK readiness and without physical completion

Scenario: Original room custody survives caller loss and lost responses
  Test: native_provisioning_inline_rooms_custody
  Level: integration
  Test Double: held local TLS create/invite/join and original encrypted files
  Given an admitted original room POST
  When its caller drops or its response is lost
  Then original ownership and known observations remain retained and no unknown POST repeats

Scenario: Room configuration cannot replace original observed rooms
  Test: native_provisioning_inline_rooms_replay
  Given actual known original rooms and original SDK Complete
  When approval replays or a caller tries a changed plan/room set
  Then no create/invite/join/key write repeats and no replacement room becomes ready

Scenario: Native bootstrap accepts only the explicit protected rooms profile
  Test: native_bootstrap_token_rooms_profile
  Level: integration
  Test Double: actual executable bootstrap/private files and malformed closed configuration
  Given the rooms/enrollment marker with public anchors and separate protected representative token
  When native configuration is prepared
  Then only the fixed valid plan attaches, no credential enters JSON and account-only/default behavior stays unchanged

Scenario: Original scope revoked during the last project GET prevents invitation
  Test: native_provisioning_inline_rooms_scope_change
  Given an actual known created DM and a held representative project-state GET
  When the original engagement is revoked before that GET responds
  Then no invitation or join POST or SDK write occurs and the known DM stays inspectable

Scenario: Torn partial foreign and extra room custody cannot rearm a POST
  Test: native_provisioning_inline_rooms_custody_refusals
  Given an actual original completed room job and protected fixed records
  When a record or wrapping key is torn, a stage is swapped or missing, or an extra file appears
  Then original read-only inspection refuses before HTTP, retains its known DM and becomes permanently spent
