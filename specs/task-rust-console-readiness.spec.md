spec: task
name: "Serve the console readiness and version strip from the existing boundaries"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, console, readiness, version]
---

## Intent

Bind ADR-145: a readiness and version strip on every native console page, reading
the existing unauthenticated `GET /ready` as-is (never `/health`, never a console
route, never proxied) and two build-time constants from a generated module. No
Rust file changes; the readiness contract (ADR-096's brief-19/21 amendments)
stays frozen.

## Constraints

### Must
- Read `GET /ready` from the page same-origin and consume the payload as-is — exactly `{"status","implementation","components[].{name,state}}`, no new wire keys.
- Generate `status-constants.js` in the staged tree before `next build`, carrying `HAGENCY_NATIVE_VERSION` (parsed from the root Cargo.toml's `[workspace.package]`) and `HAGENCY_NATIVE_SCHEMA_HEAD` (a `--schema-head` argument copied from the migration registry). The staged tree is the build's `mkdtemp` workspace — **no repo path for the generated file is committed or expected in Allowed Changes**; if Next excludes an unbundled generated module, the constants are appended to an existing staged `.js` instead (same values, same test).
- Render `status === "ok"` as ready; 503/`unavailable` as not ready with failing components' names and words — a failing component never renders ready; an unreachable `/ready` renders unknown, never ready.
- Render the sweep-tick cell's raw word unstyled by outcome — `refused_*` is a ready word (`lib.rs:207-210`); only the sweep's liveness participates in the colour.
- Render the strip as a `PageHead` child on every native console page.
- Validate the payload by exact key set and count only; unknown state words render as text, never error.

### Must Not
- Do not read `/health`, add any console route for readiness, or proxy `/ready` through `/console/api/*`.
- Do not add any field to `manifest.json` or touch `native/hagency/src/console/assets.rs` — the constants travel as a bundled `.js`, and "no Rust change" holds by construction.
- Do not enumerate readiness state words in the client — one vocabulary, the server's.
- Do not add restart or stop controls — explicitly out of scope until the service wrapper exists.
- Do not change the readiness contract: `/health` 200-while-live, `/ready` the 503 boundary, the `ComponentState` vocabulary and its enumeration test, diagnostic-never-authority.

## Boundaries

### Allowed Changes
- mockup/components/NativeStatusStrip.jsx
- mockup/components/PageHead.jsx
- mockup/components/NativeUsage.jsx
- mockup/components/NativeResources.jsx
- mockup/components/NativeAlerts.jsx
- mockup/components/NativeEngagements.jsx
- mockup/lib/native-api.js
- mockup/scripts/build-native-console.mjs
- native/hagency/tests/console/status_strip.rs
- native/hagency/tests/console.rs
- specs/task-rust-console-readiness.spec.md
- knowledge/decisions/adr-145-console-readiness-version-strip.md
- native/README.md
- docs/progress.md

### Forbidden
- Live services, credentials, deployed state.
- native/hagency/src/** (no Rust source file — the strip's own rule); mockup/app/** page files; the manifest schema.

## Acceptance Criteria

Scenario: The strip renders not-ready on a 503 component
  Test: native_console_status_strip_503_renders_not_ready
  Level: integration
  Test Double: the console fixture with a closed domain writer driving /ready to 503; browser-lane
  Given the native-console-browser feature whose selector appears in cargo test --list under --all-features exactly as native_console_browser is bound by the usage console spec and a console page rendered while /ready answers 503 with a failing component
  When the strip renders
  Then it shows not ready with the failing component's name and state word and never the word ready

Scenario: The strip renders unknown when /ready is unreachable
  Test: native_console_status_strip_unreachable_renders_unknown
  Level: integration
  Test Double: the console fixture with the readiness route unreachable; browser-lane
  Given the native-console-browser feature whose selector appears in cargo test --list under --all-features exactly as native_console_browser is bound by the usage console spec and a console page whose readiness fetch fails at the network level
  When the strip renders
  Then it shows unknown with no component list and never ready

Scenario: The built assets carry the workspace version after a rebuild
  Test: native_console_status_strip_version_matches_workspace
  Level: integration
  Test Double: the real asset bundle located through HAGENCY_NATIVE_CONSOLE_ASSETS; never the synthetic console fixture
  Given console assets built by build-native-console.mjs with the generated status-constants module
  When the bundled HAGENCY_NATIVE_VERSION value is compared with the workspace [workspace.package] version
  Then the values are equal and the assertion is on the value never a chunk hash or size
  And a missing bundle or a missing constant fails the test never skips

## Out of Scope

Restart and stop controls (the service wrapper's slice), any Rust change, the
retained console's pages, and the readiness payload itself (frozen by
ADR-096's amendments).
