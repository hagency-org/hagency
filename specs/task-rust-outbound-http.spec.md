spec: task
name: "Bounded native outbound Palpo HTTP adapter"
inherits: project
satisfies: [REQ-PALPO-OUTBOUND, REQ-RUST-MIGRATION-EXECUTION]
tags: [active, rust, transport, http]
---

## Intent

Send actual authenticated outbound HTTP requests through the durable custody
writer and verify them against local scripted Palpo fixtures. No live service
or domain/Matrix authorization is introduced by these transport tests.

## Constraints

### Must
- Pin the host endpoint fleet registration credential and machine generation without a public configuration route.
- Use verified HTTPS except explicit literal loopback HTTP fixtures and disable environment proxies redirects implicit retries and secret-bearing URLs or logs.
- Bound concurrency header/body reads JSON depth requests deadlines and retry delay.
- Reject duplicate JSON keys coalesced documents malformed envelopes and stale generation or lane responses.
- Persist the complete transaction before sending its exact ACK and resume unconfirmed leases including completed redelivery tombstones after restart.
- Keep Matrix delivery ordering while independent work and publication lanes continue.
- Freeze publication content sequence and observation times before external writes and reconcile exact accepted responses.
- Preserve received work and uncertain ACK/update state across cancellation timeout and token rotation.
- Keep host consumer custody separate from canonical request admission and authenticated Matrix proof.

### Must Not
- Do not call a real homeserver model or deployed service.
- Do not expose machine/lease tokens private owner payload or raw client errors through Debug or public errors.
- Do not add a domain migration or fabricate source membership approval or connection proof.

## Boundaries

### Allowed Changes
- ./Cargo.toml
- ./Cargo.lock
- .github/workflows/rust.yml
- native/hagency-palpo/**
- native/hagency-store/src/outbound.rs
- native/hagency-store/src/outbound/repository.rs
- native/hagency-store/src/outbound/tests.rs
- specs/task-rust-outbound-http.spec.md
- knowledge/decisions/adr-042-native-outbound-http.md
- docs/**

### Forbidden
- Production JavaScript Palpo source live credentials domain schema and other working trees.

## Acceptance Criteria

Scenario: Configured outbound HTTP authority is confined
  Test: native_outbound_http_authority
  Level: integration
  Test Double: local scripted HTTP and TLS listener with synthetic public test credentials
  Given host configuration and local HTTP and TLS fixture listeners
  When endpoints redirects credentials TLS failures and server errors are exercised
  Then requests use only the configured authenticated endpoint and errors disclose no private credentials

Scenario: Poll custody commits before exact ACK and resumes after interruption
  Test: native_outbound_http_custody
  Given real fixture responses and a fresh durable custody store
  When delivery ACK responses disappear cancellation occurs or the adapter restarts
  Then complete payload is durable before ACK and original work plus unconfirmed tombstone leases remain recoverable

Scenario: Wire framing is bounded and unambiguous
  Test: native_outbound_http_wire
  Given empty invalid duplicate oversized coalesced stale and slow responses
  When the adapter reads headers bodies and JSON
  Then only one bounded exact v2 envelope can enter custody and invalid replies produce no ACK

Scenario: Frozen updates retain content across lost responses and rotation
  Test: native_outbound_http_publication
  Level: integration
  Test Double: local scripted Palpo responses with a real SQLite custody writer
  Given persisted host observations and machine generations
  When HTTP update replies are lost delayed rejected repeated or superseded
  Then exact bytes sequence and observedAt are retained and old authority cannot mutate the new scope

Scenario: Independent cancellable lanes enforce finite work
  Test: native_outbound_http_lanes
  Given a blocked Matrix consumer independent work delivery and publication
  When concurrent lane loops encounter delay backoff and cancellation
  Then work and publications progress within bounded requests while stopping preserves durable received work
  And no domain migration or source membership approval or connection proof is fabricated

## Out of Scope

Local fixture TLS proves client validation and protocol behavior only. Real Palpo
deployment compatibility SDK source trust domain admission Agent execution and
live user workflows remain separate M5-M9 gates. The host consumer still needs
authenticated Matrix observations and idempotent domain receipts.
