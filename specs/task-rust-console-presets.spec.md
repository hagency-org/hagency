spec: task
name: "Serve the presets and catalogue section from the existing resources read"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, console, resources, catalogue]
---

## Intent

Bind the presets-and-catalogue design v2 (G5, as a section of the existing
resources page fed by the existing route): three derived keys on each role row
— `families`, `fillable`, `overTier` — computed over the one predicate
`Resource::qualifies` that already produces `available`, read-only by
construction. No new route, no new scope, no mutation; the write side stays
ADR-111's.

## Constraints

### Must
- Derive `families` as the model family (`qualification::model(&r.profile()).1`, `qualification.rs:143-156`), sorted for a stable wire — never a framework name (`derive.js:91,100` counts model families).
- Derive all three values over the same qualifying set as `available`: `published ∧ provisionable() ∧ ceiling.tokens.is_some() ∧ qualification::qualifies` (`project.rs:168-173`); do NOT use `qualification::resources_for_role`.
- Serve a role with no qualifying resource with `fillable` 0 and an empty `families` — the key is never omitted.
- Keep the one-commit contract change: server `json!` keys, the `deny_unknown_fields` `RoleRow` (`console/resources.rs:57-59`) and the client's exact-key conjunction (`native-api.js:10-11,176-178`) move together.
- Keep the page's derived label and its reasoned blank for `rateCapPerDay`.

### Must Not
- Do not add a `name`, `rateCapPerDay`, `apiBaseUrl`, `apiKeySet`, `extraArgs` or `usedBy` key — absent by ADR-111 and the store's own shape, not by omission.
- Do not change `qualification.rs`'s policy tables, `resources_for_role` itself, the two existing scopes, any mutation route, the operator refusal `roles_are_model_derived` (`native/hagency/src/resources.rs:95`), or the count assertion at `resources.rs:150-152`.
- Do not touch the console boundary machinery (five-document exception, hoops, semaphore, `build-native-console.mjs`) — no page is added.
- Do not gate any scenario by OS or feature in the binding set.

## Boundaries

### Allowed Changes
- native/hagency-store/src/domain.rs
- native/hagency-store/tests/catalog_publication.rs
- native/hagency/src/console/resources.rs
- native/hagency/tests/console/resources.rs
- mockup/components/NativeResources.jsx
- mockup/lib/native-api.js
- specs/task-rust-console-presets.spec.md
- knowledge/decisions/adr-108-native-console-resource-publication.md
- native/README.md
- docs/progress.md

### Forbidden
- Live services, credentials, deployed state and Matrix server changes.
- native/hagency-core/src/qualification.rs; native/hagency/src/resources.rs; mockup/app/resources/page.jsx; mockup/app/capability/page.jsx.

## Acceptance Criteria

Scenario: Every catalogue value comes from one predicate and the model family
  Test: native_console_catalogue_fillability_is_derived
  Level: integration
  Test Double: real store rows; a provisionable resource and a non-provisionable one, both published with a ceiling
  Given two published resources with a ceiling on different model families where only one is provisionable
  When the resources read is served
  Then fillable counts only the provisionable one exactly as available does
  And families lists model families and never a framework name
  And a resource whose model matches no policy tier adds no family and is not over-tier
  And a role with no qualifying resource reports fillable 0 rather than omitting the key

Scenario: The catalogue omits every profile field native does not persist
  Test: native_console_catalogue_omits_unpersisted_profile_fields
  Level: integration
  Test Double: a published resource whose profile carries a key-shaped value
  Given a published resource
  When the resources read is served
  Then no role or resource row carries a name rateCapPerDay apiBaseUrl apiKeySet or extraArgs key
  And no byte of the response contains a stored key value

Scenario: The roles table keeps the eight-key set exactly and the page reaches ready
  Test: native_console_resource_observations
  Level: integration
  Test Double: the native console fixture and the built resources page
  Given the native console fixture and the built resources page
  When the resources page loads
  Then every role row satisfies the exact eight-key validator in native-api.js:176-178
  And the page reaches data-native-resource-state ready and never the pre-correction data-native-state
  # the EXISTING selector (tests/console/resources.rs:130), extended here; the F4-corrected
  # attribute is the component's own (NativeResources.jsx:58)

## Out of Scope

The retained presets and catalogue pages, the offer-terms write (canonical
persistence, ADR-111), `usedBy` (blocked on the agent roster, CL-S1), any CLI
surface (the CLI reads the operator route, not the console route), and every
other console page.
