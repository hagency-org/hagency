spec: task
name: "Bounded Matrix GET rate-limit recovery"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-LOCAL-THREE-RUNNER-QUALIFICATION]
tags: [active, rust, matrix, recovery]
---

## Intent

An explicit Matrix429 on an observational GET should wait and re-read within
its original request budget before the collector applies existing failure fencing.

## Constraints

- Only JSON GET requests may repeat after an actual complete429 response.
- At most four total attempts, one original absolute request deadline, cancellable
  waits and existing response/body bounds. Never retry failed/unknown transport.
- Honor integer Retry-After seconds and Matrix retry_after_ms using the larger
  delay. Invalid hints refuse; absent hints use bounded exponential1s backoff.
- POST/PUT/upload/download remain single-attempt with original custody semantics.
- No response is positive authority until the ordinary fresh validation passes.
- No automatic revival of retired routes or replay of old live requests.

## Allowed changes

- native/hagency-matrix/src/http.rs
- native/hagency-matrix/src/http/**
- specs/task-rust-matrix-read-rate-limit.spec.md
- knowledge/decisions/adr-174-matrix-read-rate-limit.md
- docs/**

## Scenarios

Scenario: Read-only429 waits and returns the fresh authenticated response
  Test: native_matrix_get_rate_limit
  Given a bounded local TLS peer returns complete429 then200
  When a JSON GET runs
  Then the same URL/authentication and larger declared delay are preserved
  And the final response is actual200

Scenario: Retry budget and cancellation never grant a fresh deadline
  Test: native_matrix_get_rate_limit_bounds
  Given repeated429, invalid hints, excessive delay or cancellation
  When a GET runs
  Then at most four attempts use one original finite request interval
  And no early retry crosses a server delay or cancellation

Scenario: Mutations and uncertain transport never repeat
  Test: native_matrix_rate_limit_no_write_retry
  Given POST/PUT429 or a lost GET response
  When the original HTTP call finishes
  Then exactly one request was emitted
