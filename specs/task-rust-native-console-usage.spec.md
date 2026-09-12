spec: task
name: "Serve retained usage components through bounded native browser authority"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, console, usage, browser]
---

## Intent

Implement the accepted ADR107 read-only M7 slice from migration1cb60d7 using the
existing browser page and the original typed native ledger. Root reviewed this
exact twenty-seven-path boundary before any source mutation and approved the
two-path amendment for the existing pinned cap-fs-ext dependency edge and the
thirtieth workflow path for a mandatory separate browser qualification lane and
the thirty-first inventory path to list all feature-gated tests without executing them.

## Decisions

Browser selectors are compiled only with the explicit default-off
`native-console-browser` feature. Their bound command is
`cargo test --locked -p hagency --features native-console-browser --test console -- --nocapture`.
Missing assets, Node or Chromium fail in that enabled lane. Ordinary Cargo runs
do not execute those scenarios and are not evidence of their acceptance. Strict
lifecycle must use the same enabled feature and inspect each actual test count.

## Constraints

### Must
- Preserve the existing usage component preferences and English and Chinese presentation.
- Keep native underlying operator and runner APIs closed to browser headers.
- Bound access issuance tickets sessions bodies queries requests assets and deadlines.
- Retain and verify actual nofollow asset snapshots before serving immutable bytes.
- Distinguish public document navigation from authenticated API request authority.
- Recheck original session after awaited reads and retire authority before shutdown.
- Preserve null counts absent periods incomplete history lower bounds and exact failures.
- Discover and select engagements created after the browser asset build.
- Retain same-selection observations with an explicit refresh indicator and mark failed refreshes stale while navigation credentials and logout clear mismatched or retired data.
- Run real Chromium against fresh native state in both languages and the real native executable without Node on PATH.
- Parse and lint before implementation then run exact nonzero tests and strict lifecycle with all changed paths.

### Must Not
- Do not put operator credentials tickets cookies or session secrets in assets or script-accessible browser storage.
- Do not create legacy fleet DTOs fixture success fallbacks billing claims or allocation inferences.
- Do not expose arbitrary proxy routes filesystem paths source records rooms or runtime identities.
- Do not contact live services modify live configuration add dependencies or claim all M7 workflows complete.

## Boundaries

### Allowed Changes
- native/scripts/check-rust-spec-bindings.mjs
- .github/workflows/rust.yml
- ./Cargo.lock
- native/hagency/Cargo.toml
- native/hagency/src/lib.rs
- native/hagency/src/main.rs
- native/hagency/src/bootstrap.rs
- native/hagency/src/console.rs
- native/hagency/src/console/assets.rs
- native/hagency/src/console/authority.rs
- native/hagency/src/console/usage.rs
- native/hagency/src/console/client.rs
- native/hagency/tests/console.rs
- native/hagency/tests/console/fixture.rs
- native/hagency/tests/console/browser.rs
- mockup/next.config.mjs
- mockup/package.json
- mockup/components/Data.jsx
- mockup/components/Rail.jsx
- mockup/components/NativeUsage.jsx
- mockup/app/usage/page.jsx
- mockup/lib/native-api.js
- mockup/lib/i18n.js
- mockup/scripts/build-native-console.mjs
- mockup/scripts/native-console-browser.mjs
- tests/dashboard-native-usage.test.js
- knowledge/decisions/adr-107-native-console-usage.md
- specs/task-rust-native-console-usage.spec.md
- native/README.md
- docs/agent-knowledge.md
- docs/progress.md

## Acceptance Criteria

Scenario: Lock contention cannot extend ticket or session authority
  Test: native_console_clock_after_lock
  Level: unit
  Test Double: clock callback inspects the held original mutex and separate real held-lock expiry regression
  Given a request waiting on the original session mutex
  When absolute expiry passes before that mutex becomes available
  Then the production clock is sampled after acquisition and the credential is refused

Scenario: Ticket and session clocks enforce finite capacity
  Test: native_console_finite_clock
  Level: unit
  Test Double: production authority methods with deterministic monotonic clock values
  Given finite issuance tickets and four session slots
  When ticket replacement expiry session expiry or retirement occurs
  Then stale credentials never regain authority or reset the absolute lifetime

Scenario: Browser authority has finite isolated scope
  Test: native_console_authority
  Level: integration
  Test Double: Salvo request transport over actual console authority and fresh writers
  Given an operator issuer and finite browser tickets and sessions
  When requests replay expire exceed bounds change authority or attempt native API access
  Then only the original current session reads the fixed console facade

Scenario: Assets retain verified original file evidence
  Test: native_console_assets
  Level: integration
  Test Double: real private temporary manifest and file mutations
  Given bounded nofollow manifest assets
  When files links hashes lengths or HTTP paths are changed
  Then startup refuses invalid sets and admitted bytes remain the verified originals

Scenario: Native usage remains truthful and bounded
  Test: native_console_usage
  Level: integration
  Test Double: fresh canonical engagement and actual typed ledger fixtures
  Given missing partial regressed and complete aggregate observations
  When bounded session reads or post-await retirement occur
  Then evidence optional counts periods and failures remain explicit without private fields

Scenario: Retained browser works against native service
  Test: native_console_browser
  Level: integration
  Test Double: actual Chromium and Salvo with fresh writers and built retained components
  Given the native static usage build and a runtime engagement absent from that build
  When English and Chinese users exchange access select usage reload refresh and encounter missing state
  Then preferences truthful evidence query selection visible refresh state and privacy survive in the actual browser

Scenario: Native executable serves without deployed Node
  Test: native_console_executable
  Level: integration
  Test Double: actual native binary with private fresh state and no Node on PATH
  Given validated retained browser assets
  When the installed native executable serves and the local access command runs
  Then native HTTP serves the claimed console workflow without a Next server

## Out of Scope

Other console pages full Agent routing fleet usage aggregation health alerts
operations maintenance remote roles packaging production configuration and cutover.
