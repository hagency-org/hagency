spec: task
name: "Expose early Matrix collector errors in identity and bounded sync fixtures"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, matrix, fixtures]
---

## Intent

Preserve a collector result that settles before its scripted HTTP requests
instead of hiding it behind a later fake-peer timeout.

## Constraints

### Must
- Use the existing script-first completion race for both authenticated identity collections and the bounded sync loop.
- Preserve all authentication restart identity cancellation and negative authority assertions.
- Report an actual early collector result as failure before waiting for another impossible HTTP request.
- Keep every production SDK HTTP fake-peer and shutdown deadline unchanged.
- Preserve historical Windows CI and independent shutdown failures as separate failed evidence.

### Must Not
- Do not retry requests ignore errors widen deadlines or infer the historical collector cause.
- Do not change production Matrix transport storage schemas or media durability evidence.

## Boundaries

### Allowed Changes
- native/hagency-matrix/tests/transport.rs
- knowledge/decisions/adr-080-native-matrix-transport-fixture-errors.md
- specs/task-rust-matrix-transport-fixture-errors.spec.md
- docs/progress.md
- docs/agent-knowledge.md

## Acceptance Criteria

Scenario: Authenticated HTTPS and restart retain exact identity checks
  Test: native_matrix_transport_identity_authenticated_https_and_sdk_restart
  Level: integration
  Test Double: local scripted HTTPS and actual private SDK storage
  Given an untrusted HTTPS endpoint followed by authenticated collection and SDK restart
  When each collection races its original HTTP script
  Then untrusted access remains refused and authenticated restart preserves the same SDK identity

Scenario: Bounded sync and cancellation retain negative authority
  Test: native_matrix_transport_bounds_sync_scopes_events_and_cancel
  Level: integration
  Test Double: local scripted HTTP and actual collector cancellation
  Given foreign rooms excess events excessive JSON depth cancellation and oversized headers
  When bounded collection races each unchanged HTTP script
  Then every refused response remains negative authority and early collection completion is visible

Scenario: Early refusal exposes the actual collector error
  Test: native_matrix_transport_rooms_fixture_reports_early_refusal
  Level: integration
  Test Double: local scripted HTTP wrong-account response
  Given a real wrong-account whoami response and an impossible next scripted request
  When the collector refuses identity before the script finishes
  Then the fixture reports the actual Identity error instead of a later request timeout

Scenario: A legal SDK interval does not terminate the script
  Test: native_matrix_transport_rooms_fixture_accepts_sdk_sized_gap_between_requests
  Level: integration
  Test Double: local scripted HTTP and a bounded deliberate SDK-sized interval
  Given actual local HTTP requests separated by the existing legal SDK interval
  When the script driver polls the requests and collection together
  Then the original sequence completes within the unchanged fixture bound

## Out of Scope

Historical Windows root-cause proof, deadline changes, live services, encrypted
staging qualification, independent Palpo shutdown uncertainty and migration parity.
