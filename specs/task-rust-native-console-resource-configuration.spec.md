spec: task
name: "Create and edit additional native resource configurations"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, console, resources, browser]
---

## Intent

Implement accepted ADR111 from d85369d within the parent-approved exact 36 paths.
Reuse the retained four-step wizard for additional configurations from an existing
private account association. No schema dependency or live configuration changes.

## Constraints

### Must
- Keep existing private association framework provider and publication semantics in the original writer.
- Use canonical model reasoning qualification and preserve untouched ceiling null and missing values.
- Issue separate finite configuration authority and retain a concrete non-Clone non-Deserialize command.
- Acquire SQLite before the nonblocking original session gate and never acquire the global map from that gate.
- Check original clocks after waiting and before mutation and commit without renewing deadlines.
- Keep conflict unknown create and Busy logout visible with no automatic mutation retry.
- Clear old drafts on navigation credential replacement and logout while preserving unsaved same-selection drafts.
- Reuse retained controls preferences translations and actual dynamic static document routing.
- Execute all seven bound selectors with actual nonzero results and bilingual browser and executable acceptance.
- Keep enabled browser prerequisites fatal and ordinary tests independent of optional browser tooling.

### Must Not
- Do not expose private preset account credential or auth-home facts to the browser.
- Do not collect unsupported native names rate caps execution policy keys endpoints or extra arguments.
- Do not invent runtime readiness quota first-resource enrollment or full M7 completion.
- Do not overwrite existing resources on create or retry a mutation automatically after unknown outcomes.
- Do not widen deadlines add dependencies create a fresh broad cache or contact live services.

## Boundaries

### Allowed Changes
- ./knowledge/decisions/adr-111-native-console-resource-configuration.md
- ./specs/task-rust-native-console-resource-configuration.spec.md
- ./native/README.md
- ./docs/agent-knowledge.md
- ./docs/progress.md
- ./native/hagency-core/src/qualification.rs
- ./native/hagency-core/tests/qualification.rs
- ./native/hagency-store/src/lib.rs
- ./native/hagency-store/src/domain.rs
- ./native/hagency-store/src/domain_worker.rs
- ./native/hagency-store/src/domain/resource_publication.rs
- ./native/hagency-store/src/domain/resource_configuration.rs
- ./native/hagency-store/tests/resource_configuration.rs
- ./native/hagency/src/console.rs
- ./native/hagency/src/console/authority.rs
- ./native/hagency/src/console/client.rs
- ./native/hagency/src/console/resources.rs
- ./native/hagency/src/console/resource_configuration.rs
- ./native/hagency/src/console/assets.rs
- ./native/hagency/src/main.rs
- ./native/hagency/tests/console.rs
- ./native/hagency/tests/console/fixture.rs
- ./native/hagency/tests/console/browser.rs
- ./native/hagency/tests/console/configuration.rs
- ./native/hagency/tests/console/resources.rs
- ./mockup/app/resources/new/page.jsx
- ./mockup/components/NativeResources.jsx
- ./mockup/components/Data.jsx
- ./mockup/components/Rail.jsx
- ./mockup/lib/native-api.js
- ./mockup/lib/i18n.js
- ./mockup/scripts/build-native-console.mjs
- ./mockup/scripts/native-console-resource-configuration-browser.mjs
- ./mockup/scripts/native-console-resources-browser.mjs
- ./tests/dashboard-native-resource-configuration.test.js
- ./tests/dashboard-native-resources.test.js

## Acceptance Criteria

Scenario: Canonical choices retain qualification meanings
  Test: native_resource_configuration_choices
  Level: integration
  Test Double: original embedded policy and native profiles
  Given known and unknown model profiles
  When the native choice projection is read
  Then only exact canonical combinations are selectable and role preview is model qualification

Scenario: Create and edit preserve original account and budgets
  Test: native_resource_configuration_create_edit
  Level: integration
  Test Double: real private SQLite resources seats and commitments
  Given an existing resource with its original private account association
  When current create and edit commands commit and state reopens
  Then only allowed profile and ceiling fields change while identities withdrawal and commitments remain truthful

Scenario: Configuration authority remains current after waiting
  Test: native_resource_configuration_current
  Level: integration
  Test Double: actual SQLite locks and original session gates with controlled clocks
  Given a concrete original session command
  When waits expiry retirement caller loss and commit races occur
  Then stale work cannot mutate and possible commits remain unknown without retry

Scenario: Configuration writes need their exact explicit scope
  Test: native_console_resource_configuration_authority
  Level: integration
  Test Double: real Salvo issuer exchange and original domain writer
  Given read-only publication and configuration sessions
  When configuration requests and logout race original authority
  Then only exact current configuration scope admits a command and Busy does not claim revocation

Scenario: Editor observations expose only bounded native facts
  Test: native_console_resource_configuration_observations
  Level: integration
  Test Double: actual native resource state through Salvo
  Given published withdrawn unusual and newly created profiles
  When bounded editor reads complete or lose authority
  Then safe typed responses preserve exact current fields and never expose private account or preset data

Scenario: Retained wizard creates and edits in both languages
  Test: native_console_resource_configuration_browser
  Level: integration
  Test Double: actual Chromium native Salvo and retained static wizard
  Given English and Chinese configuration sessions
  When users create edit navigate refresh and encounter real conflicts unknown replies or Busy logout
  Then drafts revisions current state and explicit outcomes remain truthful without automatic mutation retry

Scenario: Configuration works through the native executable
  Test: native_console_resource_configuration_executable
  Level: integration
  Test Double: actual native executable with private state and empty runtime PATH
  Given retained wizard assets and explicit operator-issued configuration access
  When the browser creates edits and the native executable restarts
  Then actual resource state persists without a deployed Node server or live provider

## Decisions

The existing mandatory native-console-browser CI lane executes the complete
console test target and feature-enabled Clippy. Local agent-spec feature adaptation
must record source hash and actual argv and disclose adapted lifecycle execution.
An all-feature inventory lists tests only; it is never execution evidence.

## Out of Scope

First-resource enrollment credential management account rebinding friendly-name
persistence rate-cap enforcement runtime settings live providers and full M7.
