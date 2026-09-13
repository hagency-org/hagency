spec: task
name: "Serve the console and CLI account surface without readiness"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, console, accounts, managed-accounts]
---

## Intent

Bind MA-S3a of the account-surface design v3: list, prepare (reserve+
materialize), retire and enrol through the console under one finite scope, and
through the CLI's existing offline `account` verb — every route serializing the
five-key `AccountRow` only, never `AccountChoice`, with ADR-114's opacity kept
(no readiness, no login) and `ConsoleAccess` staying a flat variant. The store
wrappers are the sibling MA-S3a-store slice (`native_console_account_wrappers_
mirror_the_store`); this spec binds the console block's five selectors.

## Constraints

### Must
- Serialize `AccountRow` — exactly the five declared keys (id, preset, role framework, state, revision) — on the list read, the single read and all three mutation responses; `AccountChoice` never reaches the wire.
- Mount the three mutations under one finite scope (`Scope::Account`, `--manage-account-enrollment`), mutually exclusive with both existing management flags; reads stay scope-free.
- Take `expected_revision` on the enrolment mutation only; reserve and retire take none.
- Keep the unknown window honest: an interrupted or slow preparation stays inspectable, its state never invents an outcome, and the row is never dropped.
- Land the printed ticket link on `/console/accounts/` (the third branch of client.rs:118-126); the page is a non-document with no query string, outside the five-document exception.
- Assert the identity negative over BOTH value classes: the decoded-JSON walk (every string value and object key, recursively — contains neither the seeded tuple T nor volume V, equals none of S, T, V, P) and the raw-byte search for the alphanumeric pair S (seat id) and P (preset id) only. A raw search for T is unsound (escaped on the wire) and must not be used.

### Must Not
- Do not serve `namespace_identity`, `identity_tuple`, `seat_id`, `preset_id` or any identity-triple component through any key — no readiness inference, no login, no auth-file read, no legacy import.
- Do not add a `ConsoleAccount` CLI subcommand; the offline `account` verb stays the CLI surface.
- Do not change `ManagedAccount`'s `!Clone`/`!Deserialize`, `AccountEnrollmentAccess` as `AccountEnrollmentCommand`'s only constructor, `materialize_account`'s one-shot ordering, the `resource_accounts` immutability triggers, the console boundary machinery (five-document exception, hoops, semaphore, session rules, the required asset key), or the existing two flags' `conflicts_with`.
- Do not gate any scenario by OS or feature in the binding set (the browser scenario rides the native-console-browser lane as ever).

## Boundaries

### Allowed Changes
- native/hagency/src/console/accounts.rs
- native/hagency/src/console.rs
- native/hagency/src/console/authority.rs
- native/hagency/src/console/client.rs
- native/hagency/src/console/assets.rs
- native/hagency/src/main.rs
- native/hagency/tests/console/accounts.rs
- native/hagency/tests/console.rs
- native/hagency/tests/console/browser.rs
- native/hagency/tests/cli.rs
- mockup/lib/native-api.js
- mockup/components/NativeAccounts.jsx
- mockup/app/accounts/page.jsx
- mockup/scripts/build-native-console.mjs
- specs/task-rust-console-accounts.spec.md
- knowledge/decisions/adr-108-native-console-resource-publication.md
- knowledge/decisions/adr-111-native-console-resource-configuration.md
- knowledge/decisions/adr-114-native-managed-account-binding.md
- native/README.md
- docs/progress.md

### Forbidden
- Live services, live auth, credentials in tree, deployed state.
- native/hagency-store/src/domain/accounts.rs; the CLI's offline account verb implementation (only its pair test binds here).

## Acceptance Criteria

Scenario: Every account response carries the five keys and no seeded identity value
  Test: native_console_account_routes_carry_no_identity
  Level: integration
  Test Double: a real prepared and enrolled account seeded with a known seat S, tuple T, volume V and preset id P
  Given an active account whose store row holds seat_id S, identity_tuple T, namespace volume V and preset id P
  When the list read, the single read and all three mutation responses are served
  Then every response body carries exactly the five declared keys per account object
  And every decoded string value and key in every response contains neither T nor V and equals none of S T V P
  And the raw body of every response contains neither S nor P as a substring
  And no response carries a key named namespace_identity, identity_tuple, seat_id or preset_id

Scenario: The account grant is mutually exclusive with the existing grants
  Test: native_console_account_grant_is_mutually_exclusive
  Level: unit
  Given the console-access command line
  When --manage-account-enrollment is combined with either existing management flag
  Then the command is refused before any ticket is issued
  And each of the three pairs is refused independently

Scenario: A read-only session cannot prepare retire or enrol
  Test: native_console_account_mutations_require_the_scope
  Level: integration
  Given a read-only console session
  When each of the three mutation routes is called
  Then each refuses with account_scope_required and no account row changes state
  And with the scoped session the same call succeeds and the row state advances
  And only the enrolment body carries an expected revision

Scenario: An interrupted or slow preparation stays inspectable and unknown
  Test: native_console_account_prepare_interrupted_is_unknown
  Level: integration
  Given a preparation whose store reply is delayed past the two-second reply bound while the five-second preparation deadline has not elapsed
  When the account is read before the outcome resolves
  Then the row is present with an unknown-in-progress state and no invented outcome
  And the row is never dropped and remains readable after the outcome settles either way

Scenario: The accounts page renders under the native browser boundary
  Test: native_console_accounts_browser
  Level: integration
  Test Double: real Chromium over a fresh native fixture, receiving no operator token
  Given the native console fixture and the built accounts page
  When the browser opens /console/accounts
  Then the list reaches data-native-state ready with no external request
  And no credential namespace tuple seat or volume value appears in the DOM or in local or session storage

## Out of Scope

The store wrappers (the sibling MA-S3a-store selector), the readiness enum and
D-ADR114 (MA-S3b), login or auth inspection, the command-binding follow-up for
reserve and retire, and every other console page.
