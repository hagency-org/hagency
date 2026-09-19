spec: task
name: "Carry the observed readiness on the account DTO without asserting it"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, console, accounts, readiness, dto]
---

## Intent

Bind MA-S3b: add the readiness field to the console account surface's DTO —
the **sixth** key on `AccountRow` (id, ordinal, state, revision, profile,
readiness) — serving MA-S1's recorded fact and `unknown` when no fact
exists, never computing, inferring or asserting readiness in the console.
The DTO's redaction contract stays exactly as the retained product's own
console states it: what the retained page shows about an account's provider
login, and what it deliberately never shows.

## Constraints

### Must
- Add `readiness` to `AccountRow` and its exact-key validator **in one commit** (the `object()` helper checks set and count — a key added server-side without the validator fails the whole read, and vice versa).
- Serve exactly the MA-S1 fact: the account's latest **observed, unexpired** mode (`subscription`/`api_key`), else `unknown` — including when the fact is absent, expired, or `uncertain` (an interrupted login is never a readiness answer).
- Evaluate at read time from the recorded fact, exactly as MA-S2's gate does; a read never writes and never promotes.
- Match the retained redaction contract: the readiness word, the state word and the fix pointer are operator-facing; the credential home path, the credential-present tri-state's underlying file answer, any token-shaped byte, and the provider's raw probe output are **never** served.
- Render the readiness word through `t()` with entries in both dictionaries (`en`, `zh`).

### Must Not
- Do not compute, infer or assert readiness in the console — no filesystem check, no directory existence, no model selection, no auth attempt, no live probe of any kind; the console reads the recorded fact or says `unknown`.
- Do not drive a login from any console surface — the login is the operator's own host act (MA-S1, D-ADR114); no route, button or link starts one.
- Do not touch the store: the fact, its tables and its migration are MA-S1's, landed; this slice adds a key that reads them through the existing account surface's read.
- Do not widen `AccountRow` beyond six keys or serve `AccountChoice` (its `authentication`/`quota` fields read as readiness answers and break the exact-key contract).
- Do not change the console boundary machinery (hoops, five-document exception, semaphore, session rules, the required asset key); do not gate any scenario by OS or feature in the binding set.

## Boundaries

### Allowed Changes
- native/hagency/src/console/accounts.rs
- native/hagency/src/console.rs
- native/hagency/src/console/assets.rs
- mockup/components/NativeAccounts.jsx
- mockup/lib/native-api.js
- mockup/lib/i18n.js
- mockup/app/accounts/page.jsx
- mockup/scripts/build-native-console.mjs
- native/hagency/tests/console/accounts.rs
- native/hagency/tests/console.rs
- native/hagency/tests/console/browser.rs
- specs/task-rust-console-account-readiness-field.spec.md
- docs/progress.md

### Forbidden
- Live providers, live probes, credentials in tree, deployed state.
- native/hagency-store/** (MA-S1 owns the fact); any login-driving code path; native/hagency/src/bootstrap/accounts.rs; native/hagency/src/main.rs.

## Acceptance Criteria

Scenario: The account DTO matches the retained redaction contract
  Test: native_console_account_dto_matches_retained_redaction
  Level: integration
  Test Double: the console fixture with accounts in every readiness shape and a seeded credential-shaped value
  Given accounts whose store rows carry observed, expired, uncertain and absent readiness facts
  When the list and single reads are served
  Then every account object carries exactly the six declared keys
  And the readiness word is the observed mode or unknown and no other field appears
  And no byte of any response contains a credential home path, a token-shaped value, a credential-present file answer or any provider probe output

Scenario: The DTO's readiness is observed, never asserted
  Test: native_console_account_readiness_is_observed_not_asserted
  Level: integration
  Test Double: the console fixture with an expired fact and an uncertain attempt beside an observed one
  Given the three readiness shapes on one account surface
  When the reads are served
  Then the observed-unexpired fact reports its mode and the expired and uncertain ones report unknown
  And no read path performs any filesystem, directory, model or network check to produce the word
  And a read never writes and never promotes a fact

## Decisions

**Depends on MA-S1 and MA-S3a landing.** MA-S1 owns the fact (migration 028,
the observation tables, the `usable` predicate); MA-S3a owns the five-key
DTO this slice widens by exactly one key. Neither exists on this branch yet —
this spec binds the field against their landed shapes and is unimplementable
before them, by design.

**The sixth key moves in one commit with the validator** — the exact-key
conjunction is the contract's own fence against silent widening, and a
half-landed key fails every read loudly rather than rendering a missing
field.

**The retained redaction contract, as the retained page states it**: the
operator sees the provider-login *state word* and the *fix pointer*
("run `${command} login`") — never the credential home's absolute path (the
retained page renders `~/.codex`, and native serves nothing at all), never
the raw `credentialPresent` file answer, never any probe output. Native's
`readiness` word is the observed counterpart of the retained state word;
native serves no fix pointer because the fix is the operator's own command,
not a console action.

## Out of Scope

MA-S1's fact and its store (landed spec), MA-S2's dispatch gate, MA-S4
(subscription materialisation), the login command and any surface that
drives it, and every other console page.
