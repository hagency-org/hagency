spec: task
name: "Serve the native project-side observation behind bounded browser authority"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, console, project-sides, browser]
---

## Intent

Bind the project-side read-only observation (ADR-132): a
`/console/api/project-sides` projection over the fleet registrations and their
projects that omits credentials in every byte, and the `/console/project-sides/`
page that renders it. One spec per page family, per the house pattern. The store
list read is owned here and lands first (the slice is two commits). Browser
selectors follow the same gated lane the usage console spec's Decisions block
records: compiled only under the default-off `native-console-browser` feature,
listed by `cargo test --list` under `--all-features` exactly as
`native_console_browser` is bound there, executed only in the enabled lane.

## Constraints

### Must
- Add the bounded `SELECT`-named `project_sides()` list read (registrations `LEFT JOIN` projects) with the `DomainStore` wrapper and `lib.rs` re-export, before the route.
- Mount `GET /console/api/project-sides` under the console `authenticate` hoop with no scope: scope facts are a payload, never a gate on reads.
- Serve exactly the six declared keys with `projects[]` of exactly `{id, room_id}` capped at 64 per side; withhold `owner_mxid`/`owner_room_id`.
- Publish a server-owned `unavailable` list naming every retained column with no native source.
- Assert over the serialized response bytes, not only the parsed key set.
- Stage the page through the asset key map, mime allowlist and `/console/project-sides` alias; the required key `/console/usage/` stays.

### Must Not
- Do not serve `as_token`, `hs_token` or any credential-shaped value through any key or any nested byte.
- Do not derive `hasCredential: false` or any claim about a record that does not exist.
- Do not add a new scope, and do not widen the five-document exception; the page serves as a non-document with no query string.
- Do not touch the retained proxy allowlist or `publicSide`.

## Boundaries

### Allowed Changes
- native/hagency-store/src/domain.rs
- native/hagency-store/src/domain_worker.rs
- native/hagency-store/src/lib.rs
- native/hagency-store/tests/project_sides.rs
- native/hagency/src/console/project_sides.rs
- native/hagency/src/console.rs
- native/hagency/src/console/assets.rs
- native/hagency/tests/console/project_sides.rs
- native/hagency/tests/console.rs
- native/hagency/tests/console/browser.rs
- native/hagency/Cargo.toml
- mockup/app/project-sides/page.jsx
- mockup/components/NativeProjectSides.jsx
- mockup/lib/native-api.js
- mockup/scripts/build-native-console.mjs
- specs/task-rust-console-project-sides.spec.md
- knowledge/decisions/adr-132-native-project-side-observation.md
- native/README.md
- docs/progress.md

### Forbidden
- Live services, credentials, deployed Node runtime and Matrix server changes.
- mockup/app/api/hagency/[...path]/route.js; mockup/app/projects/page.jsx; lib/project-side-store.js.

## Acceptance Criteria

Scenario: The project-side projection omits credentials in every byte
  Test: native_console_project_side_projection_omits_credentials
  Given a registered fleet whose store config carries a credential-shaped value and projects on its side
  When the console reads the project sides through a valid session
  Then the serialized response body contains none of that value and none of as_token hs_token asToken or hsToken
  And every item carries exactly the six declared keys with projects entries of exactly id and room_id
  And the top-level unavailable list names every column native has no source for

Scenario: A foreign origin cannot read the project sides
  Test: native_console_project_side_refuses_foreign_origin
  Given a valid console session cookie
  When the projection is read with a cross-site sec-fetch-site or a foreign host or origin
  Then the route refuses with console_origin_required and serves no side item

Scenario: The project-sides page renders under the native browser boundary
  Test: native_console_project_side_browser
  Given the native console fixture and the built project-sides page with the native-console-browser feature whose selectors appear in cargo test --list under --all-features exactly as native_console_browser is bound by the usage console spec
  When real Chromium opens /console/project-sides without any operator token
  Then the list reaches data-native-state ready with no external request and no credential value on screen

## Out of Scope

A CLI read via an `/api/native/v1/project-sides` operator route, the rail row (a
retained-file edit), the retained side card, and every other console page.
