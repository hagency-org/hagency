spec: task
name: "Manage native resource catalog visibility in the retained console"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, console, resources, browser]
---

## Intent

Implement accepted ADR108 from the clean dfba95b checkpoint. Root reviewed the
concrete scope, original 31 paths, lock order and transition table before mutation.
Root approved NativeUsage as the exact 32nd path for shared truthful logout retry.
This is the next step toward full resource management; the retained wizard's
missing native fields and discovery contracts remain explicit follow-up work.

## Decisions

Ordinary Cargo tests are independent of browser tooling. The existing mandatory
native-console-browser feature lane must actually execute both new browser
selectors; missing assets or Node or Chromium is a failure when enabled.
Local lifecycle feature adaptation must disclose its source hash and actual argv;
listing gated tests never counts as their execution.

## Constraints

### Must
- Retain the existing resource page controls preferences and both languages.
- Keep native operator and runner APIs closed to browser authority.
- Keep ordinary console sessions read only and grant publication only through an explicit finite management ticket.
- Retain a concrete opaque non-Clone non-Deserialize command from the exact original management session.
- Acquire SQLite IMMEDIATE before the per-session nonblocking mutation gate and never acquire the global session map from that gate.
- Check original session and request deadlines after SQLite waiting and before mutation and commit without renewing them.
- Keep unrelated reads and sessions independent of a mutation gate held through disk IO.
- Compare actual configuration revision and change only publication and derived roles.
- Preserve missing pool and shared-account values and periods without inventing legacy DTO fields.
- Bound requests queries bytes pages assets and deadlines using the existing console limits.
- Keep Busy and unknown logout visible with an explicit retry and no claimed revocation.
- Keep mutation conflict and unknown visible with read-only current-state reconciliation and no automatic write retry.
- Test actual dynamic resource selection and local catalog publication in English and Chinese browsers and the native executable with an empty runtime PATH.
- Parse and lint the contract then verify exact nonzero selectors and all changed paths.

### Must Not
- Do not put credentials auth-home paths internal preset IDs or full resource configurations in browser DTOs.
- Do not claim native catalog changes establish remote Palpo publication.
- Do not expose a generic browser-authority callback or accept a browser-supplied scope assertion.
- Do not replace the retained console with a separate minimal product.
- Do not invent runtime readiness seat members rate caps execution policy or provider accounting.
- Do not call live services change live configuration widen deadlines introduce dependencies or claim M7 complete.

## Boundaries

### Allowed Changes
- knowledge/decisions/adr-108-native-console-resource-publication.md
- specs/task-rust-native-console-resource-publication.spec.md
- native/README.md
- docs/agent-knowledge.md
- docs/progress.md
- native/hagency-store/src/lib.rs
- native/hagency-store/src/domain.rs
- native/hagency-store/src/domain/resource_publication.rs
- native/hagency-store/src/domain_worker.rs
- native/hagency-store/tests/resource_publication.rs
- native/hagency/src/console.rs
- native/hagency/src/console/authority.rs
- native/hagency/src/console/client.rs
- native/hagency/src/console/resources.rs
- native/hagency/src/console/assets.rs
- native/hagency/src/console/usage.rs
- native/hagency/src/main.rs
- native/hagency/tests/console.rs
- native/hagency/tests/console/fixture.rs
- native/hagency/tests/console/browser.rs
- native/hagency/tests/console/resources.rs
- mockup/app/resources/page.jsx
- mockup/components/NativeResources.jsx
- mockup/components/NativeUsage.jsx
- mockup/components/ResourceAgents.jsx
- mockup/components/Data.jsx
- mockup/components/Rail.jsx
- mockup/lib/native-api.js
- mockup/lib/i18n.js
- mockup/scripts/build-native-console.mjs
- mockup/scripts/native-console-resources-browser.mjs
- tests/dashboard-native-resources.test.js

## Acceptance Criteria

Scenario: Publication compares the actual configuration atomically
  Test: native_resource_publication_cas
  Level: integration
  Test Double: original private SQLite repository and resource edits
  Given a native configuration and a revision read from its actual stored fields
  When current or stale publication commands arrive
  Then only the matching revision changes publication without overwriting profile or budget

Scenario: Original finite publication authority survives queued work
  Test: native_resource_publication_current
  Level: integration
  Test Double: original SQLite write lock and concrete session gate with real expiry
  Given an exact session command with original deadlines
  When queue or SQLite waits caller loss expiry retirement or gate contention occurs
  Then stale commands cannot mutate and possible commits remain unknown without retry

Scenario: Browser publication needs its exact management scope
  Test: native_console_resource_authority
  Level: integration
  Test Double: actual finite console authority and native writer behind Salvo
  Given read-only and management sessions from the native issuer
  When publication or logout races requests and expiry
  Then only current management scope admits writes and Busy never claims revocation

Scenario: Resource observations preserve native meanings
  Test: native_console_resource_observations
  Level: integration
  Test Double: canonical configurations seats roles and commitments in fresh native state
  Given published withdrawn missing and partially bounded resources
  When bounded resource and budget reads complete or lose authority
  Then safe DTOs preserve current native facts and absent values without private fields

Scenario: Retained resource controls work in both languages
  Test: native_console_resources_browser
  Level: integration
  Test Double: actual Chromium and native Salvo with new runtime resource creation
  Given the retained static resource page and finite native management access
  When users select publish withdraw refresh and observe conflict unknown or Busy logout
  Then current state preferences and explicit action outcomes remain truthful in both languages

Scenario: Resource management runs without deployed Node
  Test: native_console_resources_executable
  Level: integration
  Test Double: actual native executable and private temporary state with empty runtime PATH
  Given the retained native resource assets
  When the native access command and resource catalog action run
  Then the claimed workflow uses native HTTP without a Next server

## Out of Scope

Resource creation full profile editing friendly-name persistence rate caps execution
policy host discovery Agent detail routing seat declaration edits role publication
writes remote Palpo delivery production packaging and complete M7 parity.
