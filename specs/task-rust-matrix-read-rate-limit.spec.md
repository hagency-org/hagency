spec: task
name: "Bounded Matrix GET rate-limit recovery"
inherits: project
satisfies: [REQ-RUST-MIGRATION-EXECUTION, REQ-LOCAL-THREE-RUNNER-QUALIFICATION]
tags: [active, rust, matrix, recovery]
---

## Intent

An explicit Matrix429 on an observational GET should wait and re-read within
its original request budget before the collector applies existing failure fencing.
So should any JSON request whose connection never existed: nothing was sent, so a
new dial repeats nothing. GETs reuse connections so far fewer dials are needed.

## Constraints

- Only a JSON GET may repeat a request, and only after an actual complete429 response.
- Any JSON request -- GET, POST or PUT -- whose failure is connect-phase is redialled:
  the dial failed before any connection existed, so no request byte left and a new
  dial repeats nothing. A write is therefore still sent at most once. A TLS
  verification failure is a refusal, not a connect-phase failure, and is never
  redialled.
- At most four total attempts across both causes, one original absolute request
  deadline, cancellable waits and existing response/body bounds. A connect-phase
  wait starts at100ms and doubles; one that cannot fit the deadline is not started.
  Never retry any other failed or unknown transport, including a lost response.
- JSON GETs may reuse keep-alive connections of their own client, idle at most10s.
  Every POST/PUT/upload/download dials a fresh connection and closes it, so a stale
  reused connection can never make a write uncertain.
- Honor integer Retry-After seconds and Matrix retry_after_ms using the larger
  delay. Invalid hints refuse; absent hints use bounded exponential1s backoff.
- A POST/PUT429 and every upload/download remain single-attempt, and a write that
  ends in Transport keeps its original custody semantics: the outcome is unknown.
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

Scenario: A GET whose connection never existed is redialled and sent once
  Test: native_matrix_get_connect_retry_reaches_a_late_peer
  Given a loopback port that refuses connections until a peer starts listening
  When a JSON GET runs across that moment
  Then the call is still waiting to redial after the refusal
  And the peer receives exactly one request and the response is actual200

Scenario: Connect-phase redials share the budget and never grant a fresh deadline
  Test: native_matrix_get_connect_retry_bounds
  Given a port that never accepts, a deadline too short for the next wait, or cancellation
  When a GET runs
  Then four dials and three growing waits end in the original Transport failure
  And a wait that cannot fit is not started and a wait in progress is cancellable

Scenario: A write whose dial is refused is redialled and sent exactly once
  Test: native_matrix_write_connect_failure_is_redialled_and_sent_once
  Given a port that refuses connections until a peer starts listening, and one that never accepts
  When POST and PUT run across that moment
  Then each is still waiting to redial after the refusal and the peer receives it once on its own closed connection
  And against the port that never accepts the schedule ends in the original Transport failure

Scenario: A TLS verification failure is refused rather than redialled
  Test: native_matrix_connect_phase_excludes_tls_verification
  Given a refused dial and an endpoint whose certificate is not trusted
  When each failure is classified and the untrusted GET runs
  Then only the refused dial is connect-phase and the untrusted GET fails without the schedule

Scenario: GETs reuse a connection and writes never do
  Test: native_matrix_get_reuses_connections_and_writes_do_not
  Given a keep-alive peer that counts accepted connections
  When three GETs then a PUT and a POST run
  Then the GETs share one connection and each write dials its own and asks for it to be closed
