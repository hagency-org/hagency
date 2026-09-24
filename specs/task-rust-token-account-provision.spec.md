spec: task
name: "Host-owned ordinary Matrix account creation with original uncertain custody"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, native, matrix, provisioning, custody]
---

## Objective

Implement the registration-token account step needed by ADR-147's inline
physical provisioner. ADR-016 requires both registration-token and application
service deployment profiles; this ordinary-account primitive is not the full
factory, not the application-service profile, and not provisioning completion.
The two missing ingress completion/route selectors remain required.

## Ownership and bounds

The process Host constructs the operation from an exact Started provision
effect and its current registration. No HTTP request, RunnerCommand,
Deserialize, Debug, credential getter or generic endpoint/auth setter exposes
the account-creation capability. It creates no DomainRepository observation,
Active state, session route, SDK enrollment or runtime-launch receipt.

Use the fixed registration endpoint and returned-account whoami endpoint only,
the existing no-proxy/no-redirect/no-retry bounded HTTP transport, one original
absolute deadline and existing bounded DNS/body/framing admission. Registration
requests carry no representative bearer token. Only returned credentials may
authenticate the new account's whoami. Validate actual returned MXID/device;
never infer success from a desired username. Require the original
engagement-scoped namespace and device, refusing a mapped/substituted account.

Before any registration POST, persist original private context and
WritePossible custody. A lost response, deadline, dropped receiver or process
loss never permits another registration. A valid retained successful response
may be reopened only to inspect the same credential with fresh whoami, or, after
a restart, to run that same agent again in a re-attach mode that can never
register or log in (task-rust-factory-agent-reattach).
Persist authenticated bounded response custody before reading it as usable.
Bind every encrypted record to the original registration/effect/fence/identity
and credential profile; wrong bindings, corruption, partial bootstrap and
missing records fail closed without replacing keys or clearing custody.

The password is independently random, used only within this original attempt,
then discarded; do not derive it from IDs, save it or provide login recovery.
Registration-token UIA may follow one observed token-only challenge with the
same username, password, device and session. No dummy, CAPTCHA, email, terms,
shared-secret admin, open-registration fallback or retry is introduced.
Credentials and raw response bodies stay in protected encrypted private
custody, outside canonical domain/console/config projections. A typed returned
Host handle may build a fixed ordinary-account HostConfig without exporting
its token. It is account evidence, not dispatch/room/crypto authority.

## Allowed changes

- `native/hagency-matrix/src/token_provision.rs`
- `native/hagency-matrix/src/token_provision/`
- `native/hagency-matrix/src/config.rs`, `http.rs`, `lib.rs`
- `native/hagency-matrix/tests/token_provision.rs`
- `native/hagency-matrix/tests/common/mod.rs` (zero-delay fixture writes only)
- `native/hagency-matrix/Cargo.toml`, `Cargo.lock` for the already-used RNG
- this spec, ADR-147's truthful checkpoint, `docs/progress.md`

## Scenarios

Scenario: The token account is physically observed under its original binding
  Test: native_token_account_provision_observes_registration
  Level: integration
  Test Double: bounded local TLS Matrix peer, synthetic token and claimed effect; actual protected filesystem and transport
  Given an exact Started provision effect and a private registration-token credential
  When the Host executes its original registration attempt
  Then one token-only UIA exchange retains the same random password/device/session, the actual returned credential authenticates whoami, and only matching account/device returns an opaque usable account handle

Scenario: Accepted registration is reopened for inspection without another write
  Test: native_token_account_provision_reopens_original_response
  Level: integration
  Test Double: bounded local TLS Matrix peer; actual protected encrypted filesystem and reopened operation
  Given an original accepted response retained before a failed whoami observation
  When the Host reopens the same scope
  Then it freshly inspects that same credential, makes no registration POST, and refuses conflicting context, corruption or identity substitution

Scenario: Uncertain registration cannot be retried or raced
  Test: native_token_account_provision_retains_uncertainty
  Level: integration
  Test Double: bounded local TLS peer with held/lost response; actual private lock and WritePossible custody
  Given a registration POST with no conclusive accepted response
  When the original receiver is dropped or another operation opens the same scope
  Then original ownership/custody remains retained, concurrent creation is refused, and later reopen makes no registration request

Scenario: Bounds and unsupported registration profiles fail closed
  Test: native_token_account_provision_refuses_unsafe_profiles
  Level: integration
  Test Double: bounded local TLS Matrix peer and synthetic invalid Host inputs
  Given invalid original scope or a non-token-only/malformed challenge, redirect, oversized response, or substituted account/device
  When the Host prepares or executes registration
  Then it refuses without fallback, token/password disclosure, fabricated account evidence or canonical provisioning writes

Scenario: UIA and account inspection share the original absolute deadline
  Test: native_token_account_provision_original_deadline
  Level: integration
  Test Double: bounded local TLS Matrix peer with three individually admissible virtual-time delayed responses; actual protected response custody
  Given registration and whoami steps whose cumulative time exceeds the original account-operation budget
  When each step would separately fit the HTTP request budget
  Then the original operation still expires, retains accepted registration for fresh inspect-only recovery, and makes no repeat registration POST

The cumulative-deadline selector controls Tokio time, not filesystem or TLS
results. Three300ms response delays still exceed the unchanged750ms original
budget; real IO is polled with time stationary between those boundaries. A real
3s watchdog bounds fixture orchestration. This proves cumulative deadline and
retained-response semantics, not physical IO latency. The original real-time
failure before the expected third request remains recorded, not relabeled.
The local peer skips a timer for an explicitly zero-delay chunk; positive delays,
held gates, actual TLS writes/flush/shutdown and all transport limits are unchanged.

## Completion scope

Offline Cargo tests use only local fixtures and private temporary files. Real
Palpo creation is a separate opt-in operator qualification, not a test/CI
dependency. This slice must not replace the full inline home/SDK/owner-DM/
project-membership/runtime provisioning or its real two-agent qualification.
