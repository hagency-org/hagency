spec: task
name: "Hand the original provisioned SDK to current Active operation"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREAD-SCOPED-SESSIONS, REQ-MATRIX-DM-PRIVACY]
tags: [active, rust, matrix, provisioning, enrollment, custody]
---

## Objective

Consume the original pre-activation enrollment Collector after the full inline
factory's genuine Applied transition. Preserve its same SDK owner, credential,
crypto store, identity, binding, enrollment ledger and original claimed sessions.
This is the Active handoff within ADR-147, not another deployment profile or a
substitute for the still-required full inline factory and live qualification.

## Constraints

- Only the original opaque account's retained successful enrollment job may
  admit this handoff. No configuration export, raw observation setter, replacement
  Collector/SDK, fresh identity, key upload, session claim or reset.
- Require the actual original provision effect Complete and engagement Active,
  with original Started acknowledgement, fence, payload and current registration.
  Preserve the existing Started/Reserved-only pre-activation validator unchanged.
  A new read-only Active validator cannot write Applied or synthesize authority.
- Fresh exact whoami, original project binding/owner/invitation powers and actual
  joined/encrypted two-person owner DM remain required. Recheck the original
  writer after the last HTTP/SDK await. AS side authority remains checked too.
- Use the same Collector busy permit, original SDK queue and existing Complete
  verification/collection path. Capture the original absolute SDK budget before
  spawning one retained handoff job. Caller drop cannot release its permit or
  discard its final result; no task/owner Arc cycle.
- Running, failed or closed jobs cannot rearm. Successful Active refresh may only
  revalidate the same Complete and current owner. Pre-activation calls cannot
  move an Active job backwards. Failed Active collection fences only its original
  transport identity through the existing negative CAS path.
- Tests are offline local TLS/private SQLite and independent real recipient SDK.
  Fixture Applied is explicitly not proof of physical factory fulfillment. No
  live service/model, formatter, dependency, commit/PR, deploy/reset or new marker.
- Full factory selectors native_provisioning_effect_completed and
  native_provisioning_session_route remain required and are not withdrawn or
  satisfied with fixture activation. Both complete ADR-016 profiles, actual warm
  owner consumption, AS receiver/registration generation, real fleet/two-agent,
  completion/recovery reliability and real Palpo/Robrix re-soaking remain owed.

## Allowed changes

- native/hagency-store/src/domain/verified_ingress.rs
- native/hagency-store/src/domain_worker.rs
- native/hagency-store/tests/provision_active.rs
- native/hagency-matrix/src/collector.rs
- native/hagency-matrix/src/token_provision.rs
- native/hagency-matrix/src/enrollment/provisioning.rs
- native/hagency-matrix/src/sdk.rs
- native/hagency-matrix/tests/enrollment/provisioning.rs
- native/hagency-matrix/tests/enrollment/active_handoff.rs
- native/hagency-matrix/tests/provision_rooms/mod.rs
- knowledge/decisions/adr-102-native-matrix-trust-session-enrollment.md
- knowledge/decisions/adr-147-provisioning-verdict-effect-route.md
- this spec, docs/agent-knowledge.md, docs/progress.md

## Scenarios

Scenario: Only the exact original completed provision has current Active scope
  Test: native_provisioning_active_account_scope
  Given the original writer-acknowledged Started provision
  When read before or after explicit offline fixture Applied, or with changed scope
  Then only its exact Complete/Active/current registration passes the read-only Active validator and the Started validator still refuses Active

Scenario: Active handoff consumes the same original enrollment SDK
  Test: native_provisioning_sdk_active_handoff
  Level: integration
  Test Double: original inline account/Collector/writer/local TLS and independent actual recipient SDK
  Given successful original pre-activation enrollment and explicit fixture Applied
  When the original account hands off and refreshes current Active operation
  Then the same SDK channel/owner and Collector survive, original Complete verifies, fresh transport/rooms are observed, no key upload or session claim repeats, and no session route is fabricated

Scenario: Changed or closed Active handoff cannot rearm
  Test: native_provisioning_sdk_active_refusals
  Given a premature, changed registration, revoked, unsafe room, replaced peer or closed original owner
  When Active handoff or its replay is attempted
  Then it refuses, retains its spent original result and never replaces the owner or repeats enrollment writes

Scenario: A lost Active waiter leaves the admitted original job owned
  Test: native_provisioning_sdk_active_custody
  Level: integration
  Test Double: actual held local TLS room read, original SDK and real canonical revocation
  Given an admitted original Active handoff
  When its waiter drops or its last read crosses actual revocation
  Then the same finite job/permit retains the result, changed current scope refuses before readiness, and no key write, second owner or synthetic route appears

Scenario: Active AS handoff retains side authority despite a live device token
  Test: native_provisioning_sdk_active_appservice
  Level: integration
  Test Double: original inline AS home/account/rooms/enrollment and actual local TLS with independent owner SDK
  Given the original successful AS device enrollment and explicit fixture Applied
  When its Active handoff refreshes or the side master is revoked/broadened
  Then only unchanged actual namespace authority passes on the same owner, a still-live dedicated device cannot substitute for the side master, and no registration/login/room/key/session effect repeats
