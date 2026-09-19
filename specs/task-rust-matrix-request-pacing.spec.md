spec: task
name: "Share bounded Matrix request pacing across native fleet identities"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-LOCAL-THREE-RUNNER-QUALIFICATION]
tags: [active, rust, matrix, fleet]
---

## Intent

Independent native collectors and approval polling can exhaust the same server
quota. The diagnostic fleet received complete429 responses during provisioning
even with bounded GET retries. Provide an explicit host request cadence shared
by the coordinator, approval bot, provisioner, created agents and media clients.

## Constraints

- One retained process-local pacing owner per configured endpoint, never one
  independent quota per identity. Cloned Limits retain the same owner.
- Only trusted host configuration selects the optional10..1000ms interval.
- Waiting uses the original request deadline and cancellation token. It grants
  no authority and does not change SDK custody or domain generation checks.
- Apply pacing before every actual JSON or binary HTTP attempt, including GET
  retries. Never retry a write or unknown transport result.
- Do not change existing default cadence when the option is absent.
- Cross-process/shared-IP traffic can still exhaust server quotas; no global
  quota guarantee or historical failure reclassification is implied.
- Ordinary tests contact synthetic local TLS peers only.

## Allowed changes

- native/hagency-matrix/src/config.rs
- native/hagency-matrix/src/http.rs
- native/hagency-matrix/src/http/pacing_tests.rs
- native/hagency-matrix/src/lib.rs
- native/hagency/src/bootstrap/config.rs
- native/hagency/tests/configured_fleet/mod.rs
- specs/task-rust-matrix-request-pacing.spec.md
- knowledge/decisions/adr-176-matrix-request-pacing.md
- docs/**

## Scenarios

Scenario: Independent authenticated clients share the actual request cadence
  Test: native_matrix_shared_request_pacing
  Given two HTTP clients with one pacing owner and different credentials
  When concurrent JSON requests and a GET retry contact the local TLS peer
  Then all attempts wait for the shared cadence
  And a complete write refusal is not retried

Scenario: Binary transfers cannot bypass the shared request cadence
  Test: native_matrix_media_request_pacing
  Given the same pacing owner for JSON and binary transport
  When upload and download requests run
  Then both paths observe the cadence and original response contracts

Scenario: Waiting preserves cancellation and the original deadline
  Test: native_matrix_request_pacing_bounds
  Given a previously admitted request and a waiting caller
  When cancellation or its original deadline occurs
  Then no request is sent for that caller
  And invalid intervals or another endpoint are refused

Scenario: The native configured factory propagates its selected cadence
  Test: native_configured_local_codex_fleet
  Given the host selects request pacing
  When two real synthetic factory agents execute file and approval tasks
  Then the existing native ownership and delivery assertions still pass

Scenario: Driver configuration admits only an explicit bounded cadence
  Test: native_matrix_pacing_configuration
  Given an absent, valid or out-of-range interval
  When the native host constructs its Matrix limits
  Then the absent option preserves legacy behavior and invalid values refuse
  And cloned valid limits retain the same pacing owner
