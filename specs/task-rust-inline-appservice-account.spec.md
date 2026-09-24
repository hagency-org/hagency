spec: task
name: "Provision the original application-service account and private device"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-THREAD-SCOPED-SESSIONS]
tags: [active, rust, provisioning, appservice, privacy, custody]
---

## Objective

Implement ADR-016's mandatory second credential kind in the original inline
home/account/room/SDK owner. Use AS-authorized passwordless registration and
one dedicated legacy-AS device login, never an adopted/seeded account or token
masquerading as device authority. This remains a prerequisite for both complete
deployment profiles, native AS transactions, runtime/fleet/Applied/routes and
new real mini3 Palpo/Robrix qualification, not a replacement for those gates.

## Constraints

- Derive account/device and full side identity from original validated request
  and registration; only private Host configuration chooses the side credential
  and declared namespace prefix. Require the prefix matching this exact fleet.
- Authenticate AS registration/login in the Authorization header, not URL/body
  token fields. Registration inhibits login; save actual matching user creation
  before a separate create-only possible login/device boundary. No password.
- Dedicated device session belongs only to private SDK custody. It is not a
  canonical agent/side credential record. Side master remains private fixed Host
  authority, with current full sender/namespace checks before downstream writes.
- Actual out-of-namespace read-only identity probe must be refused; broad/wrong
  credentials cannot qualify. Neither owner impersonation nor membership grant
  is authorized by an AS token alone. Original domain guard follows last GET.
- Fixed encrypted stages pin profile/origin/credential/claim before every POST.
  Caller loss retains the original task/lock. Lost/partial/unknown register/login
  never retries, falls back or invents another device; accepted replay is GET-only.
- Preserve ordinary-account custody identity and default/partial-marker behavior.
  Expose only explicit closed appservice_login_home_rooms_enrollment_step_v1,
  separate protected AS/representative/wrapping-key files and non-secret paths.
- Keep SDK trust and private owner DM/project admission unchanged. Revoked side
  master blocks rooms and SDK even when its private device token still works.
- Unsupported legacy AS login fails closed; no alternate auth/device API or
  plaintext fallback. No second launcher, Applied/Active or synthetic route.
- Cargo tests stay offline/local TLS/private temp filesystem. No formatter,
  external dependency, deploy/reset/commit/PR or live writes in this contract.

## Allowed changes

- native/hagency-matrix/src/token_provision.rs
- native/hagency-matrix/src/token_provision/custody.rs
- native/hagency-matrix/src/token_provision/application_service.rs
- native/hagency-matrix/src/token_provision/rooms.rs
- native/hagency-matrix/src/provisioning.rs
- native/hagency-matrix/src/config.rs
- native/hagency-matrix/src/collector.rs
- native/hagency-matrix/src/enrollment/provisioning.rs
- native/hagency-matrix/src/lib.rs
- native/hagency-matrix/tests/token_provision.rs
- native/hagency-matrix/tests/intake/provisioning.rs
- native/hagency-matrix/tests/provision_rooms/mod.rs
- native/hagency/src/bootstrap/config.rs
- native/hagency/tests/bootstrap.rs
- specs/task-rust-inline-appservice-account.spec.md
- knowledge/decisions/adr-147-provisioning-verdict-effect-route.md
- docs/agent-knowledge.md
- docs/progress.md

## Scenarios

Scenario: AS registration and separate device login observe exact original identity
  Test: native_appservice_account_provision
  Given a fixed side credential and original Started account scope
  When actual local TLS registration login and fresh whoami complete
  Then encrypted custody retains matching account/device/session and no credentials escape

Scenario: Wrong broad expired unsupported or mismatched credentials refuse
  Test: native_appservice_account_refusals
  Given invalid namespace sender server device login or returned credential facts
  When the original account stage checks actual responses
  Then no alternate registration/login/device or readiness is admitted

Scenario: Accepted original AS custody reopens through reads only
  Test: native_appservice_account_reopen
  Given protected actual accepted registration/login and completed identity custody
  When the same original operation reopens
  Then fresh AS/device identity GETs validate without another POST, while torn/foreign/profile-changed custody refuses

Scenario: Caller loss and lost registration or login retain original ownership
  Test: native_appservice_account_custody
  Given actual possible AS registration or device login under its lock
  When the receiver drops or the response is lost
  Then original ownership settles or remains unknown without rearming another POST

Scenario: Original provider approval runs AS home rooms and SDK setup
  Test: native_provisioning_inline_appservice
  Given actual original reception/provider approval and fixed home/AS/peer plan
  When its original inline owner creates account/device/rooms and enrolls SDK
  Then a recipient decrypts in the created owner DM and actual home precedes account effects, with Started/Reserved and no fabricated routes

Scenario: Side-master revocation blocks downstream room and SDK effects
  Test: native_provisioning_inline_appservice_scope_change
  Given an actual original AS account whose private device token remains valid
  When its master is revoked while a downstream authority GET is held
  Then original partial observations survive but no subsequent room/key POST or Active/route is published

Scenario: Native executable accepts only the explicit closed AS home profile
  Test: native_bootstrap_appservice_home_profile
  Given non-secret side/home/peer configuration with separate private credentials
  When actual executable configuration prepares
  Then valid fixed configuration attaches and wrong/missing/extra/ordinary-marker inputs refuse before HTTP/model work
